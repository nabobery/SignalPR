use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::errors::ProviderError;
use crate::secrets::credentials::{self, ProviderCredentialField};

use super::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

const DEFAULT_MODEL: &str = "claude-sonnet-4-5-20250929";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_RETRIES: u32 = 2;

/// Claude provider using direct HTTP requests to the Anthropic Messages API.
/// Uses tool_use for structured output matching the CodexReviewOutput schema.
///
/// API key sourced from `ANTHROPIC_API_KEY` env var (dev).
/// OS keychain integration planned for Phase 3.
pub struct ClaudeProvider {
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl ClaudeProvider {
    fn resolved_api_key() -> Option<String> {
        credentials::resolve_credential(ProviderCredentialField::AnthropicApiKey)
            .ok()
            .and_then(|(value, _)| value)
    }

    pub fn new() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            api_key: Self::resolved_api_key(),
            client: reqwest::Client::new(),
        }
    }

    #[allow(dead_code)]
    pub fn with_model(model: String) -> Self {
        Self {
            model,
            api_key: Self::resolved_api_key(),
            client: reqwest::Client::new(),
        }
    }
}

// --- Anthropic API request/response types ---

#[derive(Serialize)]
struct ToolChoice {
    #[serde(rename = "type")]
    choice_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
    tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct Tool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        #[allow(dead_code)]
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Deserialize)]
struct ApiError {
    error: ApiErrorDetail,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

#[async_trait]
impl ReviewProvider for ClaudeProvider {
    fn provider_name(&self) -> &str {
        "claude"
    }

    async fn health_check(&self) -> ProviderHealth {
        match &self.api_key {
            Some(_) => ProviderHealth {
                available: true,
                version: Some(self.model.clone()),
                message: None,
            },
            None => ProviderHealth {
                available: false,
                version: None,
                message: Some("ANTHROPIC_API_KEY not set".into()),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        _cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ProviderError::ClaudeFailed("ANTHROPIC_API_KEY not set".into()))?;

        // Parse the output schema for tool definition
        let schema: serde_json::Value = serde_json::from_str(&input.output_schema)
            .map_err(|e| ProviderError::ClaudeFailed(format!("Invalid output schema: {}", e)))?;

        let request = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".into(),
                content: format!("{}\n\nPR Diff:\n{}", input.system_prompt, input.diff),
            }],
            tools: vec![Tool {
                name: "submit_review".into(),
                description: "Submit the code review findings in structured format".into(),
                input_schema: schema,
            }],
            tool_choice: Some(ToolChoice {
                choice_type: "tool".into(),
                name: Some("submit_review".into()),
            }),
        };

        let mut last_error = None;

        for attempt in 0..=MAX_RETRIES {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }

            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                result = self.client
                    .post(API_URL)
                    .header("x-api-key", api_key)
                    .header("anthropic-version", API_VERSION)
                    .header("content-type", "application/json")
                    .json(&request)
                    .send() => result,
            };

            match response {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        let body = resp.text().await.map_err(|e| {
                            ProviderError::ClaudeFailed(format!("Read error: {}", e))
                        })?;
                        return parse_response(&body);
                    }

                    let body = resp.text().await.unwrap_or_default();

                    // Rate limit: retry with backoff
                    if status.as_u16() == 429 && attempt < MAX_RETRIES {
                        let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                        tracing::warn!(
                            "Claude rate limited (attempt {}), retrying in {:?}",
                            attempt + 1,
                            delay
                        );
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                            _ = tokio::time::sleep(delay) => {},
                        }
                        last_error = Some(format!("Rate limited: {}", body));
                        continue;
                    }

                    // Parse API error
                    if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                        return Err(ProviderError::ClaudeFailed(format!(
                            "{}: {}",
                            api_err.error.error_type, api_err.error.message
                        )));
                    }

                    return Err(ProviderError::ClaudeFailed(format!(
                        "HTTP {}: {}",
                        status, body
                    )));
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt));
                        tracing::warn!(
                            "Claude request failed (attempt {}): {}, retrying in {:?}",
                            attempt + 1,
                            e,
                            delay
                        );
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                            _ = tokio::time::sleep(delay) => {},
                        }
                        last_error = Some(e.to_string());
                        continue;
                    }
                    return Err(ProviderError::ClaudeFailed(format!(
                        "Request failed: {}",
                        e
                    )));
                }
            }
        }

        Err(ProviderError::ClaudeFailed(
            last_error.unwrap_or_else(|| "Unknown error".into()),
        ))
    }
}

