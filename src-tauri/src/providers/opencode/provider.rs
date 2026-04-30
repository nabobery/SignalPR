use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::errors::ProviderError;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

use super::manager::OpenCodeManager;

const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4-5";
const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct OpenCodeProvider {
    manager: Arc<OpenCodeManager>,
    model: String,
}

impl OpenCodeProvider {
    pub fn new(manager: Arc<OpenCodeManager>, model: Option<String>) -> Self {
        Self {
            manager,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }
}

#[async_trait]
impl ReviewProvider for OpenCodeProvider {
    fn provider_name(&self) -> &str {
        "opencode"
    }

    async fn health_check(&self) -> ProviderHealth {
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some("opencode-sdk".into()),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("OpenCode not available: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        _cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        // 1. Ensure the server is started
        self.manager.ensure_started().await?;

        // 2. Create a lightweight session (just title)
        let session_id = self.manager.create_session(&input.lane_id).await?;

        // 3. Send message with structured output format (synchronous — blocks until done).
        //    SSE deltas are still forwarded to the frontend by lib.rs in parallel.
        let send_fut = self.manager.send_message(
            &session_id,
            &input.system_prompt,
            &input.diff,
            &input.output_schema,
            &self.model,
        );

        let response = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = self.manager.abort_session(&session_id).await;
                self.manager.unregister_session(&session_id).await;
                return Err(ProviderError::Cancelled);
            }
            result = tokio::time::timeout(REVIEW_TIMEOUT, send_fut) => {
                match result {
                    Ok(Ok(resp)) => resp,
                    Ok(Err(e)) => {
                        let _ = self.manager.destroy_session(&session_id).await;
                        return Err(e);
                    }
                    Err(_) => {
                        let _ = self.manager.abort_session(&session_id).await;
                        self.manager.unregister_session(&session_id).await;
                        return Err(ProviderError::OpenCodeFailed(
                            "Review timeout (300s)".into(),
                        ));
                    }
                }
            }
        };

        // 4. Extract structured output from response
        let structured = response
            .get("info")
            .and_then(|i| i.get("structured"))
            .or_else(|| response.get("structured_output"))
            .cloned();

        let _ = self.manager.destroy_session(&session_id).await;

        match structured {
            Some(data) => {
                debug!("Parsing structured output from OpenCode response");
                let mut output = serde_json::from_value::<CodexReviewOutput>(data.clone())
                    .map_err(|e| {
                        warn!("Failed to parse structured output: {} — raw: {}", e, data);
                        ProviderError::OpenCodeFailed(format!(
                            "Failed to parse structured output: {}",
                            e
                        ))
                    })?;
                output.provider_session_id = Some(session_id);
                Ok(output)
            }
            None => Err(ProviderError::OpenCodeFailed(
                "OpenCode response missing structured output".into(),
            )),
        }
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
    fn test_provider_name_returns_opencode() {
        let manager = Arc::new(OpenCodeManager::new());
        let provider = OpenCodeProvider::new(manager, None);
        assert_eq!(provider.provider_name(), "opencode");
    }

    #[test]
    fn test_provider_uses_custom_model() {
        let manager = Arc::new(OpenCodeManager::new());
        let provider = OpenCodeProvider::new(manager, Some("openai/gpt-4.1".into()));
        assert_eq!(provider.model, "openai/gpt-4.1");
    }

    #[test]
    fn test_provider_uses_default_model() {
        let manager = Arc::new(OpenCodeManager::new());
        let provider = OpenCodeProvider::new(manager, None);
        assert_eq!(provider.model, DEFAULT_MODEL);
    }

    #[test]
    fn test_parse_structured_output_response() {
        let response = json!({
            "info": {
                "structured": {
                    "findings": [{
                        "title": "SQL Injection Risk",
                        "body": "User input passed directly to query",
                        "file_path": "src/db.rs",
                        "line_start": 42,
                        "line_end": 45,
                        "severity": "critical",
                        "confidence": 0.95,
                        "agent_type": "security"
                    }],
                    "overall_assessment": "Found 1 security issue",
                    "overall_confidence": 0.9
                }
            }
        });

        let structured = response
            .get("info")
            .unwrap()
            .get("structured")
            .unwrap()
            .clone();
        let output: CodexReviewOutput = serde_json::from_value(structured).unwrap();
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].title, "SQL Injection Risk");
        assert_eq!(output.findings[0].severity, "critical");
        assert_eq!(
            output.overall_assessment.as_deref(),
            Some("Found 1 security issue")
        );
    }

    #[test]
    fn test_parse_structured_output_fallback_field() {
        let response = json!({
            "structured_output": {
                "findings": [],
                "overall_assessment": "No issues",
                "overall_confidence": 1.0
            }
        });

        let structured = response
            .get("info")
            .and_then(|i| i.get("structured"))
            .or_else(|| response.get("structured_output"))
            .unwrap()
            .clone();
        let output: CodexReviewOutput = serde_json::from_value(structured).unwrap();
        assert_eq!(output.findings.len(), 0);
    }

    #[tokio::test]
    async fn test_health_check_no_server() {
        let manager = Arc::new(OpenCodeManager::new());
        let provider = OpenCodeProvider::new(manager, None);
        let health = provider.health_check().await;
        let _ = health.available;
    }
}
