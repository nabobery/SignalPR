use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::storage::hashing::sha256_hex;
use crate::storage::models::Finding;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaDiffSummary {
    pub changed_files: Vec<String>,
    pub changed_hunks_by_file: HashMap<String, Vec<HunkRange>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkRange {
    pub new_start: i32,
    pub new_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingDeltaResult {
    pub new_ids: Vec<String>,
    pub unchanged_ids: Vec<String>,
    pub stale_ids: Vec<String>,
    pub resolved: Vec<ResolvedFindingSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFindingSummary {
    pub id: String,
    pub title: String,
    pub file_path: Option<String>,
    pub agent_type: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDeltaCounts {
    pub new: usize,
    pub unchanged: usize,
    pub stale: usize,
    pub resolved: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDeltaSnapshot {
    pub changed_files: Vec<String>,
    pub changed_hunks_by_file: HashMap<String, Vec<HunkRange>>,
    pub counts: ReviewDeltaCounts,
    pub resolved: Vec<ResolvedFindingSummary>,
}

/// Compute a stable fingerprint for a finding that survives line-number changes.
/// Includes: agent_type, normalized file_path, normalized title, normalized body, severity.
/// Excludes: line numbers, so remaps don't break matching.
pub fn compute_finding_fingerprint(finding: &Finding) -> String {
    let normalized_file = finding
        .file_path
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let normalized_title = finding.title.trim().to_ascii_lowercase();
    let normalized_body = finding.body.trim().to_ascii_lowercase();
    let normalized_severity = finding.severity.trim().to_ascii_lowercase();
    let agent = finding.agent_type.trim().to_ascii_lowercase();

    let input = format!(
        "{}|{}|{}|{}|{}",
        agent, normalized_file, normalized_title, normalized_body, normalized_severity
    );
    sha256_hex(&input)
}

/// Compute which files changed between two unified diffs by comparing
/// per-file section hashes. Returns changed file paths and their hunk ranges.
pub fn compute_changed_files_and_hunks(old_diff: &str, new_diff: &str) -> DeltaDiffSummary {
    let old_sections = parse_diff_sections(old_diff);
    let new_sections = parse_diff_sections(new_diff);

    let mut changed_files = Vec::new();
    let mut changed_hunks_by_file: HashMap<String, Vec<HunkRange>> = HashMap::new();

    for (file, new_content) in &new_sections {
        let new_hash = sha256_hex(new_content);
        let changed = match old_sections.get(file) {
            Some(old_content) => sha256_hex(old_content) != new_hash,
            None => true, // new file not in old diff
        };

        if changed {
            changed_files.push(file.clone());
            let hunks = parse_hunk_ranges(new_content);
            changed_hunks_by_file.insert(file.clone(), hunks);
        }
    }

    // Files removed from new diff (were in old but not new) are also "changed"
    for file in old_sections.keys() {
        if !new_sections.contains_key(file) {
            changed_files.push(file.clone());
        }
    }

    changed_files.sort();
    DeltaDiffSummary {
        changed_files,
        changed_hunks_by_file,
    }
}

/// Classify current findings relative to baseline findings and changed files.
pub fn classify_findings(
    baseline_findings: &[Finding],
    current_findings: &[Finding],
    changed_files: &HashSet<String>,
) -> FindingDeltaResult {
    let baseline_fps: HashMap<&str, &Finding> = baseline_findings
        .iter()
        .filter_map(|f| f.fingerprint.as_deref().map(|fp| (fp, f)))
        .collect();

    let current_fps: HashSet<&str> = current_findings
        .iter()
        .filter_map(|f| f.fingerprint.as_deref())
        .collect();

    let mut new_ids = Vec::new();
    let mut unchanged_ids = Vec::new();
    let mut stale_ids = Vec::new();

    for finding in current_findings {
        let fp = match finding.fingerprint.as_deref() {
            Some(fp) => fp,
            None => {
                new_ids.push(finding.id.clone());
                continue;
            }
        };

        if !baseline_fps.contains_key(fp) {
            new_ids.push(finding.id.clone());
        } else {
            let file_changed = finding
                .file_path
                .as_ref()
                .map(|f| changed_files.contains(f))
                .unwrap_or(false);

            if file_changed || !finding.is_anchored {
                stale_ids.push(finding.id.clone());
            } else {
                unchanged_ids.push(finding.id.clone());
            }
        }
    }

    // Resolved: fingerprints in baseline but not in current
    let resolved: Vec<ResolvedFindingSummary> = baseline_findings
        .iter()
        .filter(|f| {
            f.fingerprint
                .as_deref()
                .map(|fp| !current_fps.contains(fp))
                .unwrap_or(false)
        })
        .map(|f| ResolvedFindingSummary {
            id: f.id.clone(),
            title: f.title.clone(),
            file_path: f.file_path.clone(),
            agent_type: f.agent_type.clone(),
            severity: f.severity.clone(),
        })
        .collect();

    FindingDeltaResult {
        new_ids,
        unchanged_ids,
        stale_ids,
        resolved,
    }
}

/// Parse a unified diff into per-file sections (file path -> raw section text).
fn parse_diff_sections(diff: &str) -> HashMap<String, String> {
    let mut sections: HashMap<String, String> = HashMap::new();
    let mut current_file: Option<String> = None;
    let mut current_content = String::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(file) = current_file.take() {
                sections.insert(file, std::mem::take(&mut current_content));
            }
            current_file = extract_file_path(line);
        }
        if current_file.is_some() {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if let Some(file) = current_file {
        sections.insert(file, current_content);
    }

    sections
}

/// Extract file path from a "diff --git a/path b/path" line.
fn extract_file_path(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(4, ' ').collect();
    if parts.len() >= 4 {
        let b_path = parts[3];
        Some(b_path.strip_prefix("b/").unwrap_or(b_path).to_string())
    } else {
        None
    }
}

/// Parse hunk headers from a diff section to extract ranges.
fn parse_hunk_ranges(section: &str) -> Vec<HunkRange> {
    let mut ranges = Vec::new();
    for line in section.lines() {
        if line.starts_with("@@ ") {
            if let Some(range) = parse_hunk_header(line) {
                ranges.push(range);
            }
        }
    }
    ranges
}

/// Parse "@@ -old_start,old_count +new_start,new_count @@" into HunkRange.
fn parse_hunk_header(line: &str) -> Option<HunkRange> {
    let after_at = line.strip_prefix("@@ ")?;
    let end_at = after_at.find(" @@")?;
    let range_part = &after_at[..end_at];

    // range_part is like "-1,3 +1,4"
    let parts: Vec<&str> = range_part.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let new_part = parts[1].strip_prefix('+')?;
    let (start_str, count_str) = if let Some(comma_pos) = new_part.find(',') {
        (&new_part[..comma_pos], &new_part[comma_pos + 1..])
    } else {
        (new_part, "1")
    };

    let new_start = start_str.parse::<i32>().ok()?;
    let new_count = count_str.parse::<i32>().ok()?;

    Some(HunkRange {
        new_start,
        new_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(
        id: &str,
        agent_type: &str,
        file_path: Option<&str>,
        title: &str,
        body: &str,
        severity: &str,
        fingerprint: Option<&str>,
        is_anchored: bool,
    ) -> Finding {
        Finding {
            id: id.to_string(),
            review_run_id: "run".to_string(),
            agent_type: agent_type.to_string(),
            file_path: file_path.map(|s| s.to_string()),
            line_start: Some(10),
            line_end: Some(20),
            severity: severity.to_string(),
            confidence: 0.8,
            title: title.to_string(),
            body: body.to_string(),
            evidence: None,
            status: "active".to_string(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored,
            created_at: "2026-01-01".to_string(),
            cluster_id: None,
            lane_id: None,
            provider_name: None,
            diff_side: None,
            diff_new_line: None,
            fix_search: None,
            fix_replace: None,
            fix_explanation: None,
            fix_status: None,
            fingerprint: fingerprint.map(|s| s.to_string()),
            source_kind: None,
            source_id: None,
            explain_json: None,
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let f = make_finding(
            "f1",
            "security",
            Some("src/auth.rs"),
            "SQL injection",
            "Use parameterized queries",
            "warning",
            None,
            true,
        );
        let fp1 = compute_finding_fingerprint(&f);
        let fp2 = compute_finding_fingerprint(&f);
        assert_eq!(fp1, fp2);
        assert_eq!(fp1.len(), 64);
    }

    #[test]
    fn fingerprint_ignores_line_numbers() {
        let mut f1 = make_finding(
            "f1",
            "security",
            Some("src/auth.rs"),
            "SQL injection",
            "body",
            "warning",
            None,
            true,
        );
        f1.line_start = Some(10);
        f1.line_end = Some(20);

        let mut f2 = f1.clone();
        f2.line_start = Some(50);
        f2.line_end = Some(60);

        assert_eq!(
            compute_finding_fingerprint(&f1),
            compute_finding_fingerprint(&f2)
        );
    }

    #[test]
    fn fingerprint_changes_with_title() {
        let f1 = make_finding(
            "f1",
            "security",
            Some("src/auth.rs"),
            "SQL injection",
            "body",
            "warning",
            None,
            true,
        );
        let f2 = make_finding(
            "f1",
            "security",
            Some("src/auth.rs"),
            "XSS attack",
            "body",
            "warning",
            None,
            true,
        );
        assert_ne!(
            compute_finding_fingerprint(&f1),
            compute_finding_fingerprint(&f2)
        );
    }

    #[test]
    fn fingerprint_case_insensitive() {
        let f1 = make_finding(
            "f1",
            "Security",
            Some("SRC/Auth.rs"),
            "SQL Injection",
            "Body",
            "Warning",
            None,
            true,
        );
        let f2 = make_finding(
            "f2",
            "security",
            Some("src/auth.rs"),
            "sql injection",
            "body",
            "warning",
            None,
            true,
        );
        assert_eq!(
            compute_finding_fingerprint(&f1),
            compute_finding_fingerprint(&f2)
        );
    }

    #[test]
    fn changed_files_detects_new_file() {
        let old_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n";
        let new_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-old\n+new\ndiff --git a/src/b.rs b/src/b.rs\n--- /dev/null\n+++ b/src/b.rs\n@@ -0,0 +1,5 @@\n+added\n";

        let summary = compute_changed_files_and_hunks(old_diff, new_diff);
        assert!(summary.changed_files.contains(&"src/b.rs".to_string()));
    }

    #[test]
    fn changed_files_detects_modified_file() {
        let old_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n";
        let new_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,4 @@\n-old\n+new\n+extra\n";

        let summary = compute_changed_files_and_hunks(old_diff, new_diff);
        assert!(summary.changed_files.contains(&"src/a.rs".to_string()));
    }

    #[test]
    fn changed_files_unchanged_file_not_included() {
        let diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n";

        let summary = compute_changed_files_and_hunks(diff, diff);
        assert!(summary.changed_files.is_empty());
    }

    #[test]
    fn hunk_ranges_parsed_correctly() {
        let section = "@@ -1,3 +1,4 @@\n-old\n+new\n+extra\n@@ -10,2 +11,5 @@\n+more\n";
        let ranges = parse_hunk_ranges(section);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].new_start, 1);
        assert_eq!(ranges[0].new_count, 4);
        assert_eq!(ranges[1].new_start, 11);
        assert_eq!(ranges[1].new_count, 5);
    }

    #[test]
    fn classify_new_findings() {
        let baseline = vec![make_finding(
            "b1",
            "security",
            Some("src/a.rs"),
            "old issue",
            "body",
            "warning",
            Some("fp_old"),
            true,
        )];
        let current = vec![make_finding(
            "c1",
            "security",
            Some("src/b.rs"),
            "new issue",
            "body",
            "warning",
            Some("fp_new"),
            true,
        )];
        let changed: HashSet<String> = HashSet::new();

        let result = classify_findings(&baseline, &current, &changed);
        assert_eq!(result.new_ids, vec!["c1"]);
        assert!(result.unchanged_ids.is_empty());
        assert!(result.stale_ids.is_empty());
        assert_eq!(result.resolved.len(), 1);
        assert_eq!(result.resolved[0].id, "b1");
    }

    #[test]
    fn classify_unchanged_findings() {
        let fp = compute_finding_fingerprint(&make_finding(
            "x",
            "security",
            Some("src/a.rs"),
            "issue",
            "body",
            "warning",
            None,
            true,
        ));
        let baseline = vec![make_finding(
            "b1",
            "security",
            Some("src/a.rs"),
            "issue",
            "body",
            "warning",
            Some(&fp),
            true,
        )];
        let current = vec![make_finding(
            "c1",
            "security",
            Some("src/a.rs"),
            "issue",
            "body",
            "warning",
            Some(&fp),
            true,
        )];
        let changed: HashSet<String> = HashSet::new();

        let result = classify_findings(&baseline, &current, &changed);
        assert!(result.new_ids.is_empty());
        assert_eq!(result.unchanged_ids, vec!["c1"]);
        assert!(result.stale_ids.is_empty());
        assert!(result.resolved.is_empty());
    }

    #[test]
    fn classify_stale_findings_in_changed_file() {
        let fp = "shared_fp".to_string();
        let baseline = vec![make_finding(
            "b1",
            "security",
            Some("src/a.rs"),
            "issue",
            "body",
            "warning",
            Some(&fp),
            true,
        )];
        let current = vec![make_finding(
            "c1",
            "security",
            Some("src/a.rs"),
            "issue",
            "body",
            "warning",
            Some(&fp),
            true,
        )];
        let mut changed: HashSet<String> = HashSet::new();
        changed.insert("src/a.rs".to_string());

        let result = classify_findings(&baseline, &current, &changed);
        assert!(result.new_ids.is_empty());
        assert!(result.unchanged_ids.is_empty());
        assert_eq!(result.stale_ids, vec!["c1"]);
    }
}