fn parse_response(body: &str) -> Result<CodexReviewOutput, ProviderError> {
    let response: MessagesResponse = serde_json::from_str(body)
        .map_err(|e| ProviderError::ClaudeFailed(format!("Parse response error: {}", e)))?;

    // Find the first tool_use block
    for block in &response.content {
        if let ContentBlock::ToolUse { input, .. } = block {
            let output: CodexReviewOutput = serde_json::from_value(input.clone()).map_err(|e| {
                ProviderError::ClaudeFailed(format!("Parse tool_use output error: {}", e))
            })?;
            return Ok(output);
        }
    }

    Err(ProviderError::ClaudeFailed(
        "No tool_use block in response".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::credentials::{self, ProviderCredentialField};

    #[test]
    fn test_parse_response_with_tool_use() {
        let body = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Here are my findings:"},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "submit_review",
                    "input": {
                        "findings": [
                            {
                                "title": "SQL injection risk",
                                "body": "User input not sanitized",
                                "file_path": "src/db.rs",
                                "line_start": 42,
                                "line_end": 45,
                                "severity": "blocker",
                                "confidence": 0.95,
                                "agent_type": "security"
                            }
                        ],
                        "overall_assessment": "Critical security issue found",
                        "overall_confidence": 0.9
                    }
                }
            ],
            "stop_reason": "tool_use"
        }"#;

        let result = parse_response(body).unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].title, "SQL injection risk");
        assert_eq!(result.findings[0].severity, "blocker");
        assert_eq!(result.findings[0].confidence, 0.95);
    }

    #[test]
    fn test_parse_response_no_tool_use() {
        let body = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "No issues found."}
            ],
            "stop_reason": "end_turn"
        }"#;

        let result = parse_response(body);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_multiple_tool_use_takes_first() {
        let body = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "submit_review",
                    "input": {
                        "findings": [
                            {"title": "First", "body": "body", "severity": "warning", "confidence": 0.8, "agent_type": "security"}
                        ]
                    }
                },
                {
                    "type": "tool_use",
                    "id": "toolu_2",
                    "name": "submit_review",
                    "input": {
                        "findings": [
                            {"title": "Second", "body": "body", "severity": "info", "confidence": 0.5, "agent_type": "architecture"}
                        ]
                    }
                }
            ],
            "stop_reason": "tool_use"
        }"#;

        let result = parse_response(body).unwrap();
        assert_eq!(result.findings[0].title, "First");
    }

    #[tokio::test]
    async fn test_claude_health_check_no_key() {
        // Temporarily unset the key for testing
        let provider = ClaudeProvider {
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
            client: reqwest::Client::new(),
        };
        let health = provider.health_check().await;
        assert!(!health.available);
        assert!(health.message.unwrap().contains("ANTHROPIC_API_KEY"));
    }

    #[tokio::test]
    async fn test_claude_health_check_with_key() {
        let provider = ClaudeProvider {
            model: DEFAULT_MODEL.to_string(),
            api_key: Some("test-key".into()),
            client: reqwest::Client::new(),
        };
        let health = provider.health_check().await;
        assert!(health.available);
    }

    #[test]
    fn test_claude_provider_reads_keychain_when_env_absent() {
        let prior = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        credentials::delete_secret(ProviderCredentialField::AnthropicApiKey).unwrap();
        credentials::store_secret(ProviderCredentialField::AnthropicApiKey, "keychain-key")
            .unwrap();

        let provider = ClaudeProvider::new();
        assert_eq!(provider.api_key.as_deref(), Some("keychain-key"));

        let _ = credentials::delete_secret(ProviderCredentialField::AnthropicApiKey);
        if let Some(value) = prior {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }
    }

    #[test]
    fn test_messages_request_serializes_tool_choice() {
        let request = MessagesRequest {
            model: "claude-sonnet-4-5-20250929".into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".into(),
                content: "test".into(),
            }],
            tools: vec![Tool {
                name: "submit_review".into(),
                description: "desc".into(),
                input_schema: serde_json::json!({}),
            }],
            tool_choice: Some(ToolChoice {
                choice_type: "tool".into(),
                name: Some("submit_review".into()),
            }),
        };
        let json = serde_json::to_value(&request).unwrap();
        let tc = json
            .get("tool_choice")
            .expect("tool_choice must be present");
        assert_eq!(tc.get("type").unwrap(), "tool");
        assert_eq!(tc.get("name").unwrap(), "submit_review");
    }

    #[tokio::test]
    async fn test_claude_run_review_no_key_errors() {
        let provider = ClaudeProvider {
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
            client: reqwest::Client::new(),
        };
        let input = ReviewInput {
            lane_id: "test".into(),
            system_prompt: "test".into(),
            diff: "diff".into(),
            output_schema: "{}".into(),
        };
        let result = provider
            .run_review(&input, Path::new("/tmp"), CancellationToken::new())
            .await;
        assert!(result.is_err());
    }
}
