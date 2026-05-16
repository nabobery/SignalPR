use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;
use crate::providers::acp::shared::{build_review_prompt, parse_review_output};
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

use super::manager::GeminiManager;

/// Default Gemini model. We ship a stable ID rather than a `*-preview` one
/// because preview-tier access is gated per Google account and we don't want
/// health checks to silently fall back. Users can override via the settings
/// dropdown. Full list sourced from upstream
/// `packages/core/src/config/models.ts` `VALID_GEMINI_MODELS` (verified
/// 2026-04-12).
const DEFAULT_MODEL: &str = "gemini-2.5-pro";

/// Full list of model IDs we accept in the settings UI dropdown. This is not
/// enforced at the Rust layer (the `model` field is a free `String` on the
/// provider) — it's just the authoritative set for display + tests.
#[allow(dead_code)]
pub const VALID_GEMINI_MODELS: &[&str] = &[
    // Stable tier
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    // Preview tier (access may vary by account)
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-3.1-pro-preview",
    "gemini-3.1-flash-lite-preview",
    // Aliases resolved by the CLI at runtime
    "auto",
    "pro",
    "flash",
    "flash-lite",
];

const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct GeminiProvider {
    manager: Arc<GeminiManager>,
    model: String,
}

impl GeminiProvider {
    pub fn new(manager: Arc<GeminiManager>, model: Option<String>) -> Self {
        Self {
            manager,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    /// Build the full ACP prompt: system prompt + output-schema instruction + diff.
    /// ACP does not (yet) support host-exposed structured-output tools the way
    /// Claude's Messages API does, so we instruct the agent to respond with a
    /// single JSON object and parse that from the accumulated agent message text.
    fn build_prompt(input: &ReviewInput) -> String {
        build_review_prompt(&input.system_prompt, &input.output_schema, &input.diff)
    }

    /// Extract a `CodexReviewOutput` from the accumulated agent message text.
    /// Tolerates markdown code fences and leading/trailing prose by locating
    /// the outermost JSON object.
    fn parse_output(raw: &str) -> Result<CodexReviewOutput, ProviderError> {
        parse_review_output(raw, "gemini")
    }
}

#[async_trait]
impl ReviewProvider for GeminiProvider {
    fn provider_name(&self) -> &str {
        "gemini"
    }

    async fn health_check(&self) -> ProviderHealth {
        if !GeminiManager::has_auth_env() {
            return ProviderHealth {
                available: false,
                version: None,
                message: Some(
                    "GEMINI_API_KEY not set. SignalPR requires an AI Studio API key or \
                     Vertex AI credentials. OAuth is not supported for third-party harnesses."
                        .into(),
                ),
            };
        }
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some(format!("gemini-acp/{}", self.model)),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("Gemini not available: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        self.manager.ensure_started().await?;

        let handle = self.manager.create_session(&input.lane_id, cwd).await?;
        let session_id = handle.session_id.clone();

        // Prefer ACP "plan" mode when it is available. Upstream only lists
        // `plan` when `config.isPlanEnabled()` is true. This complements our
        // deny-by-default permission handler because plan mode blocks file
        // writes and shell execution at the agent policy layer too.
        if handle.available_modes.iter().any(|m| m == "plan") {
            if let Err(e) = self.manager.set_session_mode(&session_id, "plan").await {
                warn!(
                    "Failed to enable plan mode for Gemini session {}: {} — \
                     review will still run under deny-by-default permissions",
                    session_id, e
                );
            }
        } else {
            debug!(
                "Gemini session {} does not advertise plan mode; \
                 relying on deny-by-default permission handler",
                session_id
            );
        }

        // Send the selected model to the session. Tolerates `method not found`
        // as non-fatal (some CLI builds disable the
        // unstable set_model call).
        if let Err(e) = self
            .manager
            .set_session_model(&session_id, &self.model)
            .await
        {
            warn!(
                "Failed to set Gemini session {} model to {}: {} — \
                 falling back to server default model",
                session_id, self.model, e
            );
        }

        let prompt = Self::build_prompt(input);

        info!(
            "Starting Gemini review for lane {} (session {}, model {})",
            input.lane_id, session_id, self.model
        );

        let prompt_fut = self.manager.prompt(&session_id, &prompt);
        tokio::pin!(prompt_fut);

        let prompt_result = tokio::time::timeout(REVIEW_TIMEOUT, async {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Gemini review cancelled for lane {}", input.lane_id);
                    let _ = self.manager.cancel_session(&session_id).await;
                    Err(ProviderError::Cancelled)
                }
                res = &mut prompt_fut => res,
            }
        })
        .await;

        let raw_text = match prompt_result {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                self.manager.unregister_session(&session_id).await;
                return Err(e);
            }
            Err(_timeout) => {
                let _ = self.manager.cancel_session(&session_id).await;
                self.manager.unregister_session(&session_id).await;
                return Err(ProviderError::GeminiFailed("Review timeout (300s)".into()));
            }
        };

        self.manager.unregister_session(&session_id).await;

        if raw_text.trim().is_empty() {
            return Err(ProviderError::GeminiFailed(
                "Gemini returned empty response".into(),
            ));
        }

        let mut output = Self::parse_output(&raw_text)?;
        output.provider_session_id = Some(session_id.clone());
        debug!(
            "Gemini review for lane {} produced {} findings",
            input.lane_id,
            output.findings.len()
        );
        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::acp::shared::{locate_json_object, strip_code_fences, truncate_for_log};
    use serde_json::json;

    #[test]
    fn test_provider_name() {
        let manager = Arc::new(GeminiManager::new());
        let provider = GeminiProvider::new(manager, None);
        assert_eq!(provider.provider_name(), "gemini");
    }

    #[test]
    fn test_default_model_is_stable_tier() {
        let manager = Arc::new(GeminiManager::new());
        let provider = GeminiProvider::new(manager, None);
        assert_eq!(provider.model, DEFAULT_MODEL);
        // Must be a stable (non-preview) model so users don't hit preview
        // access gating during health checks.
        assert_eq!(DEFAULT_MODEL, "gemini-2.5-pro");
        assert!(!DEFAULT_MODEL.contains("preview"));
        assert!(VALID_GEMINI_MODELS.contains(&DEFAULT_MODEL));
    }

    #[test]
    fn test_valid_gemini_models_contains_upstream_ids() {
        // Sanity check against upstream `VALID_GEMINI_MODELS` set
        // (packages/core/src/config/models.ts, verified 2026-04-12).
        for expected in &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-3-pro-preview",
            "gemini-3-flash-preview",
        ] {
            assert!(
                VALID_GEMINI_MODELS.contains(expected),
                "expected {} in VALID_GEMINI_MODELS",
                expected
            );
        }
        // gemini-3-flash (without -preview) is NOT a valid upstream ID
        // and must not appear here. This catches the original default bug.
        assert!(
            !VALID_GEMINI_MODELS.contains(&"gemini-3-flash"),
            "gemini-3-flash is not a real upstream model ID"
        );
    }

