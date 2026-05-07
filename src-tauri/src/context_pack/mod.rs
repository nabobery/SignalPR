use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const DEFAULT_MAX_BYTES_TOTAL: usize = 16_384; // 16KB
const DEFAULT_MAX_BYTES_PER_ITEM: usize = 4_096; // 4KB

const STANDARD_DOC_NAMES: &[&str] = &[
    "README.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "docs/ARCHITECTURE.md",
    "docs/SECURITY.md",
];

/// GitHub searches in this order: .github/, root, docs/
pub const CODEOWNERS_LOCATIONS_GITHUB: &[&str] =
    &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

/// GitLab searches in this order: root, docs, then .gitlab.
pub const CODEOWNERS_LOCATIONS_GITLAB: &[&str] =
    &["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"];

/// Truncate a UTF-8 string to at most `max_bytes` bytes on a valid char boundary.
pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Return true if `child` is safely inside `root` (no path-traversal escape).
fn is_inside_workspace(root: &Path, child: &Path) -> bool {
    match (root.canonicalize(), child.canonicalize()) {
        (Ok(r), Ok(c)) => c.starts_with(&r),
        _ => {
            // Fall back to component check when canonicalize fails (file doesn't exist yet)
            !child
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
                && !child.is_absolute()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackConfig {
    #[serde(default = "default_max_bytes_total")]
    pub max_bytes_total: usize,
    #[serde(default = "default_max_bytes_per_item")]
    pub max_bytes_per_item: usize,
    #[serde(default = "default_true")]
    pub include_docs: bool,
    #[serde(default = "default_true")]
    pub include_codeowners: bool,
    #[serde(default = "default_true")]
    pub include_preferences: bool,
    #[serde(default)]
    pub include_issue_context: bool,
    #[serde(default)]
    pub additional_docs: Vec<String>,
}

fn default_max_bytes_total() -> usize {
    DEFAULT_MAX_BYTES_TOTAL
}
fn default_max_bytes_per_item() -> usize {
    DEFAULT_MAX_BYTES_PER_ITEM
}
fn default_true() -> bool {
    true
}

impl Default for ContextPackConfig {
    fn default() -> Self {
        Self {
            max_bytes_total: DEFAULT_MAX_BYTES_TOTAL,
            max_bytes_per_item: DEFAULT_MAX_BYTES_PER_ITEM,
            include_docs: true,
            include_codeowners: true,
            include_preferences: true,
            include_issue_context: false,
            additional_docs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub kind: String,
    pub label: String,
    pub source: String,
    pub bytes: usize,
    pub included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    pub total_bytes: usize,
    pub item_count: usize,
    pub items: Vec<ContextItem>,
    pub prompt_suffix: String,
}

pub struct ContextPackBuilder<'a> {
    config: &'a ContextPackConfig,
    workspace_path: &'a Path,
    changed_files: &'a [String],
    preference_text: Option<String>,
    codeowners_override: Option<(String, String)>,
    issue_refs: Vec<IssueRef>,
    items: Vec<ContextItem>,
    used_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRef {
    pub number: String,
    pub title: String,
    pub body_excerpt: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub omit_reason: Option<String>,
    /// Tracker type: github, gitlab, jira, linear
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker: Option<String>,
    /// Confidence level: high, medium, low
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Deep link URL for the issue
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// How the issue was discovered: platform_link, text_ref, url, branch_name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

impl<'a> ContextPackBuilder<'a> {
    pub fn new(
        config: &'a ContextPackConfig,
        workspace_path: &'a Path,
        changed_files: &'a [String],
    ) -> Self {
        Self {
            config,
            workspace_path,
            changed_files,
            preference_text: None,
            codeowners_override: None,
            issue_refs: Vec::new(),
            items: Vec::new(),
            used_bytes: 0,
        }
    }

    pub fn with_preferences(mut self, preference_text: Option<String>) -> Self {
        self.preference_text = preference_text;
        self
    }

    pub fn with_codeowners_content(
        mut self,
        content: Option<String>,
        source: Option<String>,
    ) -> Self {
        if let Some(content) = content {
            self.codeowners_override = Some((
                source.unwrap_or_else(|| "codeowners:override".to_string()),
                content,
            ));
        }
        self
    }

    pub fn with_issues(mut self, issues: Vec<IssueRef>) -> Self {
        self.issue_refs = issues;
        self
    }

    pub fn build(mut self) -> ContextPack {
        let include_issues = self.config.include_issue_context || !self.issue_refs.is_empty();
        if include_issues {
            self.add_issues();
        }
        if self.config.include_docs {
            self.add_docs();
        }
        if self.config.include_codeowners {
            self.add_codeowners();
        }
        if self.config.include_preferences {
            self.add_preferences();
        }

        let prompt_suffix = self.build_prompt_suffix();
        let item_count = self.items.iter().filter(|i| i.included).count();

        ContextPack {
            total_bytes: self.used_bytes,
            item_count,
            items: self.items,
            prompt_suffix,
        }
    }

    fn try_add_item(&mut self, kind: &str, label: &str, source: &str, content: &str) {
        self.try_add_item_with_confidence(kind, label, source, content, None);
    }

    fn try_add_item_with_confidence(
        &mut self,
        kind: &str,
        label: &str,
        source: &str,
        content: &str,
        confidence: Option<String>,
    ) {
        let bytes = content.len();

        if bytes == 0 {
            self.items.push(ContextItem {
                kind: kind.into(),
                label: label.into(),
                source: source.into(),
                bytes: 0,
                included: false,
                omit_reason: Some("empty".into()),
                content: None,
                confidence,
            });
            return;
        }

        let truncated = truncate_utf8(content, self.config.max_bytes_per_item);
        let actual_bytes = truncated.len();

        if self.used_bytes + actual_bytes > self.config.max_bytes_total {
            self.items.push(ContextItem {
                kind: kind.into(),
                label: label.into(),
                source: source.into(),
                bytes,
                included: false,
                omit_reason: Some("budget_exceeded".into()),
                content: None,
                confidence,
            });
            return;
        }

        self.used_bytes += actual_bytes;
        self.items.push(ContextItem {
            kind: kind.into(),
            label: label.into(),
            source: source.into(),
            bytes: actual_bytes,
            included: true,
            omit_reason: None,
            content: Some(truncated.to_string()),
            confidence,
        });
    }

    fn add_docs(&mut self) {
        let mut doc_paths: Vec<String> = STANDARD_DOC_NAMES.iter().map(|s| s.to_string()).collect();
        doc_paths.extend(self.config.additional_docs.iter().cloned());

        for doc_name in &doc_paths {
            let candidate = Path::new(doc_name);
            if candidate.is_absolute()
                || candidate
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                self.items.push(ContextItem {
                    kind: "doc".into(),
                    label: doc_name.clone(),
                    source: doc_name.clone(),
                    bytes: 0,
                    included: false,
                    omit_reason: Some("outside_workspace".into()),
                    content: None,
                    confidence: None,
                });
                continue;
            }

            let full_path = self.workspace_path.join(doc_name);
            if full_path.exists() && !is_inside_workspace(self.workspace_path, &full_path) {
                self.items.push(ContextItem {
                    kind: "doc".into(),
                    label: doc_name.clone(),
                    source: full_path.display().to_string(),
                    bytes: 0,
                    included: false,
                    omit_reason: Some("outside_workspace".into()),
                    content: None,
                    confidence: None,
                });
                continue;
            }

            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    self.try_add_item("doc", doc_name, &full_path.display().to_string(), &content);
                }
                Err(_) => {
                    self.items.push(ContextItem {
                        kind: "doc".into(),
                        label: doc_name.clone(),
                        source: full_path.display().to_string(),
                        bytes: 0,
                        included: false,
                        omit_reason: Some("not_found".into()),
                        content: None,
                        confidence: None,
                    });
                }
            }
        }
    }

    fn add_codeowners(&mut self) {
        let (owners_content, found_path) =
            if let Some((source, content)) = &self.codeowners_override {
                (Some(content.clone()), source.clone())
            } else {
                let mut owners_content: Option<String> = None;
                let mut found_path = String::new();
                for location in CODEOWNERS_LOCATIONS_GITHUB {
                    let full_path = self.workspace_path.join(location);
                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        found_path = full_path.display().to_string();
                        owners_content = Some(content);
                        break;
                    }
                }
                (owners_content, found_path)
            };

        let Some(raw) = owners_content else {
            self.items.push(ContextItem {
                kind: "codeowners".into(),
                label: "CODEOWNERS".into(),
                source: "not found".into(),
                bytes: 0,
                included: false,
                omit_reason: Some("not_found".into()),
                content: None,
                confidence: None,
            });
            return;
        };

        let ownership = resolve_codeowners(&raw, self.changed_files);
        if ownership.is_empty() {
            self.items.push(ContextItem {
                kind: "codeowners".into(),
                label: "CODEOWNERS".into(),
                source: found_path,
                bytes: 0,
                included: false,
                omit_reason: Some("no_matches".into()),
                content: None,
                confidence: None,
            });
            return;
        }

        let mut summary = String::from("File ownership (changed files):\n");
        for (file, owners) in &ownership {
            if owners.is_empty() {
                summary.push_str(&format!("  {} → (no owners)\n", file));
            } else {
                summary.push_str(&format!("  {} → {}\n", file, owners.join(", ")));
            }
        }

        self.try_add_item("codeowners", "CODEOWNERS", &found_path, &summary);
    }

    fn add_preferences(&mut self) {
        let text = self.preference_text.clone();
        if let Some(text) = text {
            if !text.trim().is_empty() {
                self.try_add_item(
                    "preferences",
                    "Reviewer preferences",
                    "preference_summaries",
                    &text,
                );
            }
        }
    }

    fn add_issues(&mut self) {
        use crate::providers::github::{
            MAX_ISSUES, MAX_ISSUE_BODY_EXCERPT_BYTES, MAX_ISSUE_CONTEXT_BYTES_TOTAL,
        };

        let issues: Vec<IssueRef> = self.issue_refs.clone();
        let mut total_issue_bytes: usize = 0;

        for (i, issue) in issues.iter().enumerate() {
            let source = issue_source(issue);
            let label = issue_label(issue);
            let confidence = issue.confidence.clone();
            if i >= MAX_ISSUES {
                self.items.push(ContextItem {
                    kind: "issue".into(),
                    label,
                    source,
                    bytes: 0,
                    included: false,
                    omit_reason: Some("budget_exceeded".into()),
                    content: None,
                    confidence,
                });
                continue;
            }

            if let Some(reason) = issue.omit_reason.clone() {
                self.items.push(ContextItem {
                    kind: "issue".into(),
                    label,
                    source,
                    bytes: 0,
                    included: false,
                    omit_reason: Some(reason),
                    content: None,
                    confidence,
                });
                continue;
            }

            let excerpt = truncate_utf8(&issue.body_excerpt, MAX_ISSUE_BODY_EXCERPT_BYTES);
            let labels_str = if issue.labels.is_empty() {
                String::new()
            } else {
                format!("\nLabels: {}", issue.labels.join(", "))
            };
            let state_str = issue
                .state
                .as_deref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default();

            let content = format!(
                "#{}{}: {}{}{}{}",
                issue.number,
                state_str,
                issue.title,
                labels_str,
                if excerpt.is_empty() { "" } else { "\n" },
                excerpt
            );

            if total_issue_bytes + content.len() > MAX_ISSUE_CONTEXT_BYTES_TOTAL {
                self.items.push(ContextItem {
                    kind: "issue".into(),
                    label,
                    source,
                    bytes: 0,
                    included: false,
                    omit_reason: Some("budget_exceeded".into()),
                    content: None,
                    confidence,
                });
                continue;
            }

            total_issue_bytes += content.len();
            self.try_add_item_with_confidence("issue", &label, &source, &content, confidence);
        }
    }

    fn build_prompt_suffix(&self) -> String {
        let included: Vec<&ContextItem> = self.items.iter().filter(|i| i.included).collect();
        if included.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        parts.push("--- Context Pack ---".to_string());

        for item in &included {
            if let Some(ref content) = item.content {
                parts.push(format!("[{}] {}:", item.kind, item.label));
                parts.push(content.clone());
                parts.push(String::new());
            }
        }

        parts.push("--- End Context Pack ---".to_string());
        parts.join("\n")
    }
}

/// Read CODEOWNERS content from a local workspace directory.
/// Checks `.github/CODEOWNERS`, `CODEOWNERS`, and `docs/CODEOWNERS` in order.
pub fn read_local_codeowners(workspace_path: &Path) -> Option<String> {
    for location in CODEOWNERS_LOCATIONS_GITHUB {
        let full_path = workspace_path.join(location);
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            return Some(content);
        }
    }
    None
}

pub fn normalize_repo_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let without_dot = unified.strip_prefix("./").unwrap_or(&unified);
    let without_side = without_dot
        .strip_prefix("a/")
        .or_else(|| without_dot.strip_prefix("b/"))
        .unwrap_or(without_dot);
    without_side.trim().to_string()
}

