use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::errors::ProviderError;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};
use crate::secrets::credentials::{self, ProviderCredentialField};

use super::manager::ClaudeCodeManager;

pub struct ClaudeCodeProvider {
    manager: Arc<ClaudeCodeManager>,
    sidecar_path: String,
    app_data_dir: std::path::PathBuf,
}

impl ClaudeCodeProvider {
    fn has_api_key() -> bool {
        credentials::resolve_credential(ProviderCredentialField::AnthropicApiKey)
            .ok()
            .and_then(|(value, _)| value)
            .is_some()
    }

    pub fn new(
        manager: Arc<ClaudeCodeManager>,
        sidecar_path: String,
        app_data_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            manager,
            sidecar_path,
            app_data_dir,
        }
    }

    fn parse_output(raw: &serde_json::Value) -> Result<CodexReviewOutput, ProviderError> {
        serde_json::from_value::<CodexReviewOutput>(raw.clone()).map_err(|e| {
            ProviderError::ClaudeCodeFailed(format!(
                "Failed to parse structured output: {} — raw: {}",
                e,
                serde_json::to_string(raw).unwrap_or_default()
            ))
        })
    }
}

#[async_trait]
impl ReviewProvider for ClaudeCodeProvider {
    fn provider_name(&self) -> &str {
        "claude_code"
    }

    async fn health_check(&self) -> ProviderHealth {
        if let Err(error) =
            ClaudeCodeManager::validate_sidecar_binary(Path::new(&self.sidecar_path))
        {
            return ProviderHealth {
                available: false,
                version: None,
                message: Some(error),
            };
        }

        let has_api_key = Self::has_api_key();
        match ClaudeCodeManager::check_health(&self.sidecar_path, &self.app_data_dir, !has_api_key)
        {
            Ok(info) => ProviderHealth {
                available: has_api_key,
                version: Some(format!(
                    "bridge={} sdk={}",
                    info.bridge_version, info.sdk_version
                )),
                message: if has_api_key {
                    None
                } else {
                    Some("ANTHROPIC_API_KEY not set".into())
                },
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("Health check failed: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        let cwd_str = cwd.to_string_lossy().to_string();

        let manager = self.manager.clone();
        let sidecar = self.sidecar_path.clone();
        let app_data = self.app_data_dir.clone();
        let lane_id = input.lane_id.clone();
        let lane_id_for_cancel = lane_id.clone();
        let system_prompt = input.system_prompt.clone();
        let diff = input.diff.clone();
        let output_schema = input.output_schema.clone();

        let cancel_manager = manager.clone();

        let review_handle = tokio::spawn(async move {
            manager
                .run_review(
                    &lane_id,
                    &system_prompt,
                    &diff,
                    &output_schema,
                    &cwd_str,
                    &app_data,
                    &sidecar,
                    false,
                )
                .await
        });

        tokio::select! {
            result = review_handle => {
                let review = result
                    .map_err(|e| ProviderError::ClaudeCodeFailed(format!("Task join error: {}", e)))??;
                let mut output = Self::parse_output(&review.output)?;
                output.provider_session_id = review.session_id;
                output.cost_usd = review.cost_usd;
                output.checkpoint_metadata_json = review
                    .checkpoint_id
                    .map(|checkpoint_id| serde_json::json!({ "checkpoint_id": checkpoint_id }).to_string());
                Ok(output)
            }
            _ = cancel.cancelled() => {
                cancel_manager.cancel_lane(&lane_id_for_cancel).await;
                Err(ProviderError::Cancelled)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudeCodeProvider;
    use crate::providers::traits::CodexReviewOutput;
    use serde_json::json;

    #[test]
    fn parse_output_accepts_provider_contract_shape() {
        let raw = json!({
            "findings": [
                {
                    "title": "Suspicious shell invocation",
                    "body": "The command interpolates untrusted input into a shell string.",
                    "file_path": "src/main.rs",
                    "line_start": 12,
                    "line_end": 12,
                    "severity": "warning",
                    "confidence": 0.88,
                    "evidence": ["format!(\"sh -c {}\", user_input)"],
                    "agent_type": "security"
                }
            ],
            "overall_assessment": "Needs follow-up",
            "overall_confidence": 0.71
        });

        let output = ClaudeCodeProvider::parse_output(&raw).expect("provider output should parse");
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].file_path.as_deref(), Some("src/main.rs"));
        assert_eq!(output.findings[0].line_start, Some(12));
        assert_eq!(output.findings[0].agent_type, "security");
    }

    #[test]
    fn parse_output_rejects_mock_only_shape() {
        let raw = json!({
            "findings": [
                {
                    "file": "src/main.rs",
                    "line": 42,
                    "severity": "warning",
                    "title": "Unused variable binding",
                    "description": "Variable `result` is assigned but never used in this scope.",
                    "suggestion": "Prefix with underscore: `_result`"
                }
            ],
            "summary": "Mock review completed"
        });

        let error =
            ClaudeCodeProvider::parse_output(&raw).expect_err("legacy mock shape must fail");
        let message = error.to_string();
        assert!(message.contains("Failed to parse structured output"));
    }

    #[test]
    fn parse_output_keeps_rust_contract_stable() {
        let raw = json!({
            "findings": [],
            "overall_assessment": null,
            "overall_confidence": null
        });

        let output: CodexReviewOutput =
            ClaudeCodeProvider::parse_output(&raw).expect("empty valid payload should parse");
        assert!(output.findings.is_empty());
        assert!(output.overall_assessment.is_none());
        assert!(output.overall_confidence.is_none());
    }
}
