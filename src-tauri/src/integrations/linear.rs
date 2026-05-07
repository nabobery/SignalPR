use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BODY_EXCERPT_BYTES: usize = 2_048;

#[derive(Debug, Clone)]
pub struct LinearCredentials {
    pub api_key: String,
}

pub fn resolve_linear_credentials(api_key_setting: Option<String>) -> Option<LinearCredentials> {
    if let Ok(env_api_key) = env::var("LINEAR_API_KEY") {
        let trimmed = env_api_key.trim();
        if !trimmed.is_empty() {
            return Some(LinearCredentials {
                api_key: trimmed.to_string(),
            });
        }
    }

    let api_key = api_key_setting
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())?;
    Some(LinearCredentials { api_key })
}

#[derive(Debug, thiserror::Error)]
pub enum LinearApiError {
    #[error("HTTP {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("GraphQL error: {0}")]
    GraphQL(String),
    #[error("Rate limited (retry_after_secs={retry_after_secs:?})")]
    RateLimited { retry_after_secs: Option<u64> },
}

#[derive(Debug, Clone)]
pub struct LinearIssueInfo {
    pub identifier: String,
    pub title: String,
    pub body_excerpt: Option<String>,
    pub labels: Vec<String>,
    pub state: String,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlResponse {
    data: Option<GqlData>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Debug, Deserialize)]
struct GqlData {
    issue: Option<GqlIssue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GqlIssue {
    identifier: String,
    title: String,
    description: Option<String>,
    url: String,
    state: GqlState,
    labels: GqlLabelConnection,
}

#[derive(Debug, Deserialize)]
struct GqlState {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GqlLabelConnection {
    nodes: Vec<GqlLabel>,
}

#[derive(Debug, Deserialize)]
struct GqlLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GqlError {
    message: String,
    extensions: Option<GqlErrorExtensions>,
}

#[derive(Debug, Deserialize)]
struct GqlErrorExtensions {
    code: Option<String>,
}

pub struct LinearClient {
    client: reqwest::Client,
    auth_header: HeaderValue,
}

impl LinearClient {
    pub fn try_new(credentials: LinearCredentials) -> Result<Self, LinearApiError> {
        let mut auth_value =
            HeaderValue::from_str(&credentials.api_key).map_err(|e| LinearApiError::HttpError {
                status: 0,
                message: format!("Invalid API key header: {}", e),
            })?;
        auth_value.set_sensitive(true);

        let client = reqwest::Client::builder()
            .user_agent("SignalPR/0.1")
            .build()?;

        Ok(Self {
            client,
            auth_header: auth_value,
        })
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(AUTHORIZATION, self.auth_header.clone());
        h.insert(USER_AGENT, HeaderValue::from_static("SignalPR/0.1"));
        h
    }

    pub async fn get_issue(&self, identifier: &str) -> Result<LinearIssueInfo, LinearApiError> {
        let query = format!(
            r#"{{ "query": "query {{ issue(id: \"{}\") {{ identifier title description url state {{ name }} labels {{ nodes {{ name }} }} }} }}" }}"#,
            identifier
        );

        let resp = self
            .client
            .post("https://api.linear.app/graphql")
            .headers(self.headers())
            .body(query)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let retry_after_secs = parse_linear_retry_after(resp.headers());
        if status == 429 {
            return Err(LinearApiError::RateLimited { retry_after_secs });
        }
        if !resp.status().is_success() && status != 400 {
            let body = resp.text().await.unwrap_or_default();
            return Err(LinearApiError::HttpError {
                status,
                message: body,
            });
        }

        let gql_resp: GqlResponse = resp
            .json()
            .await
            .map_err(|e| LinearApiError::GraphQL(e.to_string()))?;

        if let Some(errors) = gql_resp.errors {
            let is_rate_limited = errors.iter().any(|e| {
                e.extensions.as_ref().and_then(|ext| ext.code.as_deref()) == Some("RATELIMITED")
            });
            if is_rate_limited {
                return Err(LinearApiError::RateLimited { retry_after_secs });
            }
            let msg = errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(LinearApiError::GraphQL(msg));
        }

        let issue = gql_resp
            .data
            .and_then(|d| d.issue)
            .ok_or_else(|| LinearApiError::GraphQL("Issue not found".into()))?;

        let body_excerpt = issue.description.map(|desc| {
            if desc.len() > MAX_BODY_EXCERPT_BYTES {
                truncate_utf8(&desc, MAX_BODY_EXCERPT_BYTES)
            } else {
                desc
            }
        });

        Ok(LinearIssueInfo {
            identifier: issue.identifier,
            title: issue.title,
            body_excerpt,
            labels: issue.labels.nodes.into_iter().map(|l| l.name).collect(),
            state: issue.state.name,
            url: Some(issue.url),
        })
    }
}

fn parse_linear_retry_after(headers: &HeaderMap) -> Option<u64> {
    if let Some(retry_after_secs) = headers
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Some(retry_after_secs);
    }

    let reset_epoch_ms = headers
        .get("X-RateLimit-Requests-Reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())?;
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)?;

    if reset_epoch_ms <= now_epoch_ms {
        return Some(1);
    }

    let delta_ms = reset_epoch_ms.saturating_sub(now_epoch_ms);
    Some(delta_ms.div_ceil(1000))
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8_ascii() {
        let result = truncate_utf8("hello world", 5);
        assert_eq!(result, "hello…");
    }

    #[test]
    fn test_truncate_utf8_no_truncation() {
        let result = truncate_utf8("short", 100);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        let input = "你好世界abc";
        let result = truncate_utf8(input, 7);
        assert!(result.len() <= 10); // 6 bytes for 2 chars + ellipsis
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_resolve_linear_credentials_from_settings() {
        let original = env::var("LINEAR_API_KEY").ok();
        env::remove_var("LINEAR_API_KEY");
        let creds = resolve_linear_credentials(Some("sk-linear".into())).unwrap();
        assert_eq!(creds.api_key, "sk-linear");
        match original {
            Some(value) => env::set_var("LINEAR_API_KEY", value),
            None => env::remove_var("LINEAR_API_KEY"),
        }
    }
}