/// Simple CODEOWNERS pattern matching (last-match-wins per GitHub spec).
/// Patterns are glob-like: `*.rs` matches all .rs files, `/src/` matches the src directory.
pub fn resolve_codeowners(raw: &str, changed_files: &[String]) -> Vec<(String, Vec<String>)> {
    let rules = parse_codeowners(raw);
    let mut result = BTreeMap::new();

    for file in changed_files {
        let normalized = normalize_repo_path(file);
        if let Some(owners) = match_file(&rules, &normalized) {
            result.insert(normalized, owners);
        }
    }

    result.into_iter().collect()
}

#[derive(Debug)]
struct CodeownersRule {
    pattern: String,
    owners: Vec<String>,
}

fn parse_codeowners(raw: &str) -> Vec<CodeownersRule> {
    raw.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .filter_map(|line| {
            let effective = if let Some(idx) = line.find(" #") {
                &line[..idx]
            } else {
                line
            };
            let parts: Vec<&str> = effective.split_whitespace().collect();
            if parts.is_empty() {
                return None;
            }
            // Per GitHub spec: a pattern with no owners clears ownership for
            // matched files. We emit a rule with an empty `owners` vec so
            // `match_file` can propagate the "no owner" result.
            Some(CodeownersRule {
                pattern: parts[0].to_string(),
                owners: parts[1..].iter().map(|s| s.to_string()).collect(),
            })
        })
        .collect()
}

