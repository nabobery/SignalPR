use crate::storage::models::Finding;
use std::collections::HashMap;

#[derive(Debug)]
struct HunkRange {
    new_start: i32,
    new_count: i32,
}

pub fn verify(findings: Vec<Finding>, diff: &str) -> Vec<Finding> {
    let file_hunks = parse_diff_hunks(diff);

    findings
        .into_iter()
        .filter_map(|mut f| {
            let file_path = match &f.file_path {
                Some(fp) => fp.clone(),
                None => return Some(f), // No file path → keep as general finding
            };

            // Check if file exists in diff (try with and without path prefix)
            let hunks = file_hunks
                .get(&file_path)
                .or_else(|| file_hunks.get(&strip_prefix(&file_path)));

            let hunks = match hunks {
                Some(h) => h,
                None => return None, // File not in diff → drop
            };

            // If finding has line range, verify it falls within a hunk
            if let (Some(start), Some(end)) = (f.line_start, f.line_end) {
                let in_hunk = hunks.iter().any(|h| {
                    let hunk_end = h.new_start + h.new_count - 1;
                    start <= hunk_end && end >= h.new_start
                });

                if in_hunk {
                    // Anchored: set diff_new_line to line_start (already in
                    // new-file coordinates from +new_start,new_count) and
                    // diff_side to RIGHT for additions/modifications.
                    f.diff_new_line = Some(start);
                    f.diff_side = Some("RIGHT".to_string());
                } else {
                    // Lines not in any hunk → demote to file-level
                    f.line_start = None;
                    f.line_end = None;
                    f.is_anchored = false;
                    f.diff_new_line = None;
                    f.diff_side = None;
                }
            }

            Some(f)
        })
        .collect()
}

fn strip_prefix(path: &str) -> String {
    // Strip common diff prefixes like "a/" or "b/"
    if path.starts_with("a/") || path.starts_with("b/") {
        path[2..].to_string()
    } else {
        path.to_string()
    }
}

fn parse_diff_hunks(diff: &str) -> HashMap<String, Vec<HunkRange>> {
    let mut result: HashMap<String, Vec<HunkRange>> = HashMap::new();
    let mut current_file: Option<String> = None;

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            // Reset file context at diff boundaries to avoid misattributing hunks.
            current_file = None;
        } else if line.starts_with("+++ ") {
            // Extract file path from "+++ b/path/to/file"
            let path = line.trim_start_matches("+++ ").trim();
            let clean_path = strip_prefix(path);
            if clean_path == "/dev/null" {
                // Deleted file, no file context should remain.
                current_file = None;
            } else {
                current_file = Some(clean_path);
            }
        } else if line.starts_with("@@ ") {
            // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
            if let Some(ref file) = current_file {
                if let Some(hunk) = parse_hunk_header(line) {
                    result.entry(file.clone()).or_default().push(hunk);
                }
            }
        }
    }

    result
}

