use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

use super::manager::CopilotManager;

const DEFAULT_MODEL: &str = "gpt-4.1";
const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct CopilotProvider {
    manager: Arc<CopilotManager>,
    model: String,
}

impl CopilotProvider {
    pub fn new(manager: Arc<CopilotManager>, model: Option<String>) -> Self {
        Self {
            manager,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }
}

#[async_trait]
impl ReviewProvider for CopilotProvider {
    fn provider_name(&self) -> &str {
        "copilot"
    }

    async fn health_check(&self) -> ProviderHealth {
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some("copilot-sdk".into()),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("Copilot not available: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        // 1. Ensure the server is started
        self.manager.ensure_started().await?;

        // 2. Create a session with the submit_review tool
        let session_id = self
            .manager
            .create_session(
                &input.lane_id,
                &input.system_prompt,
                &input.output_schema,
                cwd,
                &self.model,
            )
            .await?;

        // 3. Subscribe to events BEFORE sending the message
        let mut event_rx = self.manager.subscribe_events();

        // 4. Send the diff as the user message
        self.manager.send_message(&session_id, &input.diff).await?;

        // 5. Event loop: wait for tool call or session.idle
        let mut result: Option<CodexReviewOutput> = None;

        let review_result = tokio::time::timeout(REVIEW_TIMEOUT, async {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("Copilot review cancelled for lane {}", input.lane_id);
                        let _ = self.manager.abort_session(&session_id).await;
                        self.manager.unregister_session(&session_id).await;
                        return Err(ProviderError::Cancelled);
                    }
                    event = event_rx.recv() => {
                        let event = match event {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                return Err(ProviderError::CopilotFailed(
                                    "Event channel closed".into(),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Copilot event receiver lagged by {} messages", n);
                                continue;
                            }
                        };

                        // Filter events for this session
                        if event.session_id != session_id {
                            continue;
                        }

                        match event.event_type.as_str() {
                            "assistant.message_delta" => {
                                // Streaming deltas are forwarded to the frontend by lib.rs
                                // via the event broadcast; we don't need to buffer here.
                            }
                            "external_tool.requested" => {
                                let tool_name = event.event
                                    .get("toolName")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();

                                if tool_name == "submit_review" {
                                    let arguments = event.event
                                        .get("arguments")
                                        .cloned()
                                        .unwrap_or(Value::Null);

                                    match serde_json::from_value::<CodexReviewOutput>(arguments.clone()) {
                                        Ok(output) => {
                                            debug!(
                                                "Parsed submit_review tool call: {} findings",
                                                output.findings.len()
                                            );
                                            result = Some(output);

                                            // Respond to the tool call
                                            let _ = self.manager.respond_to_tool_call(
                                                &session_id,
                                                &event.event_id,
                                                json!({"success": true}),
                                            ).await;
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to parse submit_review arguments: {} — raw: {}",
                                                e, arguments
                                            );
                                        }
                                    }
                                }
                            }
                            "session.idle" => {
                                debug!("Copilot session {} idle", session_id);
                                break;
                            }
                            "session.error" => {
                                let error_msg = event.event
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown session error");
                                return Err(ProviderError::CopilotFailed(
                                    format!("Session error: {}", error_msg),
                                ));
                            }
                            // permission.requested is handled by manager → lib.rs → frontend
                            _ => {}
                        }
                    }
                }
            }
            Ok(())
        })
        .await;

        // Handle timeout
        match review_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = self.manager.destroy_session(&session_id).await;
                return Err(e);
            }
            Err(_timeout) => {
                let _ = self.manager.abort_session(&session_id).await;
                self.manager.unregister_session(&session_id).await;
                return Err(ProviderError::CopilotFailed("Review timeout (300s)".into()));
            }
        }

        // 6. Clean up session
        let _ = self.manager.destroy_session(&session_id).await;

        // 7. Return the parsed result
        result.ok_or_else(|| {
            ProviderError::CopilotFailed(
                "Copilot session completed without calling submit_review tool".into(),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_provider_name_returns_copilot() {
        let manager = Arc::new(CopilotManager::new());
        let provider = CopilotProvider::new(manager, None);
        assert_eq!(provider.provider_name(), "copilot");
    }

    #[test]
    fn test_provider_uses_custom_model() {
        let manager = Arc::new(CopilotManager::new());
        let provider = CopilotProvider::new(manager, Some("claude-sonnet-4-5".into()));
        assert_eq!(provider.model, "claude-sonnet-4-5");
    }

    #[test]
    fn test_provider_uses_default_model() {
        let manager = Arc::new(CopilotManager::new());
        let provider = CopilotProvider::new(manager, None);
        assert_eq!(provider.model, DEFAULT_MODEL);
    }

    #[test]
    fn test_parse_submit_review_tool_call() {
        let arguments = json!({
            "findings": [
                {
                    "title": "SQL Injection Risk",
                    "body": "User input passed directly to query",
                    "file_path": "src/db.rs",
                    "line_start": 42,
                    "line_end": 45,
                    "severity": "critical",
                    "confidence": 0.95,
                    "agent_type": "security"
                }
            ],
            "overall_assessment": "Found 1 security issue",
            "overall_confidence": 0.9
        });

        let output: CodexReviewOutput = serde_json::from_value(arguments).unwrap();
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].title, "SQL Injection Risk");
        assert_eq!(output.findings[0].severity, "critical");
        assert_eq!(
            output.overall_assessment.as_deref(),
            Some("Found 1 security issue")
        );
    }

    #[tokio::test]
    async fn test_health_check_no_cli() {
        let manager = Arc::new(CopilotManager::new());
        let provider = CopilotProvider::new(manager, None);
        let health = provider.health_check().await;
        // Just verify it doesn't panic
        let _ = health.available;
    }
}