/// Last-match-wins pattern matching per GitHub CODEOWNERS spec.
fn match_file(rules: &[CodeownersRule], file: &str) -> Option<Vec<String>> {
    let mut matched_owners: Option<Vec<String>> = None;

    for rule in rules {
        if pattern_matches(&rule.pattern, file) {
            matched_owners = Some(rule.owners.clone());
        }
    }

    matched_owners
}

fn pattern_matches(pattern: &str, file: &str) -> bool {
    let pattern = pattern.trim_start_matches('/');

    if pattern == "*" {
        return true;
    }

    // Exact path match
    if file == pattern || file.ends_with(&format!("/{}", pattern)) {
        return true;
    }

    // Directory match: pattern ends with /
    if pattern.ends_with('/') {
        let dir = pattern.trim_end_matches('/');
        return file.starts_with(dir) || file.contains(&format!("/{}/", dir));
    }

    // Extension match: pattern starts with *
    if let Some(ext) = pattern.strip_prefix('*') {
        return file.ends_with(ext);
    }

    // Prefix match: pattern ends with /*
    if let Some(dir) = pattern.strip_suffix("/*") {
        return file.starts_with(&format!("{}/", dir));
    }

    // Directory-recursive match: pattern contains **
    if pattern.contains("**") {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim_end_matches('/');
            let suffix = parts[1].trim_start_matches('/');
            if prefix.is_empty() {
                return suffix.is_empty() || file.ends_with(suffix);
            }
            if !file.starts_with(prefix) {
                return false;
            }
            return suffix.is_empty() || file.ends_with(suffix);
        }
    }

    // Simple prefix match for directory patterns without trailing slash
    if !pattern.contains('*') && !pattern.contains('.') {
        return file.starts_with(&format!("{}/", pattern));
    }

    false
}

