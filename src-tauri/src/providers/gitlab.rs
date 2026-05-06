use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;

pub const MAX_ISSUE_BODY_EXCERPT_BYTES: usize = 2_048;

// --- Token resolution ---

pub fn resolve_gitlab_token() -> Option<String> {
    env::var("GITLAB_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_string())
}

// --- API Error ---

#[derive(Debug, thiserror::Error)]
pub enum GitLabApiError {
    #[error("HTTP {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("Rate limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
}

// --- Response models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlUser {
    pub username: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlMergeRequest {
    pub iid: i64,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    pub author: Option<GlUser>,
    pub source_branch: String,
    pub target_branch: String,
    pub diff_refs: Option<GlDiffRefs>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub merge_status: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlDiffRefs {
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub start_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlApproval {
    pub approved: bool,
    #[serde(default)]
    pub approved_by: Vec<GlApprovedBy>,
    #[serde(default)]
    pub suggested_approvers: Vec<GlUser>,
    pub approvals_required: Option<i32>,
    pub approvals_left: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlApprovedBy {
    pub user: GlUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlDiscussion {
    pub id: String,
    #[serde(default)]
    pub notes: Vec<GlNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlNote {
    pub id: i64,
    pub body: String,
    pub author: Option<GlUser>,
    pub r#type: Option<String>,
    pub system: Option<bool>,
    pub resolvable: Option<bool>,
    pub resolved: Option<bool>,
    #[serde(default)]
    pub position: Option<GlNotePosition>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlNotePosition {
    pub base_sha: Option<String>,
    pub start_sha: Option<String>,
    pub head_sha: Option<String>,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_line: Option<i32>,
    pub new_line: Option<i32>,
    pub position_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlIssue {
    pub iid: i64,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlReviewer {
    pub username: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

// --- Payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct CreateNotePayload {
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateDiffNotePayload {
    pub body: String,
    pub position: DiffNotePosition,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffNotePosition {
    pub base_sha: String,
    pub start_sha: String,
    pub head_sha: String,
    pub position_type: String,
    pub new_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line: Option<i32>,
}

// --- GitLabApi client ---

pub struct GitLabApi {
    client: reqwest::Client,
    token: String,
    api_base: String,
}

impl GitLabApi {
    pub fn new(token: String, host: &str) -> Result<Self, GitLabApiError> {
        let client = reqwest::Client::builder()
            .user_agent("SignalPR/0.1")
            .build()
            .map_err(GitLabApiError::Network)?;
        let api_base = format!("https://{}/api/v4", host);
        Ok(Self {
            client,
            token,
            api_base,
        })
    }

    fn encoded_path(project_path: &str) -> String {
        urlencoding::encode(project_path).to_string()
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Ok(mut value) = HeaderValue::from_str(&self.token) {
            value.set_sensitive(true);
            h.insert("PRIVATE-TOKEN", value);
        }
        h.insert(USER_AGENT, HeaderValue::from_static("SignalPR/0.1"));
        h
    }

    fn looks_like_unified_diff(text: &str) -> bool {
        let sample = text.trim_start();
        sample.starts_with("diff --git ")
            || sample.starts_with("--- ")
            || sample.contains("\ndiff --git ")
    }

    fn retry_after_secs(headers: &HeaderMap) -> u64 {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60)
    }

    async fn check_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, GitLabApiError> {
        let status = resp.status().as_u16();
        if status == 429 {
            let retry_after = Self::retry_after_secs(resp.headers());
            return Err(GitLabApiError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !resp.status().is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error response body".to_string());
            return Err(GitLabApiError::HttpError {
                status,
                message: body,
            });
        }
        Ok(resp)
    }

    // --- REST endpoints ---

    pub async fn get_merge_request(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<GlMergeRequest, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn get_raw_diffs(
        &self,
        project_path: &str,
        iid: i32,
        host: &str,
    ) -> Result<String, GitLabApiError> {
        let api_url = format!(
            "{}/projects/{}/merge_requests/{}/raw_diffs",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let api_resp = self
            .client
            .get(&api_url)
            .headers(self.headers())
            .send()
            .await?;
        if api_resp.status().is_success() {
            let text = api_resp
                .text()
                .await
                .map_err(|e| GitLabApiError::JsonParse(e.to_string()))?;
            if Self::looks_like_unified_diff(&text) {
                return Ok(text);
            }
            return Err(GitLabApiError::HttpError {
                status: 502,
                message: "GitLab raw_diffs response did not look like unified diff".to_string(),
            });
        }

        if api_resp.status().as_u16() != 404 {
            let status = api_resp.status().as_u16();
            let message = api_resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read GitLab raw_diffs error body".to_string());
            return Err(GitLabApiError::HttpError { status, message });
        }

        // Compatibility fallback for older GitLab instances: web .diff route.
        let url = format!(
            "https://{}/{}/-/merge_requests/{}.diff",
            host, project_path, iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        let text = resp
            .text()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))?;
        if Self::looks_like_unified_diff(&text) {
            Ok(text)
        } else {
            Err(GitLabApiError::HttpError {
                status: 502,
                message: "GitLab .diff fallback returned non-diff content".to_string(),
            })
        }
    }

    pub async fn list_discussions(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<Vec<GlDiscussion>, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/discussions",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn get_approvals(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<GlApproval, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/approvals",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn approve_merge_request(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<(), GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/approve",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn unapprove_merge_request(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<(), GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/unapprove",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn list_reviewers(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<Vec<GlReviewer>, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self
            .client
            .get(&url)
            .query(&[("include_reviewers", "true")])
            .headers(self.headers())
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        let mr: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))?;
        let reviewers_json = mr
            .get("reviewers")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![]));
        if reviewers_json.is_null() {
            return Ok(Vec::new());
        }
        let reviewers: Vec<GlReviewer> = serde_json::from_value(reviewers_json)
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))?;
        Ok(reviewers)
    }

    pub async fn list_notes(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<Vec<GlNote>, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/notes",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn get_issue(&self, project_path: &str, iid: i64) -> Result<GlIssue, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/issues/{}",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn list_closes_issues(
        &self,
        project_path: &str,
        iid: i32,
    ) -> Result<Vec<GlIssue>, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/closes_issues",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn get_file_content(
        &self,
        project_path: &str,
        file_path: &str,
        git_ref: &str,
    ) -> Result<Option<String>, GitLabApiError> {
        let encoded_file = urlencoding::encode(file_path);
        let url = format!(
            "{}/projects/{}/repository/files/{}/raw",
            self.api_base,
            Self::encoded_path(project_path),
            encoded_file
        );
        let resp = self
            .client
            .get(&url)
            .query(&[("ref", git_ref)])
            .headers(self.headers())
            .send()
            .await?;

        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let resp = self.check_response(resp).await?;
        Ok(Some(resp.text().await?))
    }

    pub async fn create_note(
        &self,
        project_path: &str,
        iid: i32,
        body: &str,
    ) -> Result<GlNote, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/notes",
            self.api_base,
            Self::encoded_path(project_path),
            iid
        );
        let payload = CreateNotePayload {
            body: body.to_string(),
        };
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(&payload)
            .send()
            .await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }

    pub async fn create_discussion(
        &self,
        project_path: &str,
        iid: i32,
        payload: &CreateDiffNotePayload,
    ) -> Result<GlDiscussion, GitLabApiError> {
        let url = format!(
            "{}/projects/{}/merge_requests/{}/discussions",
            self.api_base,
            Self::encoded_path(project_path),
            iid
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
            .map_err(|e| GitLabApiError::JsonParse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoded_path_simple() {
        assert_eq!(GitLabApi::encoded_path("group/project"), "group%2Fproject");
    }

    #[test]
    fn test_encoded_path_nested() {
        assert_eq!(
            GitLabApi::encoded_path("org/team/project"),
            "org%2Fteam%2Fproject"
        );
    }

    #[test]
    fn test_resolve_gitlab_token_from_env() {
        // Token resolution depends on env; just verify function signature
        let _result = resolve_gitlab_token();
    }

    #[test]
    fn test_diff_detection_helper() {
        assert!(GitLabApi::looks_like_unified_diff(
            "diff --git a/src/a.rs b/src/a.rs\n@@ -1,1 +1,1 @@"
        ));
        assert!(GitLabApi::looks_like_unified_diff(
            "--- a/src/a.rs\n+++ b/src/a.rs"
        ));
        assert!(!GitLabApi::looks_like_unified_diff(
            "<html>not a diff</html>"
        ));
    }
}
