use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_GRAPHQL_URL: &str = "https://api.github.com/graphql";
const API_VERSION_HEADER: &str = "X-GitHub-Api-Version";
const API_VERSION: &str = "2026-03-10";

// --- Issue context budgets (Phase 5) ---
pub const MAX_ISSUES: usize = 3;
pub const MAX_ISSUE_BODY_EXCERPT_BYTES: usize = 2_048;
pub const MAX_ISSUE_CONTEXT_BYTES_TOTAL: usize = 6_144;

const _: () = {
    assert!(MAX_ISSUES >= 1 && MAX_ISSUES <= 10);
    assert!(MAX_ISSUE_BODY_EXCERPT_BYTES >= 512);
    assert!(MAX_ISSUE_CONTEXT_BYTES_TOTAL >= MAX_ISSUE_BODY_EXCERPT_BYTES);
};

// --- Token sourcing ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar,
    GhCli,
}

pub struct ResolvedToken {
    pub token: String,
    pub source: TokenSource,
}

/// Resolve a GitHub token: env first (`GITHUB_TOKEN`, then `GH_TOKEN`), then `gh auth token` fallback.
pub fn resolve_token_from_env() -> Option<ResolvedToken> {
    if let Ok(t) = env::var("GITHUB_TOKEN") {
        if !t.trim().is_empty() {
            return Some(ResolvedToken {
                token: t.trim().to_string(),
                source: TokenSource::EnvVar,
            });
        }
    }
    if let Ok(t) = env::var("GH_TOKEN") {
        if !t.trim().is_empty() {
            return Some(ResolvedToken {
                token: t.trim().to_string(),
                source: TokenSource::EnvVar,
            });
        }
    }
    None
}