/// Extract issue references (`#123`, `owner/repo#456`) from a PR body.
/// Returns strings like `"123"` or `"owner/repo#456"`.
pub fn extract_issue_refs(body: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut chars = body.chars().peekable();
    let mut pos = 0;

    while let Some(ch) = chars.next() {
        if ch == '#' {
            let hash_pos = pos;
            pos += ch.len_utf8();
            let mut num = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    num.push(next);
                    pos += next.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            if !num.is_empty() {
                let ref_str = build_issue_ref(body, hash_pos, &num);
                if !refs.contains(&ref_str) {
                    refs.push(ref_str);
                }
            }
        } else {
            pos += ch.len_utf8();
        }
    }

    refs
}

/// Try to expand a bare `#num` into `owner/repo#num` by looking at the preceding text.
fn build_issue_ref(body: &str, hash_pos: usize, num: &str) -> String {
    let preceding = &body[..hash_pos];
    if let Some(slash_pos) = preceding.rfind('/') {
        let maybe_repo = &preceding[slash_pos + 1..];
        if !maybe_repo.is_empty()
            && maybe_repo
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            // Look for owner before the slash
            let before_slash = &preceding[..slash_pos];
            let owner_start = before_slash
                .rfind(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .map(|i| i + 1)
                .unwrap_or(0);
            let maybe_owner = &before_slash[owner_start..];
            if !maybe_owner.is_empty()
                && maybe_owner
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return format!("{}/{}#{}", maybe_owner, maybe_repo, num);
            }
        }
    }
    num.to_string()
}

fn issue_source(issue: &IssueRef) -> String {
    let tracker = issue.tracker.as_deref().unwrap_or("github");
    match tracker {
        "jira" => {
            if let Some(url) = issue.url.as_deref() {
                let host = url
                    .split("//")
                    .nth(1)
                    .and_then(|s| s.split('/').next())
                    .unwrap_or("unknown");
                format!("jira:issue:{}#{}", host, issue.number)
            } else {
                format!("jira:issue:{}", issue.number)
            }
        }
        "linear" => {
            if let Some(url) = issue.url.as_deref() {
                let workspace = url
                    .split("linear.app/")
                    .nth(1)
                    .and_then(|s| s.split('/').next())
                    .unwrap_or("unknown");
                format!("linear:issue:{}#{}", workspace, issue.number)
            } else {
                format!("linear:issue:{}", issue.number)
            }
        }
        "gitlab" => match (&issue.owner, &issue.repo) {
            (Some(owner), Some(repo)) => {
                format!("gitlab:issue:{}/{}#{}", owner, repo, issue.number)
            }
            _ => format!("gitlab:issue:{}", issue.number),
        },
        _ => match (&issue.owner, &issue.repo) {
            (Some(owner), Some(repo)) => {
                format!("github:issue:{}/{}#{}", owner, repo, issue.number)
            }
            _ => format!("github:issue:{}", issue.number),
        },
    }
}

