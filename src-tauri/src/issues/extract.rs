use super::types::{IssueCandidate, IssueExtractionConfig};
use regex::Regex;
use std::sync::LazyLock;

static JIRA_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^/]+/browse/([A-Z][A-Z0-9_]+-\d+)").unwrap());

static LINEAR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://linear\.app/([^/]+)/issue/([A-Z][A-Z0-9]+-\d+)").unwrap()
});

static BARE_PROJECT_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z][A-Z0-9_]+-\d+)\b").unwrap());

/// Extract Jira issue keys from URLs in the given text.
pub fn extract_jira_urls(text: &str) -> Vec<IssueCandidate> {
    let mut results = Vec::new();
    for cap in JIRA_URL_RE.captures_iter(text) {
        let full_match = cap.get(0).unwrap().as_str();
        let key = cap[1].to_string();
        results.push(IssueCandidate {
            key,
            tracker: "jira".into(),
            confidence: "high".into(),
            origin: "url".into(),
            url: Some(full_match.to_string()),
            owner: None,
            repo: None,
            omit_reason: None,
        });
    }
    results
}

/// Extract Linear issue identifiers from URLs in the given text.
pub fn extract_linear_urls(text: &str) -> Vec<IssueCandidate> {
    let mut results = Vec::new();
    for cap in LINEAR_URL_RE.captures_iter(text) {
        let full_match = cap.get(0).unwrap().as_str();
        let _workspace = &cap[1];
        let key = cap[2].to_string();
        results.push(IssueCandidate {
            key,
            tracker: "linear".into(),
            confidence: "high".into(),
            origin: "url".into(),
            url: Some(full_match.to_string()),
            owner: None,
            repo: None,
            omit_reason: None,
        });
    }
    results
}

/// Extract bare `KEY-123` tokens from text and classify them using allowlists.
pub fn extract_bare_project_keys(
    text: &str,
    config: &IssueExtractionConfig,
) -> Vec<IssueCandidate> {
    let mut results = Vec::new();
    for cap in BARE_PROJECT_KEY_RE.captures_iter(text) {
        let full_key = cap[1].to_string();
        let prefix = full_key.split('-').next().unwrap_or("");

        let jira_match = config.jira_project_keys.iter().any(|k| k == prefix);
        let linear_match = config.linear_team_keys.iter().any(|k| k == prefix);

        match (jira_match, linear_match) {
            (true, false) => {
                if config.jira_enabled {
                    results.push(IssueCandidate {
                        key: full_key,
                        tracker: "jira".into(),
                        confidence: "medium".into(),
                        origin: "text_ref".into(),
                        url: None,
                        owner: None,
                        repo: None,
                        omit_reason: None,
                    });
                } else {
                    results.push(IssueCandidate {
                        key: full_key,
                        tracker: "jira".into(),
                        confidence: "medium".into(),
                        origin: "text_ref".into(),
                        url: None,
                        owner: None,
                        repo: None,
                        omit_reason: Some("integration_disabled".into()),
                    });
                }
            }
            (false, true) => {
                if config.linear_enabled {
                    results.push(IssueCandidate {
                        key: full_key,
                        tracker: "linear".into(),
                        confidence: "medium".into(),
                        origin: "text_ref".into(),
                        url: None,
                        owner: None,
                        repo: None,
                        omit_reason: None,
                    });
                } else {
                    results.push(IssueCandidate {
                        key: full_key,
                        tracker: "linear".into(),
                        confidence: "medium".into(),
                        origin: "text_ref".into(),
                        url: None,
                        owner: None,
                        repo: None,
                        omit_reason: Some("integration_disabled".into()),
                    });
                }
            }
            (true, true) => {
                results.push(IssueCandidate {
                    key: full_key,
                    tracker: "unknown".into(),
                    confidence: "low".into(),
                    origin: "text_ref".into(),
                    url: None,
                    owner: None,
                    repo: None,
                    omit_reason: Some("ambiguous_issue_key".into()),
                });
            }
            (false, false) => {
                // Key doesn't match any configured allowlist; skip it.
            }
        }
    }
    results
}