/// Resolve token via `gh auth token` shell command (interactive fallback).
pub async fn resolve_token_from_gh_cli(
    shell: &tauri_plugin_shell::Shell<tauri::Wry>,
) -> Option<ResolvedToken> {
    let output = shell
        .command("gh")
        .args(["auth", "token"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return None;
    }

    Some(ResolvedToken {
        token,
        source: TokenSource::GhCli,
    })
}

/// Resolve an authenticated GitHub API client from env token or gh CLI.
pub async fn resolve_api_from_app(
    app: &AppHandle,
) -> Result<(GitHubApi, TokenSource), crate::errors::AppError> {
    use crate::errors::AppError;

    if let Some(resolved) = resolve_token_from_env() {
        return Ok((GitHubApi::new(resolved.token), resolved.source));
    }

    let shell = app.shell();
    if let Some(resolved) = resolve_token_from_gh_cli(shell).await {
        return Ok((GitHubApi::new(resolved.token), resolved.source));
    }

    Err(AppError::InvalidInput(
        "No GitHub token available. Set GITHUB_TOKEN or GH_TOKEN, or run `gh auth login`.".into(),
    ))
}

// --- API Error types ---

#[derive(Debug, thiserror::Error)]
pub enum GitHubApiError {
    #[error("HTTP {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("Rate limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
    #[error("GraphQL errors: {0}")]
    GraphQL(String),
}

// --- Typed response models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhUser {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhTeam {
    pub slug: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhPullRequest {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub draft: Option<bool>,
    pub user: Option<GhUser>,
    pub head: GhRef,
    pub base: GhRef,
    pub labels: Option<Vec<GhLabel>>,
    pub requested_reviewers: Option<Vec<GhUser>>,
    pub requested_teams: Option<Vec<GhTeam>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhLabel {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReview {
    pub id: i64,
    pub user: Option<GhUser>,
    pub state: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRequestedReviewers {
    pub users: Vec<GhUser>,
    pub teams: Vec<GhTeam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhIssue {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub labels: Option<Vec<GhLabel>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhReviewComment {
    pub id: i64,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i32>,
    pub side: Option<String>,
}

/// Typed metadata snapshot persisted as `platform_metadata_json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformMetadataSnapshot {
    pub pr_body: Option<String>,
    pub head_sha: String,
    pub base_sha: String,
    pub base_ref: String,
    pub head_ref: String,
    pub draft: bool,
    pub labels: Vec<String>,
    pub requested_reviewers: Vec<String>,
    pub requested_teams: Vec<String>,
    pub review_state_summary: Vec<ReviewStateSummary>,
    pub linked_issue_numbers: Vec<i64>,
    pub text_issue_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewStateSummary {
    pub login: String,
    pub state: String,
    pub submitted_at: Option<String>,
}

/// Response wrapper for GitHub review creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhCreatedReview {
    pub id: i64,
}

/// Review comment in the `POST /reviews` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCommentPayload {
    pub path: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_side: Option<String>,
}

/// Payload for creating a coherent review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReviewPayload {
    pub commit_id: String,
    pub body: String,
    pub event: String,
    pub comments: Vec<ReviewCommentPayload>,
}

// --- GitHubApi client ---

pub struct GitHubApi {
    client: reqwest::Client,
    token: String,
}

impl GitHubApi {
    pub fn new(token: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("SignalPR/0.1")
            .build()
            .expect("failed to build reqwest client");
        Self { client, token }
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", self.token)) {
            h.insert(AUTHORIZATION, value);
        }
        h.insert(USER_AGENT, HeaderValue::from_static("SignalPR/0.1"));
        h.insert(API_VERSION_HEADER, HeaderValue::from_static(API_VERSION));
        h
    }

    fn retry_after_secs(headers: &HeaderMap) -> u64 {
        if let Some(retry_after) = headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            return retry_after;
        }

        if let Some(reset_epoch_secs) = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            let now_epoch_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(reset_epoch_secs);
            return reset_epoch_secs.saturating_sub(now_epoch_secs).max(1);
        }

        60
    }

    fn is_rate_limited(status: u16, headers: &HeaderMap) -> bool {
        if status == 429 {
            return true;
        }
        if status != 403 {
            return false;
        }
        headers.contains_key("retry-after")
            || headers
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v == "0")
    }

    async fn check_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, GitHubApiError> {
        let status = resp.status().as_u16();
        if Self::is_rate_limited(status, resp.headers()) {
            let retry_after = Self::retry_after_secs(resp.headers());
            return Err(GitHubApiError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(GitHubApiError::HttpError {
                status,
                message: body,
            });
        }
        Ok(resp)
    }

    // --- REST endpoints ---

    pub async fn get_pull_request(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<GhPullRequest, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn get_requested_reviewers(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<GhRequestedReviewers, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/requested_reviewers",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn list_reviews(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<Vec<GhReview>, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/reviews",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn get_issue(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<GhIssue, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn list_review_comments(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<Vec<GhReviewComment>, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/comments",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn create_review(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
        payload: &CreateReviewPayload,
    ) -> Result<GhCreatedReview, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/reviews",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(payload)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))
    }

    pub async fn get_file_content(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<String>, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/contents/{}",
            GITHUB_API_BASE, owner, repo, path
        );
        let resp = self
            .client
            .get(&url)
            .query(&[("ref", git_ref)])
            .headers(self.headers())
            .header(ACCEPT, "application/vnd.github.raw+json")
            .send()
            .await?;

        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let resp = self.check_response(resp).await?;
        Ok(Some(resp.text().await?))
    }

    pub async fn get_pull_diff(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<String, GitHubApiError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}",
            GITHUB_API_BASE, owner, repo, number
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.headers())
            .header(ACCEPT, "application/vnd.github.diff")
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.text().await?)
    }

    // --- GraphQL ---

    pub async fn get_linked_issues(
        &self,
        owner: &str,
        repo: &str,
        number: i32,
    ) -> Result<Vec<i64>, GitHubApiError> {
        let query = r#"query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                pullRequest(number: $number) {
                    closingIssuesReferences(first: 10, userLinkedOnly: true) {
                        nodes { number }
                    }
                }
            }
        }"#;

        let body = serde_json::json!({
            "query": query,
            "variables": {
                "owner": owner,
                "repo": repo,
                "number": number,
            }
        });

        let resp = self
            .client
            .post(GITHUB_GRAPHQL_URL)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GitHubApiError::JsonParse(e.to_string()))?;

        if let Some(errors) = json.get("errors") {
            return Err(GitHubApiError::GraphQL(errors.to_string()));
        }

        let numbers: Vec<i64> = json
            .pointer("/data/repository/pullRequest/closingIssuesReferences/nodes")
            .and_then(|n| n.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|node| node.get("number").and_then(|n| n.as_i64()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(numbers)
    }

    /// Fetch CODEOWNERS content from the PR base branch.
    /// Checks `.github/CODEOWNERS`, `CODEOWNERS`, and `docs/CODEOWNERS` in order.
    pub async fn get_codeowners(
        &self,
        owner: &str,
        repo: &str,
        base_ref: &str,
    ) -> Result<Option<String>, GitHubApiError> {
        for path in &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"] {
            match self.get_file_content(owner, repo, path, base_ref).await {
                Ok(Some(content)) => return Ok(Some(content)),
                Ok(None) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_token_from_env_github_token() {
        let _guard = EnvGuard::new("GITHUB_TOKEN", "test-gh-token");
        let resolved = resolve_token_from_env();
        assert!(resolved.is_some());
        let r = resolved.unwrap();
        assert_eq!(r.token, "test-gh-token");
        assert_eq!(r.source, TokenSource::EnvVar);
    }

    #[test]
    fn test_resolve_token_from_env_gh_token_fallback() {
        let _guard1 = EnvGuard::new("GITHUB_TOKEN", "");
        let _guard2 = EnvGuard::new("GH_TOKEN", "fallback-token");
        let resolved = resolve_token_from_env();
        assert!(resolved.is_some());
        let r = resolved.unwrap();
        assert_eq!(r.token, "fallback-token");
        assert_eq!(r.source, TokenSource::EnvVar);
    }

    #[test]
    fn test_resolve_token_from_env_none() {
        let _guard1 = EnvGuard::new("GITHUB_TOKEN", "");
        let _guard2 = EnvGuard::new("GH_TOKEN", "");
        let resolved = resolve_token_from_env();
        assert!(resolved.is_none());
    }

    #[test]
    fn test_platform_metadata_snapshot_serialization() {
        let snapshot = PlatformMetadataSnapshot {
            pr_body: Some("test body".into()),
            head_sha: "abc123".into(),
            base_sha: "def456".into(),
            base_ref: "main".into(),
            head_ref: "feature".into(),
            draft: false,
            labels: vec!["bug".into()],
            requested_reviewers: vec!["alice".into()],
            requested_teams: vec!["security-team".into()],
            review_state_summary: vec![ReviewStateSummary {
                login: "bob".into(),
                state: "APPROVED".into(),
                submitted_at: Some("2026-01-01T00:00:00Z".into()),
            }],
            linked_issue_numbers: vec![42],
            text_issue_refs: vec!["123".into()],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: PlatformMetadataSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.head_sha, "abc123");
        assert_eq!(parsed.requested_reviewers, vec!["alice"]);
        assert_eq!(parsed.linked_issue_numbers, vec![42]);
    }

    #[test]
    fn test_create_review_payload_serialization() {
        let payload = CreateReviewPayload {
            commit_id: "abc".into(),
            body: "LGTM".into(),
            event: "APPROVE".into(),
            comments: vec![ReviewCommentPayload {
                path: "src/main.rs".into(),
                body: "Nit".into(),
                line: Some(10),
                side: Some("RIGHT".into()),
                start_line: None,
                start_side: None,
            }],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("APPROVE"));
        assert!(json.contains("src/main.rs"));
        assert!(!json.contains("start_line"));
    }

    #[test]
    fn test_review_comment_multi_line_serialization() {
        let comment = ReviewCommentPayload {
            path: "src/auth.rs".into(),
            body: "This block needs refactoring".into(),
            line: Some(25),
            side: Some("RIGHT".into()),
            start_line: Some(20),
            start_side: Some("RIGHT".into()),
        };
        let json = serde_json::to_string(&comment).unwrap();
        assert!(json.contains("\"start_line\":20"));
        assert!(json.contains("\"start_side\":\"RIGHT\""));
    }

    #[test]
    fn test_review_comment_left_side() {
        let comment = ReviewCommentPayload {
            path: "src/old.rs".into(),
            body: "Deletion concern".into(),
            line: Some(15),
            side: Some("LEFT".into()),
            start_line: None,
            start_side: None,
        };
        let json = serde_json::to_string(&comment).unwrap();
        assert!(json.contains("\"side\":\"LEFT\""));
    }

    #[test]
    fn test_review_comment_suggestion_body() {
        let suggestion_body = "Consider this:\n```suggestion\nlet x = compute();\n```";
        let comment = ReviewCommentPayload {
            path: "src/main.rs".into(),
            body: suggestion_body.into(),
            line: Some(42),
            side: Some("RIGHT".into()),
            start_line: None,
            start_side: None,
        };
        let json = serde_json::to_string(&comment).unwrap();
        assert!(json.contains("suggestion"));
    }

    #[test]
    fn test_platform_metadata_snapshot_defaults() {
        let snapshot = PlatformMetadataSnapshot {
            pr_body: None,
            head_sha: String::new(),
            base_sha: String::new(),
            base_ref: String::new(),
            head_ref: String::new(),
            draft: false,
            labels: vec![],
            requested_reviewers: vec![],
            requested_teams: vec![],
            review_state_summary: vec![],
            linked_issue_numbers: vec![],
            text_issue_refs: vec![],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: PlatformMetadataSnapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.labels.is_empty());
        assert!(parsed.linked_issue_numbers.is_empty());
        assert!(!parsed.draft);
    }

    #[test]
    fn test_create_review_payload_request_changes() {
        let payload = CreateReviewPayload {
            commit_id: "sha123".into(),
            body: "Please address these concerns".into(),
            event: "REQUEST_CHANGES".into(),
            comments: vec![],
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("REQUEST_CHANGES"));
        let parsed: CreateReviewPayload = serde_json::from_str(&json).unwrap();
        assert!(parsed.comments.is_empty());
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &'static str, value: &str) -> Self {
            let original = env::var(key).ok();
            if value.is_empty() {
                env::remove_var(key);
            } else {
                env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => env::set_var(self.key, v),
                None => env::remove_var(self.key),
            }
        }
    }
}