    #[test]
    fn test_truncate_for_log_short_string_unchanged() {
        assert_eq!(truncate_for_log("short"), "short");
    }

    #[test]
    fn test_truncate_for_log_ascii_boundary() {
        let s = "a".repeat(1000);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("...(truncated)"));
        assert!(out.len() < 1000);
    }

    #[test]
    fn test_truncate_for_log_cjk_boundary_no_panic() {
        // Each CJK char is 3 bytes in UTF-8; 200 chars = 600 bytes, which
        // means byte 500 lands mid-codepoint. Byte-slicing would panic.
        let s = "日".repeat(200);
        let out = truncate_for_log(&s);
        // Must not panic, must be valid UTF-8, must end with marker.
        assert!(out.ends_with("...(truncated)"));
        assert!(out.chars().count() > 0);
    }

    #[test]
    fn test_truncate_for_log_emoji_boundary_no_panic() {
        // 4-byte codepoints that could land mid-sequence at byte 500.
        let s = "😀".repeat(200);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("...(truncated)"));
    }

    #[test]
    fn test_custom_model() {
        let manager = Arc::new(GeminiManager::new());
        let provider = GeminiProvider::new(manager, Some("gemini-3-pro".into()));
        assert_eq!(provider.model, "gemini-3-pro");
    }

    #[test]
    fn test_strip_code_fences_json() {
        let input = "```json\n{\"a\": 1}\n```";
        assert_eq!(strip_code_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn test_strip_code_fences_plain() {
        let input = "```\n{\"a\": 1}\n```";
        assert_eq!(strip_code_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn test_strip_code_fences_no_fences() {
        let input = "{\"a\": 1}";
        assert_eq!(strip_code_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn test_locate_json_object_with_prose() {
        let input = "Here is your answer: {\"findings\": []} hope that helps";
        assert_eq!(locate_json_object(input), Some("{\"findings\": []}"));
    }

    #[test]
    fn test_parse_output_valid_json() {
        let raw = json!({
            "findings": [{
                "title": "SQL injection",
                "body": "User input passed directly to query",
                "file_path": "src/db.rs",
                "line_start": 42,
                "line_end": 45,
                "severity": "critical",
                "confidence": 0.95,
                "agent_type": "security"
            }],
            "overall_assessment": "One critical issue",
            "overall_confidence": 0.9
        })
        .to_string();

        let output = GeminiProvider::parse_output(&raw).unwrap();
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].title, "SQL injection");
        assert_eq!(output.findings[0].severity, "critical");
    }

    #[test]
    fn test_parse_output_with_markdown_fences() {
        let raw = "```json\n{\"findings\": [], \"overall_assessment\": \"clean\", \"overall_confidence\": 0.9}\n```";
        let output = GeminiProvider::parse_output(raw).unwrap();
        assert_eq!(output.findings.len(), 0);
        assert_eq!(output.overall_assessment.as_deref(), Some("clean"));
    }

    #[test]
    fn test_parse_output_with_leading_prose() {
        let raw = "Sure, here is my review:\n{\"findings\": [], \"overall_assessment\": null, \"overall_confidence\": null}";
        let output = GeminiProvider::parse_output(raw).unwrap();
        assert_eq!(output.findings.len(), 0);
    }

    #[test]
    fn test_parse_output_invalid_returns_error() {
        let raw = "not json at all";
        let result = GeminiProvider::parse_output(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_prompt_includes_sections() {
        let input = ReviewInput {
            lane_id: "security".into(),
            system_prompt: "You are a security reviewer".into(),
            diff: "diff --git a/x b/x".into(),
            output_schema: "{\"type\": \"object\"}".into(),
        };
        let prompt = GeminiProvider::build_prompt(&input);
        assert!(prompt.contains("You are a security reviewer"));
        assert!(prompt.contains("{\"type\": \"object\"}"));
        assert!(prompt.contains("diff --git a/x b/x"));
        assert!(prompt.contains("Output format"));
    }

    #[tokio::test]
    async fn test_health_check_without_api_key() {
        // Snapshot env to avoid cross-test contamination.
        let had_gemini = std::env::var("GEMINI_API_KEY").ok();
        let had_google = std::env::var("GOOGLE_API_KEY").ok();
        let had_gac = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");

        let manager = Arc::new(GeminiManager::new());
        let provider = GeminiProvider::new(manager, None);
        let health = provider.health_check().await;
        assert!(!health.available);
        assert!(health
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("GEMINI_API_KEY"));

        // Restore env
        if let Some(v) = had_gemini {
            std::env::set_var("GEMINI_API_KEY", v);
        }
        if let Some(v) = had_google {
            std::env::set_var("GOOGLE_API_KEY", v);
        }
        if let Some(v) = had_gac {
            std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", v);
        }
    }
}