/// Run all extractors on a text block, deduplicating by key.
/// URL-based extractions take priority over bare-key extractions.
pub fn extract_all_external_issues(
    text: &str,
    config: &IssueExtractionConfig,
) -> Vec<IssueCandidate> {
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();

    for candidate in extract_jira_urls(text) {
        if seen.insert(candidate.key.clone()) {
            results.push(candidate);
        }
    }

    for candidate in extract_linear_urls(text) {
        if seen.insert(candidate.key.clone()) {
            results.push(candidate);
        }
    }

    for candidate in extract_bare_project_keys(text, config) {
        if seen.insert(candidate.key.clone()) {
            results.push(candidate);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_jira_urls() {
        let text = "See https://mycompany.atlassian.net/browse/AUTH-123 for details";
        let results = extract_jira_urls(text);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "AUTH-123");
        assert_eq!(results[0].tracker, "jira");
        assert_eq!(results[0].confidence, "high");
        assert_eq!(results[0].origin, "url");
        assert!(results[0].url.as_ref().unwrap().contains("AUTH-123"));
    }

    #[test]
    fn test_extract_jira_urls_multiple() {
        let text = "Fixes https://jira.example.com/browse/CORE-42 and https://jira.example.com/browse/CORE-43";
        let results = extract_jira_urls(text);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].key, "CORE-42");
        assert_eq!(results[1].key, "CORE-43");
    }

    #[test]
    fn test_extract_linear_urls() {
        let text = "Related: https://linear.app/acme/issue/ENG-456/fix-login-flow";
        let results = extract_linear_urls(text);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "ENG-456");
        assert_eq!(results[0].tracker, "linear");
        assert_eq!(results[0].confidence, "high");
        assert_eq!(results[0].origin, "url");
    }

    #[test]
    fn test_extract_bare_keys_jira_allowlist() {
        let config = IssueExtractionConfig {
            jira_project_keys: vec!["AUTH".into()],
            linear_team_keys: vec![],
            jira_enabled: true,
            linear_enabled: false,
        };
        let text = "Fixes AUTH-123 and UNKNOWN-456";
        let results = extract_bare_project_keys(text, &config);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "AUTH-123");
        assert_eq!(results[0].tracker, "jira");
        assert!(results[0].omit_reason.is_none());
    }

    #[test]
    fn test_extract_bare_keys_linear_allowlist() {
        let config = IssueExtractionConfig {
            jira_project_keys: vec![],
            linear_team_keys: vec!["ENG".into()],
            jira_enabled: false,
            linear_enabled: true,
        };
        let text = "Closes ENG-789";
        let results = extract_bare_project_keys(text, &config);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "ENG-789");
        assert_eq!(results[0].tracker, "linear");
        assert!(results[0].omit_reason.is_none());
    }

    #[test]
    fn test_extract_bare_keys_ambiguous() {
        let config = IssueExtractionConfig {
            jira_project_keys: vec!["PROJ".into()],
            linear_team_keys: vec!["PROJ".into()],
            jira_enabled: true,
            linear_enabled: true,
        };
        let text = "See PROJ-100";
        let results = extract_bare_project_keys(text, &config);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].omit_reason.as_deref(),
            Some("ambiguous_issue_key")
        );
    }

    #[test]
    fn test_extract_bare_keys_integration_disabled() {
        let config = IssueExtractionConfig {
            jira_project_keys: vec!["AUTH".into()],
            linear_team_keys: vec![],
            jira_enabled: false,
            linear_enabled: false,
        };
        let text = "Fixes AUTH-123";
        let results = extract_bare_project_keys(text, &config);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].omit_reason.as_deref(),
            Some("integration_disabled")
        );
    }

    #[test]
    fn test_extract_all_deduplicates_url_over_bare() {
        let config = IssueExtractionConfig {
            jira_project_keys: vec!["AUTH".into()],
            linear_team_keys: vec![],
            jira_enabled: true,
            linear_enabled: false,
        };
        let text = "Fix https://jira.example.com/browse/AUTH-123 (AUTH-123)";
        let results = extract_all_external_issues(text, &config);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "AUTH-123");
        assert_eq!(results[0].confidence, "high");
        assert_eq!(results[0].origin, "url");
    }

    #[test]
    fn test_no_matches_on_empty_config() {
        let config = IssueExtractionConfig::default();
        let text = "Nothing to find AUTH-123 here";
        let results = extract_all_external_issues(text, &config);
        assert!(results.is_empty());
    }
}