fn parse_hunk_header(line: &str) -> Option<HunkRange> {
    // Format: @@ -old_start[,old_count] +new_start[,new_count] @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let new_part = parts[2]; // +new_start[,new_count]
    let new_part = new_part.trim_start_matches('+');

    let (new_start, new_count) = if new_part.contains(',') {
        let nums: Vec<&str> = new_part.split(',').collect();
        (nums[0].parse::<i32>().ok()?, nums[1].parse::<i32>().ok()?)
    } else {
        (new_part.parse::<i32>().ok()?, 1)
    };

    Some(HunkRange {
        new_start,
        new_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/auth.rs b/src/auth.rs
index abc1234..def5678 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -10,6 +10,8 @@ fn authenticate() {
     let token = get_token();
+    // Added validation
+    validate_token(&token);
     process(token);
 }
@@ -50,3 +52,5 @@ fn logout() {
     clear_session();
+    log_audit("logout");
+    notify_admin();
 }
diff --git a/src/routes.rs b/src/routes.rs
index 111222..333444 100644
--- a/src/routes.rs
+++ b/src/routes.rs
@@ -5,4 +5,6 @@ fn setup_routes() {
     router.get("/health");
+    router.get("/auth/callback");
+    router.post("/auth/token");
 }
"#;

    const DIFF_WITH_DELETION: &str = r#"diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 1111111..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,5 +0,0 @@
-fn old() {}
diff --git a/src/new.rs b/src/new.rs
index 2222222..3333333 100644
--- a/src/new.rs
+++ b/src/new.rs
@@ -1,3 +1,5 @@
 fn new() {
+    let x = 1;
+    let y = 2;
 }
"#;

    fn make_finding(file: Option<&str>, line_start: Option<i32>, line_end: Option<i32>) -> Finding {
        Finding {
            id: "f1".to_string(),
            review_run_id: "run".to_string(),
            agent_type: "security".to_string(),
            file_path: file.map(|s| s.to_string()),
            line_start,
            line_end,
            severity: "warning".to_string(),
            confidence: 0.8,
            title: "Test finding".to_string(),
            body: "Test body".to_string(),
            evidence: None,
            status: "active".to_string(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: file.is_some() && line_start.is_some(),
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
            fingerprint: None,
        }
    }

    #[test]
    fn test_deleted_file_not_in_hunks() {
        let hunks = parse_diff_hunks(DIFF_WITH_DELETION);
        assert!(!hunks.contains_key("src/old.rs"));
        assert!(hunks.contains_key("src/new.rs"));
    }

    #[test]
    fn test_finding_for_deleted_file_dropped() {
        let findings = vec![make_finding(Some("src/old.rs"), Some(1), Some(1))];
        let result = verify(findings, DIFF_WITH_DELETION);
        assert!(result.is_empty());
    }

    #[test]
    fn test_file_after_deletion_works() {
        let findings = vec![make_finding(Some("src/new.rs"), Some(1), Some(2))];
        let result = verify(findings, DIFF_WITH_DELETION);
        assert_eq!(result.len(), 1);
        assert!(result[0].file_path.as_deref() == Some("src/new.rs"));
    }

    #[test]
    fn test_file_in_diff_lines_in_hunk() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(10), Some(12))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_anchored);
        assert_eq!(result[0].line_start, Some(10));
    }

    #[test]
    fn test_file_in_diff_lines_outside_hunk() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(30), Some(35))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(!result[0].is_anchored);
        assert!(result[0].line_start.is_none()); // demoted to file-level
    }

    #[test]
    fn test_file_not_in_diff_dropped() {
        let findings = vec![make_finding(Some("src/unknown.rs"), Some(1), Some(5))];
        let result = verify(findings, SAMPLE_DIFF);
        assert!(result.is_empty());
    }

    #[test]
    fn test_no_file_path_kept() {
        let findings = vec![make_finding(None, None, None)];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_file_in_diff_no_line_range_kept() {
        let findings = vec![make_finding(Some("src/auth.rs"), None, None)];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_second_file_in_diff() {
        let findings = vec![make_finding(Some("src/routes.rs"), Some(5), Some(7))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_anchored);
    }

    #[test]
    fn test_second_hunk_in_file() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(52), Some(54))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_anchored);
    }

    #[test]
    fn test_parse_diff_hunks() {
        let hunks = parse_diff_hunks(SAMPLE_DIFF);
        assert!(hunks.contains_key("src/auth.rs"));
        assert!(hunks.contains_key("src/routes.rs"));
        assert_eq!(hunks["src/auth.rs"].len(), 2);
        assert_eq!(hunks["src/routes.rs"].len(), 1);
    }

    #[test]
    fn test_empty_diff() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(10), Some(12))];
        let result = verify(findings, "");
        assert!(result.is_empty()); // file not found in empty diff
    }

    #[test]
    fn test_anchored_finding_gets_diff_new_line_and_side() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(10), Some(12))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(result[0].is_anchored);
        assert_eq!(result[0].diff_new_line, Some(10));
        assert_eq!(result[0].diff_side.as_deref(), Some("RIGHT"));
    }

    #[test]
    fn test_demoted_finding_clears_diff_fields() {
        let findings = vec![make_finding(Some("src/auth.rs"), Some(30), Some(35))];
        let result = verify(findings, SAMPLE_DIFF);
        assert_eq!(result.len(), 1);
        assert!(!result[0].is_anchored);
        assert_eq!(result[0].diff_new_line, None);
        assert_eq!(result[0].diff_side, None);
    }

    #[test]
    fn test_file_not_in_diff_drops_entirely() {
        let findings = vec![make_finding(Some("src/missing.rs"), Some(1), Some(5))];
        let result = verify(findings, SAMPLE_DIFF);
        assert!(result.is_empty());
    }
}
