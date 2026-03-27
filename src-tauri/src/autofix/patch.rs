use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSuggestion {
    pub search: String,
    pub replace: String,
    pub file_path: String,
    pub explanation: Option<String>,
}

/// Convert a search/replace fix into a unified diff format string.
///
/// Finds `search` inside `original_content`, and produces a minimal unified
/// diff showing the replacement. Returns `None` when the search text is not
/// found in the original content.
pub fn search_replace_to_unified_diff(
    file_path: &str,
    original_content: &str,
    search: &str,
    replace: &str,
) -> Option<String> {
    // Find the search text in original_content
    let start_byte = original_content.find(search)?;

    // Determine the line number where the match starts (1-indexed)
    let prefix = &original_content[..start_byte];
    let start_line = prefix.matches('\n').count() + 1;

    let search_lines: Vec<&str> = search.lines().collect();
    let replace_lines: Vec<&str> = replace.lines().collect();

    let search_count = search_lines.len();
    let replace_count = replace_lines.len();

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", file_path));
    diff.push_str(&format!("+++ b/{}\n", file_path));
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start_line, search_count, start_line, replace_count,
    ));

    for line in &search_lines {
        diff.push_str(&format!("-{}\n", line));
    }
    for line in &replace_lines {
        diff.push_str(&format!("+{}\n", line));
    }

    Some(diff)
}

/// Validate that the search text exists in the file content.
#[cfg(test)]
fn validate_fix(file_content: &str, search: &str) -> bool {
    file_content.contains(search)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_diff_basic() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let search = "    println!(\"hello\");";
        let replace = "    println!(\"world\");";

        let diff = search_replace_to_unified_diff("src/main.rs", content, search, replace);
        assert!(diff.is_some());
        let diff = diff.unwrap();
        assert!(diff.contains("--- a/src/main.rs"));
        assert!(diff.contains("+++ b/src/main.rs"));
        assert!(diff.contains("-    println!(\"hello\");"));
        assert!(diff.contains("+    println!(\"world\");"));
        assert!(diff.contains("@@ -2,1 +2,1 @@"));
    }

    #[test]
    fn test_unified_diff_not_found() {
        let content = "fn main() {}\n";
        let result =
            search_replace_to_unified_diff("src/main.rs", content, "nonexistent", "replacement");
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_fix_true() {
        let content = "let x = 42;\nlet y = x + 1;";
        assert!(validate_fix(content, "let x = 42;"));
    }

    #[test]
    fn test_validate_fix_false() {
        let content = "let x = 42;";
        assert!(!validate_fix(content, "let y = 99;"));
    }

    #[test]
    fn test_multiline_search_replace() {
        let content = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let search = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}";
        let replace = "fn add(a: i32, b: i32) -> i32 {\n    let result = a + b;\n    result\n}";

        let diff = search_replace_to_unified_diff("src/lib.rs", content, search, replace);
        assert!(diff.is_some());
        let diff = diff.unwrap();
        assert!(diff.contains("@@ -1,3 +1,4 @@"));
        assert!(diff.contains("-fn add(a: i32, b: i32) -> i32 {"));
        assert!(diff.contains("-    a + b"));
        assert!(diff.contains("+    let result = a + b;"));
        assert!(diff.contains("+    result"));
    }

    #[test]
    fn test_fix_suggestion_serde() {
        let fix = FixSuggestion {
            search: "old code".to_string(),
            replace: "new code".to_string(),
            file_path: "src/main.rs".to_string(),
            explanation: Some("Fix the bug".to_string()),
        };
        let json = serde_json::to_string(&fix).unwrap();
        let deserialized: FixSuggestion = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.search, "old code");
        assert_eq!(deserialized.replace, "new code");
        assert_eq!(deserialized.file_path, "src/main.rs");
        assert_eq!(deserialized.explanation.unwrap(), "Fix the bug");
    }
}