fn issue_label(issue: &IssueRef) -> String {
    let tracker = issue.tracker.as_deref().unwrap_or("github");
    match tracker {
        "jira" | "linear" => format!("Issue {}", issue.number),
        _ => format!("Issue #{}", issue.number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = ContextPackConfig::default();
        assert_eq!(config.max_bytes_total, 16_384);
        assert_eq!(config.max_bytes_per_item, 4_096);
        assert!(config.include_docs);
        assert!(config.include_codeowners);
        assert!(config.include_preferences);
        assert!(!config.include_issue_context);
    }

    #[test]
    fn test_empty_workspace_builds_empty_pack() {
        let dir = tempdir().unwrap();
        let config = ContextPackConfig::default();
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        assert_eq!(pack.total_bytes, 0);
        assert_eq!(pack.item_count, 0);
        assert!(pack.prompt_suffix.is_empty());
        assert!(!pack.items.is_empty()); // manifest still lists not_found items
    }

    #[test]
    fn test_docs_included_when_present() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# My Project\nSome docs").unwrap();

        let config = ContextPackConfig {
            include_codeowners: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let readme = pack.items.iter().find(|i| i.label == "README.md").unwrap();
        assert!(readme.included);
        assert!(readme.bytes > 0);
        assert!(pack.prompt_suffix.contains("My Project"));
    }

    #[test]
    fn test_per_item_truncation() {
        let dir = tempdir().unwrap();
        let big = "x".repeat(10_000);
        std::fs::write(dir.path().join("README.md"), &big).unwrap();

        let config = ContextPackConfig {
            max_bytes_per_item: 100,
            include_codeowners: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let readme = pack.items.iter().find(|i| i.label == "README.md").unwrap();
        assert!(readme.included);
        assert_eq!(readme.bytes, 100);
    }

    #[test]
    fn test_total_budget_enforced() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "a".repeat(200)).unwrap();
        std::fs::write(dir.path().join("CONTRIBUTING.md"), "b".repeat(200)).unwrap();

        let config = ContextPackConfig {
            max_bytes_total: 300,
            max_bytes_per_item: 500,
            include_codeowners: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let included_count = pack.items.iter().filter(|i| i.included).count();
        assert_eq!(included_count, 1);
        let omitted = pack
            .items
            .iter()
            .find(|i| i.label == "CONTRIBUTING.md")
            .unwrap();
        assert!(!omitted.included);
        assert_eq!(omitted.omit_reason.as_deref(), Some("budget_exceeded"));
    }

    #[test]
    fn test_codeowners_resolved_for_changed_files() {
        let dir = tempdir().unwrap();
        let codeowners = "*.rs @rust-team\nsrc/auth/ @security-team\n";
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/CODEOWNERS"), codeowners).unwrap();

        let changed = vec![
            "src/auth/login.rs".to_string(),
            "docs/readme.md".to_string(),
        ];
        let config = ContextPackConfig {
            include_docs: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &changed).build();

        let co = pack.items.iter().find(|i| i.kind == "codeowners").unwrap();
        assert!(co.included);
        assert!(co.content.as_ref().unwrap().contains("@security-team"));
    }

    #[test]
    fn test_codeowners_last_match_wins() {
        let raw = "* @default\n*.rs @rust-team\nsrc/ @src-team\n";
        let files = vec!["src/main.rs".to_string()];
        let result = resolve_codeowners(raw, &files);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, vec!["@src-team".to_string()]);
    }

    #[test]
    fn test_codeowners_no_match() {
        let raw = "src/ @team\n";
        let files = vec!["docs/readme.md".to_string()];
        let result = resolve_codeowners(raw, &files);
        assert!(result.is_empty());
    }

    #[test]
    fn test_codeowners_wildcard_match() {
        let raw = "* @everyone\n";
        let files = vec!["anything.txt".to_string()];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, vec!["@everyone"]);
    }

    #[test]
    fn test_codeowners_extension_match() {
        let raw = "*.rs @rust-team\n*.ts @ts-team\n";
        let files = vec![
            "src/main.rs".to_string(),
            "src/app.ts".to_string(),
            "docs/guide.md".to_string(),
        ];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_preferences_included() {
        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_preferences(Some("security/auth: accept_rate=0.8 (10 decisions)".into()))
            .build();

        let pref = pack.items.iter().find(|i| i.kind == "preferences").unwrap();
        assert!(pref.included);
        assert!(pack.prompt_suffix.contains("accept_rate"));
    }

    #[test]
    fn test_issue_refs_included() {
        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            include_preferences: false,
            include_issue_context: true,
            ..Default::default()
        };
        let issues = vec![IssueRef {
            number: "42".into(),
            title: "Fix auth bypass".into(),
            body_excerpt: "Users can bypass login".into(),
            owner: None,
            repo: None,
            labels: vec!["bug".into()],
            state: Some("open".into()),
            omit_reason: None,
            tracker: None,
            confidence: None,
            url: None,
            origin: None,
        }];
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_issues(issues)
            .build();

        let issue = pack.items.iter().find(|i| i.kind == "issue").unwrap();
        assert!(issue.included);
        assert!(pack.prompt_suffix.contains("Fix auth bypass"));
    }

    #[test]
    fn test_extract_issue_refs() {
        let body = "Fixes #123 and related to #456. See owner/repo#789.";
        let refs = extract_issue_refs(body);
        assert!(refs.contains(&"123".to_string()));
        assert!(refs.contains(&"456".to_string()));
        assert!(refs.contains(&"owner/repo#789".to_string()));
    }

    #[test]
    fn test_extract_issue_refs_no_refs() {
        let refs = extract_issue_refs("No issues here.");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_issue_refs_dedup() {
        let body = "#123 is the same as #123";
        let refs = extract_issue_refs(body);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn test_prompt_suffix_format() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "Hello").unwrap();

        let config = ContextPackConfig {
            include_codeowners: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        assert!(pack.prompt_suffix.starts_with("--- Context Pack ---"));
        assert!(pack.prompt_suffix.ends_with("--- End Context Pack ---"));
    }

    #[test]
    fn test_manifest_lists_all_items() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "content").unwrap();

        let config = ContextPackConfig::default();
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let found = pack
            .items
            .iter()
            .any(|i| i.label == "README.md" && i.included);
        let missing = pack
            .items
            .iter()
            .any(|i| i.label == "CONTRIBUTING.md" && !i.included);
        assert!(found);
        assert!(missing);
    }

    #[test]
    fn test_parse_codeowners_skips_comments_and_blanks() {
        let raw = "\n# comment\n\n*.rs @team\n";
        let rules = parse_codeowners(raw);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "*.rs");
    }

    #[test]
    fn test_additional_docs() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/API.md"), "API docs").unwrap();

        let config = ContextPackConfig {
            include_codeowners: false,
            include_preferences: false,
            additional_docs: vec!["docs/API.md".into()],
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let api = pack
            .items
            .iter()
            .find(|i| i.label == "docs/API.md")
            .unwrap();
        assert!(api.included);
    }

    // ---- Regression tests for Phase 3 fixes ----

    #[test]
    fn test_utf8_truncation_does_not_panic() {
        // 3-byte UTF-8 char: '€' = 0xE2 0x82 0xAC
        let content = "€€€€€€€€€€"; // 10 euro signs = 30 bytes
        let result = truncate_utf8(content, 7);
        assert!(result.len() <= 7);
        assert!(result.is_char_boundary(result.len()));
        // 7 bytes => 2 full '€' chars (6 bytes)
        assert_eq!(result, "€€");
    }

    #[test]
    fn test_truncation_on_ascii() {
        let content = "hello world";
        assert_eq!(truncate_utf8(content, 5), "hello");
        assert_eq!(truncate_utf8(content, 100), "hello world");
    }

    #[test]
    fn test_path_traversal_rejected() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("secret.txt"), "do not read").unwrap();

        let config = ContextPackConfig {
            include_codeowners: false,
            include_preferences: false,
            include_docs: true,
            additional_docs: vec!["../../secret.txt".into(), "/etc/passwd".into()],
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let traversal = pack
            .items
            .iter()
            .find(|i| i.label == "../../secret.txt")
            .unwrap();
        assert!(!traversal.included);
        assert_eq!(traversal.omit_reason.as_deref(), Some("outside_workspace"));

        let absolute = pack
            .items
            .iter()
            .find(|i| i.label == "/etc/passwd")
            .unwrap();
        assert!(!absolute.included);
        assert_eq!(absolute.omit_reason.as_deref(), Some("outside_workspace"));
    }

    #[test]
    fn test_codeowners_github_dir_wins() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/CODEOWNERS"), "* @github-team\n").unwrap();
        std::fs::write(dir.path().join("CODEOWNERS"), "* @root-team\n").unwrap();

        let files = vec!["any.txt".to_string()];
        let config = ContextPackConfig {
            include_docs: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &files).build();

        let co = pack.items.iter().find(|i| i.kind == "codeowners").unwrap();
        assert!(co.included);
        assert!(co.content.as_ref().unwrap().contains("@github-team"));
        assert!(!co.content.as_ref().unwrap().contains("@root-team"));
    }

    #[test]
    fn test_parse_codeowners_inline_comments() {
        let raw = "*.rs @rust-team # Rust files only\n";
        let rules = parse_codeowners(raw);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "*.rs");
        assert_eq!(rules[0].owners, vec!["@rust-team"]);
    }

    #[test]
    fn test_extract_cross_repo_ref() {
        let body = "See octocat/hello-world#42 for details";
        let refs = extract_issue_refs(body);
        assert!(refs.contains(&"octocat/hello-world#42".to_string()));
    }

    #[test]
    fn test_multibyte_truncation_in_builder() {
        let dir = tempdir().unwrap();
        // 4-byte emoji repeated
        let content = "🎉".repeat(100); // 400 bytes
        std::fs::write(dir.path().join("README.md"), &content).unwrap();

        let config = ContextPackConfig {
            max_bytes_per_item: 10,
            include_codeowners: false,
            include_preferences: false,
            ..Default::default()
        };
        let pack = ContextPackBuilder::new(&config, dir.path(), &[]).build();

        let readme = pack.items.iter().find(|i| i.label == "README.md").unwrap();
        assert!(readme.included);
        // 10 bytes => 2 full 🎉 (8 bytes)
        assert_eq!(readme.bytes, 8);
    }

    // ---- Phase 5: CODEOWNERS golden tests ----

    #[test]
    fn test_codeowners_empty_owner_clears_ownership() {
        let raw = "* @default-team\nsrc/vendored/   \n";
        let rules = parse_codeowners(raw);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].pattern, "src/vendored/");
        assert!(
            rules[1].owners.is_empty(),
            "empty-owner line should produce empty owners vec"
        );

        let files = vec!["src/vendored/lib.js".to_string()];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].1.is_empty(),
            "file under cleared ownership should have no owners"
        );
    }

    #[test]
    fn test_codeowners_last_match_wins_complex() {
        let raw = "* @fallback\n*.rs @rust-team\nsrc/auth/ @security\nsrc/ @src-general\n";
        let files = vec!["src/auth/login.rs".to_string()];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, vec!["@src-general"], "last matching rule wins");
    }

    #[test]
    fn test_codeowners_root_anchored_pattern() {
        // Known limitation: current parser strips leading `/` so root-anchored
        // patterns match nested dirs too. This test documents current behavior.
        let raw = "/docs/ @docs-team\n";
        let files = vec![
            "docs/guide.md".to_string(),
            "src/docs/internal.md".to_string(),
        ];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(
            result.len(),
            2,
            "current parser matches both (known limitation)"
        );
    }

    #[test]
    fn test_codeowners_double_star_glob_literal_suffix() {
        // `**` matching with a literal suffix (no wildcards in suffix)
        let raw = "src/**/config.json @infra-team\n";
        let files = vec![
            "src/features/auth/config.json".to_string(),
            "src/config.json".to_string(),
            "config.json".to_string(),
        ];
        let result = resolve_codeowners(raw, &files);
        assert_eq!(result.len(), 2);
        let matched: Vec<&str> = result.iter().map(|(f, _)| f.as_str()).collect();
        assert!(matched.contains(&"src/features/auth/config.json"));
        assert!(matched.contains(&"src/config.json"));
        assert!(!matched.contains(&"config.json"));
    }

    #[test]
    fn test_codeowners_double_star_no_suffix() {
        let raw = "vendor/** @vendor-team\n";
        let files = vec![
            "vendor/lib.js".to_string(),
            "vendor/deep/nested/file.ts".to_string(),
            "src/vendor/other.js".to_string(),
        ];
        let result = resolve_codeowners(raw, &files);
        let matched: Vec<&str> = result.iter().map(|(f, _)| f.as_str()).collect();
        assert!(matched.contains(&"vendor/lib.js"));
        assert!(matched.contains(&"vendor/deep/nested/file.ts"));
    }

    #[test]
    fn test_codeowners_multiple_owners() {
        let raw = "*.rs @rust-team @security-team @lead\n";
        let rules = parse_codeowners(raw);
        assert_eq!(
            rules[0].owners,
            vec!["@rust-team", "@security-team", "@lead"]
        );
    }

    #[test]
    fn test_codeowners_empty_owner_then_reassigned() {
        let raw = "* @default\nvendor/  \nvendor/critical.js @security\n";
        let files = vec![
            "vendor/lib.js".to_string(),
            "vendor/critical.js".to_string(),
        ];
        let result = resolve_codeowners(raw, &files);
        let lib = result.iter().find(|(f, _)| f == "vendor/lib.js").unwrap();
        assert!(
            lib.1.is_empty(),
            "vendor/lib.js cleared by empty-owner rule"
        );
        let critical = result
            .iter()
            .find(|(f, _)| f == "vendor/critical.js")
            .unwrap();
        assert_eq!(
            critical.1,
            vec!["@security"],
            "vendor/critical.js reassigned"
        );
    }

    // ---- Phase 5: Issue context budget tests ----

    #[test]
    fn test_issue_budget_max_issues_enforced() {
        use crate::providers::github::MAX_ISSUES;

        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            include_preferences: false,
            include_issue_context: true,
            ..Default::default()
        };
        let issues: Vec<IssueRef> = (0..(MAX_ISSUES + 2))
            .map(|i| IssueRef {
                number: format!("{}", i + 1),
                title: format!("Issue {}", i + 1),
                body_excerpt: "Short body".into(),
                owner: None,
                repo: None,
                labels: vec![],
                state: Some("open".into()),
                omit_reason: None,
                tracker: None,
                confidence: None,
                url: None,
                origin: None,
            })
            .collect();
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_issues(issues)
            .build();

        let included = pack
            .items
            .iter()
            .filter(|i| i.kind == "issue" && i.included)
            .count();
        let omitted = pack
            .items
            .iter()
            .filter(|i| i.kind == "issue" && i.omit_reason.as_deref() == Some("budget_exceeded"))
            .count();
        assert_eq!(included, MAX_ISSUES);
        assert_eq!(omitted, 2);
    }

    #[test]
    fn test_issue_body_excerpt_truncated() {
        use crate::providers::github::MAX_ISSUE_BODY_EXCERPT_BYTES;

        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            include_preferences: false,
            include_issue_context: true,
            ..Default::default()
        };
        let big_body = "x".repeat(MAX_ISSUE_BODY_EXCERPT_BYTES + 500);
        let issues = vec![IssueRef {
            number: "1".into(),
            title: "Big".into(),
            body_excerpt: big_body,
            owner: None,
            repo: None,
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: None,
            confidence: None,
            url: None,
            origin: None,
        }];
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_issues(issues)
            .build();

        let item = pack
            .items
            .iter()
            .find(|i| i.kind == "issue" && i.included)
            .unwrap();
        assert!(
            item.bytes <= MAX_ISSUE_BODY_EXCERPT_BYTES + 200,
            "content should be bounded by excerpt truncation"
        );
    }

    #[test]
    fn test_issue_auto_enabled_when_refs_exist() {
        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            include_preferences: false,
            include_issue_context: false,
            ..Default::default()
        };
        let issues = vec![IssueRef {
            number: "99".into(),
            title: "Auto-included issue".into(),
            body_excerpt: "Should appear even when include_issue_context is false".into(),
            owner: None,
            repo: None,
            labels: vec!["enhancement".into()],
            state: Some("open".into()),
            omit_reason: None,
            tracker: None,
            confidence: None,
            url: None,
            origin: None,
        }];
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_issues(issues)
            .build();

        let item = pack.items.iter().find(|i| i.kind == "issue").unwrap();
        assert!(item.included, "issues should auto-include when refs exist");
        assert!(pack.prompt_suffix.contains("Auto-included issue"));
    }

    #[test]
    fn test_issue_labels_and_state_in_content() {
        let dir = tempdir().unwrap();
        let config = ContextPackConfig {
            include_docs: false,
            include_codeowners: false,
            include_preferences: false,
            include_issue_context: true,
            ..Default::default()
        };
        let issues = vec![IssueRef {
            number: "7".into(),
            title: "Security hole".into(),
            body_excerpt: "Details here".into(),
            owner: None,
            repo: None,
            labels: vec!["security".into(), "critical".into()],
            state: Some("open".into()),
            omit_reason: None,
            tracker: None,
            confidence: None,
            url: None,
            origin: None,
        }];
        let pack = ContextPackBuilder::new(&config, dir.path(), &[])
            .with_issues(issues)
            .build();

        let content = &pack.prompt_suffix;
        assert!(content.contains("[open]"), "state should appear in content");
        assert!(
            content.contains("security, critical"),
            "labels should appear in content"
        );
    }

    // ---- Phase 5: read_local_codeowners helper ----

    #[test]
    fn test_read_local_codeowners_from_github_dir() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/CODEOWNERS"), "*.rs @team\n").unwrap();

        let content = read_local_codeowners(dir.path());
        assert!(content.is_some());
        assert!(content.unwrap().contains("@team"));
    }

    #[test]
    fn test_read_local_codeowners_fallback_to_root() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CODEOWNERS"), "*.py @py-team\n").unwrap();

        let content = read_local_codeowners(dir.path());
        assert!(content.is_some());
        assert!(content.unwrap().contains("@py-team"));
    }

    #[test]
    fn test_read_local_codeowners_none_when_missing() {
        let dir = tempdir().unwrap();
        let content = read_local_codeowners(dir.path());
        assert!(content.is_none());
    }

    #[test]
    fn test_codeowners_location_order_github() {
        assert_eq!(
            CODEOWNERS_LOCATIONS_GITHUB,
            &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"]
        );
    }

    #[test]
    fn test_codeowners_location_order_gitlab() {
        assert_eq!(
            CODEOWNERS_LOCATIONS_GITLAB,
            &["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"]
        );
        assert_eq!(
            CODEOWNERS_LOCATIONS_GITLAB[0], "CODEOWNERS",
            "GitLab checks repository root first"
        );
    }

    #[test]
    fn test_codeowners_github_vs_gitlab_first_location_differs() {
        assert_ne!(
            CODEOWNERS_LOCATIONS_GITHUB[0], CODEOWNERS_LOCATIONS_GITLAB[0],
            "GitHub and GitLab should have different first search location"
        );
        assert_eq!(
            CODEOWNERS_LOCATIONS_GITHUB[1], CODEOWNERS_LOCATIONS_GITLAB[0],
            "Both platforms include root CODEOWNERS in early lookup order"
        );
    }

    #[test]
    fn test_issue_source_github_with_owner_repo() {
        let issue = IssueRef {
            number: "42".into(),
            title: "Bug".into(),
            body_excerpt: "".into(),
            owner: Some("acme".into()),
            repo: Some("web".into()),
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: Some("github".into()),
            confidence: Some("high".into()),
            url: Some("https://github.com/acme/web/issues/42".into()),
            origin: Some("linked".into()),
        };
        let source = issue_source(&issue);
        assert_eq!(source, "github:issue:acme/web#42");
    }

    #[test]
    fn test_issue_source_jira_without_url() {
        let issue = IssueRef {
            number: "AUTH-123".into(),
            title: "Login bug".into(),
            body_excerpt: "".into(),
            owner: None,
            repo: None,
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: Some("jira".into()),
            confidence: Some("medium".into()),
            url: None,
            origin: Some("text_ref".into()),
        };
        let source = issue_source(&issue);
        assert_eq!(source, "jira:issue:AUTH-123");
    }

    #[test]
    fn test_issue_source_fallback_no_tracker() {
        let issue = IssueRef {
            number: "99".into(),
            title: "Something".into(),
            body_excerpt: "".into(),
            owner: Some("org".into()),
            repo: Some("repo".into()),
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: None,
            confidence: None,
            url: None,
            origin: None,
        };
        let source = issue_source(&issue);
        assert_eq!(source, "github:issue:org/repo#99");
    }

    #[test]
    fn test_issue_label_github_numeric() {
        let issue = IssueRef {
            number: "42".into(),
            title: "Test".into(),
            body_excerpt: "".into(),
            owner: None,
            repo: None,
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: Some("github".into()),
            confidence: None,
            url: None,
            origin: None,
        };
        assert_eq!(issue_label(&issue), "Issue #42");
    }

    #[test]
    fn test_issue_label_jira_key() {
        let issue = IssueRef {
            number: "AUTH-123".into(),
            title: "Test".into(),
            body_excerpt: "".into(),
            owner: None,
            repo: None,
            labels: vec![],
            state: None,
            omit_reason: None,
            tracker: Some("jira".into()),
            confidence: None,
            url: None,
            origin: None,
        };
        assert_eq!(issue_label(&issue), "Issue AUTH-123");
    }
}
