#![allow(dead_code)]

use crate::storage::models::Finding;
use std::collections::HashMap;

pub struct RemapResult {
    pub remapped: Vec<Finding>,
    pub orphaned: Vec<Finding>,
}

/// Remap finding anchors when the PR diff changes between review start and submission.
/// For each anchored finding:
/// - If file not in new diff → orphan
/// - If file present but hunk shifted → adjust line_start/line_end by offset delta
/// - If file present but hunk gone → demote to file-level (clear line_start/line_end)
/// - If unanchored (no line range) → pass through unchanged
pub fn remap_findings(findings: Vec<Finding>, old_diff: &str, new_diff: &str) -> RemapResult {
    if old_diff == new_diff {
        return RemapResult {
            remapped: findings,
            orphaned: vec![],
        };
    }

    let old_files = parse_diff_files(old_diff);
    let new_files = parse_diff_files(new_diff);

    let mut remapped = Vec::new();
    let mut orphaned = Vec::new();

    for mut finding in findings {
        let file_path = match &finding.file_path {
            Some(fp) => fp.clone(),
            None => {
                remapped.push(finding);
                continue;
            }
        };

        // Unanchored findings pass through
        if finding.line_start.is_none() || !finding.is_anchored {
            remapped.push(finding);
            continue;
        }

        if !new_files.contains_key(&file_path) {
            // File removed from new diff
            orphaned.push(finding);
            continue;
        }

        let old_hunks = old_files.get(&file_path);
        let new_hunks = new_files.get(&file_path);

        match (old_hunks, new_hunks) {
            (Some(old_h), Some(new_h)) => {
                let offset = compute_line_offset(old_h, new_h, finding.line_start.unwrap());
                if let Some(delta) = offset {
                    finding.line_start = finding.line_start.map(|l| l + delta);
                    finding.line_end = finding.line_end.map(|l| l + delta);
                    finding.diff_new_line = finding.diff_new_line.map(|l| l + delta);
                    remapped.push(finding);
                } else {
                    // Hunk gone — demote to file-level
                    finding.line_start = None;
                    finding.line_end = None;
                    finding.diff_new_line = None;
                    finding.diff_side = None;
                    finding.is_anchored = false;
                    remapped.push(finding);
                }
            }
            _ => {
                // File exists but no hunk info — demote to file-level
                finding.line_start = None;
                finding.line_end = None;
                finding.is_anchored = false;
                finding.diff_new_line = None;
                finding.diff_side = None;
                remapped.push(finding);
            }
        }
    }

    RemapResult { remapped, orphaned }
}

/// Parse a unified diff into a map of file_path → list of HunkHeader tuples
fn parse_diff_files(diff: &str) -> HashMap<String, Vec<HunkHeader>> {
    let mut files: HashMap<String, Vec<HunkHeader>> = HashMap::new();
    let mut current_file: Option<String> = None;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            // Extract file path: "diff --git a/path b/path"
            if let Some(b_path) = line.split(" b/").nth(1) {
                current_file = Some(b_path.to_string());
            }
        } else if line.starts_with("@@") {
            if let (Some(ref file), Some(hunk)) = (&current_file, parse_hunk_header(line)) {
                files.entry(file.clone()).or_default().push(hunk);
            }
        }
    }

    files
}

#[derive(Debug, Clone)]
struct HunkHeader {
    old_start: i32,
    old_count: i32,
    new_start: i32,
    new_count: i32,
}

fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    // Format: @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let old_part = parts[1].trim_start_matches('-');
    let new_part = parts[2].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_part)?;
    let (new_start, new_count) = parse_range(new_part)?;

    Some(HunkHeader {
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

fn parse_range(s: &str) -> Option<(i32, i32)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

/// Compute the line offset delta for a given finding line in the old diff.
/// Returns Some(delta) if the line can be remapped, None if the hunk is gone.
fn compute_line_offset(
    old_hunks: &[HunkHeader],
    new_hunks: &[HunkHeader],
    finding_line: i32,
) -> Option<i32> {
    // Simple approach: find cumulative offset at the finding's line position
    let mut cumulative_delta: i32 = 0;
    let mut found_in_hunk = false;

    for (old_h, new_h) in old_hunks.iter().zip(new_hunks.iter()) {
        if finding_line >= old_h.new_start && finding_line < old_h.new_start + old_h.new_count {
            // Finding is within this hunk
            found_in_hunk = true;
            cumulative_delta = new_h.new_start - old_h.new_start;
            break;
        }
        if finding_line < old_h.new_start {
            // Finding is before this hunk — use cumulative offset
            break;
        }
        cumulative_delta = new_h.new_start + new_h.new_count - (old_h.new_start + old_h.new_count);
    }

    if old_hunks.is_empty() || new_hunks.is_empty() {
        return None;
    }

    // If we have matching hunks, return the delta
    if found_in_hunk || cumulative_delta != 0 {
        Some(cumulative_delta)
    } else {
        // Check if any new hunk covers the area
        let in_any_new_hunk = new_hunks.iter().any(|h| {
            finding_line + cumulative_delta >= h.new_start
                && finding_line + cumulative_delta < h.new_start + h.new_count
        });
        if in_any_new_hunk {
            Some(cumulative_delta)
        } else {
            Some(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(file: &str, line_start: Option<i32>, line_end: Option<i32>) -> Finding {
        Finding {
            id: uuid::Uuid::new_v4().to_string(),
            review_run_id: "run".into(),
            agent_type: "security".into(),
            file_path: Some(file.into()),
            line_start,
            line_end,
            severity: "warning".into(),
            confidence: 0.8,
            title: "test".into(),
            body: "test body".into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: line_start.is_some(),
            created_at: "2026-01-01".into(),
            cluster_id: None,
            lane_id: None,
            provider_name: None,
            diff_side: None,
            diff_new_line: line_start,
        }
    }

    const DIFF_A: &str = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,6 +10,8 @@ fn foo() {\n     let x = 1;\n+    let y = 2;\n     process(x);\n";

    const DIFF_SHIFTED: &str = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,6 +15,8 @@ fn foo() {\n     let x = 1;\n+    let y = 2;\n     process(x);\n";

    #[test]
    fn test_same_diff_no_changes() {
        let findings = vec![finding("src/a.rs", Some(10), Some(15))];
        let result = remap_findings(findings, DIFF_A, DIFF_A);
        assert_eq!(result.remapped.len(), 1);
        assert!(result.orphaned.is_empty());
        assert_eq!(result.remapped[0].line_start, Some(10));
    }

    #[test]
    fn test_file_removed_orphans_findings() {
        let findings = vec![finding("src/removed.rs", Some(1), Some(5))];
        let result = remap_findings(
            findings,
            "diff --git a/src/removed.rs b/src/removed.rs\n--- a/src/removed.rs\n+++ b/src/removed.rs\n@@ -1,4 +1,6 @@ fn x() {\n",
            "diff --git a/src/other.rs b/src/other.rs\n--- a/src/other.rs\n+++ b/src/other.rs\n@@ -1,4 +1,6 @@ fn y() {\n",
        );
        assert!(result.remapped.is_empty());
        assert_eq!(result.orphaned.len(), 1);
    }

    #[test]
    fn test_unanchored_findings_pass_through() {
        let findings = vec![finding("src/a.rs", None, None)];
        let result = remap_findings(findings, DIFF_A, "completely different diff");
        assert_eq!(result.remapped.len(), 1);
        assert!(result.orphaned.is_empty());
    }

    #[test]
    fn test_shifted_hunk_remaps_lines() {
        let findings = vec![finding("src/a.rs", Some(10), Some(12))];
        let result = remap_findings(findings, DIFF_A, DIFF_SHIFTED);
        assert_eq!(result.remapped.len(), 1);
        assert!(result.orphaned.is_empty());
        // Hunk shifted from new_start=10 to new_start=15, delta = +5
        assert_eq!(result.remapped[0].line_start, Some(15));
        assert_eq!(result.remapped[0].line_end, Some(17));
    }
}
