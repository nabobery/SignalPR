use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::env;
use url::Url;

const BITBUCKET_API_BASE: &str = "https://api.bitbucket.org/2.0";

// --- Credential resolution ---

pub struct BitbucketCredentials {
    pub email: String,
    pub token: String,
}

pub fn resolve_bitbucket_credentials_from_env() -> Option<BitbucketCredentials> {
    let email = env::var("BITBUCKET_EMAIL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())?;
    let token = env::var("BITBUCKET_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())?;
    Some(BitbucketCredentials { email, token })
}

// --- API Error ---

#[derive(Debug, thiserror::Error)]
pub enum BitbucketApiError {
    #[error("HTTP {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("Rate limited; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Invalid header value: {0}")]
    InvalidHeader(String),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
}

// --- Response models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbUser {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

impl BbUser {
    pub fn best_name(&self) -> String {
        self.nickname
            .as_deref()
            .or(self.display_name.as_deref())
            .unwrap_or("unknown")
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbPullRequest {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    pub author: Option<BbUser>,
    pub source: BbRef,
    pub destination: BbRef,
    #[serde(default)]
    pub reviewers: Vec<BbUser>,
    #[serde(default)]
    pub participants: Vec<BbParticipant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbRef {
    pub branch: BbBranch,
    pub commit: BbCommit,
    pub repository: Option<BbRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbBranch {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbCommit {
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbRepository {
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbParticipant {
    pub user: BbUser,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbComment {
    pub id: i64,
    pub content: BbContent,
    #[serde(default)]
    pub inline: Option<BbInline>,
    pub user: Option<BbUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbContent {
    pub raw: String,
    #[serde(default)]
    pub markup: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbInline {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub from: Option<i32>,
    #[serde(default)]
    pub to: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbTask {
    pub id: i64,
    pub content: BbContent,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub comment: Option<BbTaskCommentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbTaskCommentRef {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbDefaultReviewer {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BbPaginatedResponse<T> {
    pub values: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
}

// --- Payloads ---

#[derive(Debug, Clone, Serialize)]
pub struct CreateCommentPayload {
    pub content: CreateContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline: Option<CreateInline>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateContent {
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateInline {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_to: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateTaskPayload {
    pub content: CreateContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<BbTaskCommentRef>,
}

// --- BitbucketApi client ---

pub struct BitbucketApi {
    client: reqwest::Client,
    auth_header: HeaderValue,
}

impl BitbucketApi {
    pub fn try_new(credentials: BitbucketCredentials) -> Result<Self, BitbucketApiError> {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", credentials.email, credentials.token));
        let mut auth_value = HeaderValue::from_str(&format!("Basic {}", encoded))
            .map_err(|e| BitbucketApiError::InvalidHeader(e.to_string()))?;
        auth_value.set_sensitive(true);

        let client = reqwest::Client::builder()
            .user_agent("SignalPR/0.1")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;

        Ok(Self {
            client,
            auth_header: auth_value,
        })
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h.insert(AUTHORIZATION, self.auth_header.clone());
        h.insert(USER_AGENT, HeaderValue::from_static("SignalPR/0.1"));
        h
    }

    async fn check_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, BitbucketApiError> {
        let status = resp.status().as_u16();
        if status == 429 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            return Err(BitbucketApiError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !resp.status().is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error response body".to_string());
            return Err(BitbucketApiError::HttpError {
                status,
                message: body,
            });
        }
        Ok(resp)
    }

    async fn paginate_all<T: serde::de::DeserializeOwned>(
        &self,
        initial_url: &str,
    ) -> Result<Vec<T>, BitbucketApiError> {
        let mut all = Vec::new();
        let mut url = initial_url.to_string();
        loop {
            let resp = self.client.get(&url).headers(self.headers()).send().await?;
            let resp = self.check_response(resp).await?;
            let page: BbPaginatedResponse<T> = resp
                .json()
                .await
                .map_err(|e| BitbucketApiError::JsonParse(e.to_string()))?;
            all.extend(page.values);
            match page.next {
                Some(next_url) if !next_url.is_empty() => url = next_url,
                _ => break,
            }
        }
        Ok(all)
    }

    fn build_api_url(
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
    ) -> Result<String, BitbucketApiError> {
        let mut url = Url::parse(BITBUCKET_API_BASE)
            .map_err(|e| BitbucketApiError::InvalidUrl(e.to_string()))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| BitbucketApiError::InvalidUrl("base URL cannot be a base".into()))?;
            path.clear();
            for segment in segments {
                path.push(segment);
            }
        }
        if let Some(params) = query {
            url.query_pairs_mut().extend_pairs(params.iter().copied());
        }
        Ok(url.to_string())
    }

    fn build_src_file_url(
        workspace: &str,
        repo_slug: &str,
        commit_or_ref: &str,
        path: &str,
    ) -> Result<String, BitbucketApiError> {
        if path.trim().is_empty() {
            return Err(BitbucketApiError::InvalidUrl(
                "file path cannot be empty".to_string(),
            ));
        }
        let mut segments = vec![
            "repositories".to_string(),
            workspace.to_string(),
            repo_slug.to_string(),
            "src".to_string(),
            commit_or_ref.to_string(),
        ];
        segments.extend(
            path.split('/')
                .filter(|s| !s.is_empty())
                .map(ToString::to_string),
        );
        let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
        Self::build_api_url(&refs, None)
    }

    // --- REST endpoints ---

    pub async fn get_pull_request(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<BbPullRequest, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &["repositories", workspace, repo_slug, "pullrequests", &pr_id],
            None,
        )?;
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        let resp = self.check_response(resp).await?;
        resp.json()
            .await
            .map_err(|e| BitbucketApiError::JsonParse(e.to_string()))
    }

    pub async fn get_pull_request_diff(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<String, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "diff",
            ],
            None,
        )?;
        let mut headers = self.headers();
        headers.insert(ACCEPT, HeaderValue::from_static("text/plain"));
        let resp = self.client.get(&url).headers(headers).send().await?;
        let resp = self.check_response(resp).await?;
        Ok(resp.text().await?)
    }

    pub async fn list_pull_request_comments(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<Vec<BbComment>, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "comments",
            ],
            Some(&[("pagelen", "100")]),
        )?;
        self.paginate_all(&url).await
    }

    pub async fn create_pull_request_comment(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
        payload: &CreateCommentPayload,
    ) -> Result<BbComment, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "comments",
            ],
            None,
        )?;
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
            .map_err(|e| BitbucketApiError::JsonParse(e.to_string()))
    }

    pub async fn approve_pull_request(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<(), BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "approve",
            ],
            None,
        )?;
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn unapprove_pull_request(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<(), BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "approve",
            ],
            None,
        )?;
        let resp = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .await?;
        // Unapprove returns 404 if user hasn't approved — that's fine.
        if resp.status().as_u16() == 404 {
            return Ok(());
        }
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn list_default_reviewers(
        &self,
        workspace: &str,
        repo_slug: &str,
    ) -> Result<Vec<BbDefaultReviewer>, BitbucketApiError> {
        let url = Self::build_api_url(
            &["repositories", workspace, repo_slug, "default-reviewers"],
            Some(&[("pagelen", "100")]),
        )?;
        self.paginate_all(&url).await
    }

    pub async fn list_tasks(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<Vec<BbTask>, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "tasks",
            ],
            Some(&[("pagelen", "100")]),
        )?;
        self.paginate_all(&url).await
    }

    pub async fn create_task(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
        payload: &CreateTaskPayload,
    ) -> Result<BbTask, BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "tasks",
            ],
            None,
        )?;
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
            .map_err(|e| BitbucketApiError::JsonParse(e.to_string()))
    }

    pub async fn request_changes_pull_request(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<(), BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "request-changes",
            ],
            None,
        )?;
        let resp = self
            .client
            .post(&url)
            .headers(self.headers())
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn remove_change_request_pull_request(
        &self,
        workspace: &str,
        repo_slug: &str,
        pr_id: i32,
    ) -> Result<(), BitbucketApiError> {
        let pr_id = pr_id.to_string();
        let url = Self::build_api_url(
            &[
                "repositories",
                workspace,
                repo_slug,
                "pullrequests",
                &pr_id,
                "request-changes",
            ],
            None,
        )?;
        let resp = self
            .client
            .delete(&url)
            .headers(self.headers())
            .send()
            .await?;
        if resp.status().as_u16() == 404 {
            return Ok(());
        }
        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn get_file_content_via_src(
        &self,
        workspace: &str,
        repo_slug: &str,
        commit_or_ref: &str,
        path: &str,
    ) -> Result<Option<String>, BitbucketApiError> {
        let url = Self::build_src_file_url(workspace, repo_slug, commit_or_ref, path)?;
        let resp = self.client.get(&url).headers(self.headers()).send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let resp = self.check_response(resp).await?;
        Ok(Some(resp.text().await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_resolve_credentials_missing() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _email_guard = EnvGuard::new("BITBUCKET_EMAIL", "");
        let _token_guard = EnvGuard::new("BITBUCKET_TOKEN", "");
        assert!(resolve_bitbucket_credentials_from_env().is_none());
    }

    #[test]
    fn test_resolve_credentials_present() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _email_guard = EnvGuard::new("BITBUCKET_EMAIL", "user@example.com");
        let _token_guard = EnvGuard::new("BITBUCKET_TOKEN", "ATCTT3xFfGN...");
        let creds = resolve_bitbucket_credentials_from_env().expect("should resolve");
        assert_eq!(creds.email, "user@example.com");
        assert_eq!(creds.token, "ATCTT3xFfGN...");
    }

    #[test]
    fn test_resolve_credentials_only_email_missing_token() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _email_guard = EnvGuard::new("BITBUCKET_EMAIL", "user@example.com");
        let _token_guard = EnvGuard::new("BITBUCKET_TOKEN", "");
        assert!(resolve_bitbucket_credentials_from_env().is_none());
    }

    #[test]
    fn test_bb_user_best_name_nickname() {
        let user = BbUser {
            display_name: Some("John Doe".into()),
            nickname: Some("johndoe".into()),
            uuid: None,
            account_id: None,
        };
        assert_eq!(user.best_name(), "johndoe");
    }

    #[test]
    fn test_bb_user_best_name_display_name_fallback() {
        let user = BbUser {
            display_name: Some("John Doe".into()),
            nickname: None,
            uuid: None,
            account_id: None,
        };
        assert_eq!(user.best_name(), "John Doe");
    }

    #[test]
    fn test_bb_user_best_name_unknown_fallback() {
        let user = BbUser {
            display_name: None,
            nickname: None,
            uuid: None,
            account_id: None,
        };
        assert_eq!(user.best_name(), "unknown");
    }

    #[test]
    fn test_auth_header_construction() {
        use base64::Engine;
        let creds = BitbucketCredentials {
            email: "test@example.com".into(),
            token: "mytoken".into(),
        };
        let expected = base64::engine::general_purpose::STANDARD.encode("test@example.com:mytoken");
        let api = BitbucketApi::try_new(creds).expect("api");
        let headers = api.headers();
        let auth = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth, format!("Basic {}", expected));
    }

    #[test]
    fn test_build_src_file_url_encodes_ref_and_path_segments() {
        let url = BitbucketApi::build_src_file_url(
            "acme",
            "repo",
            "feature/JIRA-42-login",
            "dir with spaces/file name.rs",
        )
        .expect("url");
        assert!(url.contains("feature%2FJIRA-42-login"));
        assert!(url.contains("dir%20with%20spaces/file%20name.rs"));
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
