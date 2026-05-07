use serde::{Deserialize, Serialize};

/// A candidate issue reference extracted from PR metadata or text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCandidate {
    /// The issue identifier (e.g. "42", "AUTH-123", "ENG-456")
    pub key: String,
    /// Tracker type: github, gitlab, jira, linear
    pub tracker: String,
    /// Confidence level: high, medium, low
    pub confidence: String,
    /// How this candidate was discovered
    pub origin: String,
    /// Deep link URL if available
    pub url: Option<String>,
    /// Owner/namespace for GitHub/GitLab issues
    pub owner: Option<String>,
    /// Repository/project for GitHub/GitLab issues
    pub repo: Option<String>,
    /// If the candidate is classified as ambiguous or otherwise un-resolvable
    pub omit_reason: Option<String>,
}

/// Configuration for issue extraction and classification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssueExtractionConfig {
    /// Jira project key allowlist (e.g. ["AUTH", "CORE"])
    #[serde(default)]
    pub jira_project_keys: Vec<String>,
    /// Linear team key allowlist (e.g. ["ENG", "PLAT"])
    #[serde(default)]
    pub linear_team_keys: Vec<String>,
    /// Whether Jira integration is enabled
    #[serde(default)]
    pub jira_enabled: bool,
    /// Whether Linear integration is enabled
    #[serde(default)]
    pub linear_enabled: bool,
}
