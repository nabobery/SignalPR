use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use std::env;

const MAX_BODY_EXCERPT_BYTES: usize = 2_048;

#[derive(Debug, Clone)]
pub struct JiraCredentials {
    pub base_url: String,
    pub email: String,
    pub api_token: String,
}

pub fn resolve_jira_credentials_from_env() -> Option<JiraCredentials> {
    let base_url = env::var("JIRA_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().trim_end_matches('/').to_string())?;
    let email = env::var("JIRA_EMAIL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())?;
    let api_token = env::var("JIRA_API_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())?;
    Some(JiraCredentials {
        base_url,
        email,
        api_token,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum JiraApiError {
    #[error("HTTP {status}: {message}")]
    HttpError { status: u16, message: String },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Invalid header value: {0}")]
    InvalidHeader(String),
    #[error("JSON parse error: {0}")]
    JsonParse(String),
}

#[derive(Debug, Clone)]
pub struct JiraIssueInfo {
    pub key: String,
    pub title: String,
    pub body_excerpt: Option<String>,
    pub labels: Vec<String>,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct JiraIssueResponse {
    key: String,
    fields: JiraFields,
}

#[derive(Debug, Deserialize)]
struct JiraFields {
    summary: Option<String>,
    description: Option<serde_json::Value>,
    status: Option<JiraStatus>,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct JiraStatus {
    name: Option<String>,
}

pub struct JiraClient {
    client: reqwest::Client,
    base_url: String,
    auth_header: HeaderValue,
}

impl JiraClient {
    pub fn try_new(credentials: JiraCredentials) -> Result<Self, JiraApiError> {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", credentials.email, credentials.api_token));
        let mut auth_value = HeaderValue::from_str(&format!("Basic {}", encoded))
            .map_err(|e| JiraApiError::InvalidHeader(e.to_string()))?;
        auth_value.set_sensitive(true);

        let client = reqwest::Client::builder()
            .user_agent("SignalPR/0.1")
            .build()?;

        Ok(Self {
            client,
            base_url: credentials.base_url,
            auth_header: auth_value,
        })
    }

    fn headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(AUTHORIZATION, self.auth_header.clone());
        h.insert(USER_AGENT, HeaderValue::from_static("SignalPR/0.1"));
        h
    }

    pub async fn get_issue(&self, issue_key: &str) -> Result<JiraIssueInfo, JiraApiError> {
        let url = format!(
            "{}/rest/api/3/issue/{}?fields=summary,description,status,labels",
            self.base_url, issue_key
        );
        let resp = self.client.get(&url).headers(self.headers()).send().await?;

        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error response body".to_string());
            return Err(JiraApiError::HttpError {
                status,
                message: body,
            });
        }

        let issue: JiraIssueResponse = resp
            .json()
            .await
            .map_err(|e| JiraApiError::JsonParse(e.to_string()))?;

        let title = issue.fields.summary.unwrap_or_default();
        let body_excerpt = extract_description_text(&issue.fields.description);
        let state = issue
            .fields
            .status
            .and_then(|s| s.name)
            .unwrap_or_else(|| "unknown".to_string());

        Ok(JiraIssueInfo {
            key: issue.key,
            title,
            body_excerpt,
            labels: issue.fields.labels,
            state,
        })
    }
}

/// Extract plain text from Jira's ADF (Atlassian Document Format) description,
/// truncating to MAX_BODY_EXCERPT_BYTES.
fn extract_description_text(desc: &Option<serde_json::Value>) -> Option<String> {
    let desc = desc.as_ref()?;
    let mut text = String::new();
    collect_text_from_adf(desc, &mut text);
    if text.is_empty() {
        return None;
    }
    if text.len() > MAX_BODY_EXCERPT_BYTES {
        Some(truncate_utf8_excerpt(&text, MAX_BODY_EXCERPT_BYTES))
    } else {
        Some(text)
    }
}

fn truncate_utf8_excerpt(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut candidate = text[..end].trim_end().to_string();
    if let Some(last_ws) = candidate.rfind(char::is_whitespace) {
        // Prefer cutting on a word boundary when it doesn't remove most content.
        if last_ws >= candidate.len() / 2 {
            candidate.truncate(last_ws);
            candidate = candidate.trim_end().to_string();
        }
    }
    if candidate.is_empty() {
        "…".to_string()
    } else {
        format!("{candidate}…")
    }
}

fn collect_text_from_adf(node: &serde_json::Value, out: &mut String) {
    match node {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(t)) = map.get("text") {
                out.push_str(t);
            }
            if let Some(serde_json::Value::Array(content)) = map.get("content") {
                for child in content {
                    collect_text_from_adf(child, out);
                }
            }
        }
        serde_json::Value::String(s) => {
            out.push_str(s);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_resolve_jira_credentials_missing() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _base_guard = EnvGuard::new("JIRA_BASE_URL", "");
        let _email_guard = EnvGuard::new("JIRA_EMAIL", "");
        let _token_guard = EnvGuard::new("JIRA_API_TOKEN", "");
        assert!(resolve_jira_credentials_from_env().is_none());
    }

    #[test]
    fn test_resolve_jira_credentials_present() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _base_guard = EnvGuard::new("JIRA_BASE_URL", "https://myorg.atlassian.net");
        let _email_guard = EnvGuard::new("JIRA_EMAIL", "user@example.com");
        let _token_guard = EnvGuard::new("JIRA_API_TOKEN", "ATATT3xFfGN...");
        let creds = resolve_jira_credentials_from_env().expect("should resolve");
        assert_eq!(creds.base_url, "https://myorg.atlassian.net");
        assert_eq!(creds.email, "user@example.com");
        assert_eq!(creds.api_token, "ATATT3xFfGN...");
    }

    #[test]
    fn test_resolve_jira_credentials_strips_trailing_slash() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let _base_guard = EnvGuard::new("JIRA_BASE_URL", "https://myorg.atlassian.net/");
        let _email_guard = EnvGuard::new("JIRA_EMAIL", "user@example.com");
        let _token_guard = EnvGuard::new("JIRA_API_TOKEN", "token");
        let creds = resolve_jira_credentials_from_env().expect("should resolve");
        assert_eq!(creds.base_url, "https://myorg.atlassian.net");
    }

    #[test]
    fn test_extract_description_text_none() {
        assert_eq!(extract_description_text(&None), None);
    }

    #[test]
    fn test_extract_description_text_adf() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [
                {
                    "type": "paragraph",
                    "content": [
                        { "type": "text", "text": "Hello " },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        assert_eq!(
            extract_description_text(&Some(adf)),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_extract_description_text_plain_string() {
        let plain = serde_json::Value::String("Just a string".to_string());
        assert_eq!(
            extract_description_text(&Some(plain)),
            Some("Just a string".to_string())
        );
    }

    #[test]
    fn test_extract_description_text_truncates_utf8_safely() {
        let long = "你好🙂".repeat(800);
        let desc = serde_json::Value::String(long);
        let out = extract_description_text(&Some(desc)).expect("excerpt");
        assert!(out.ends_with('…'));
        assert!(out.is_char_boundary(out.len()));
        assert!(out.len() <= MAX_BODY_EXCERPT_BYTES + 4);
    }

    #[test]
    fn test_extract_description_text_prefers_word_boundary() {
        let prefix = "alpha beta gamma delta ";
        let long = format!("{}{}", prefix.repeat(120), "tail");
        let desc = serde_json::Value::String(long);
        let out = extract_description_text(&Some(desc)).expect("excerpt");
        assert!(out.ends_with('…'));
        assert!(!out.ends_with(" …"));
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
