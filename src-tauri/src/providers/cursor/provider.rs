use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

use super::manager::CursorManager;

/// Default Cursor model ID. `auto` lets the Cursor CLI route to whichever
/// model it thinks best fits — we pick this over pinning a specific model
/// so users on different Cursor plans don't hit entitlement errors.
const DEFAULT_MODEL: &str = "auto";

/// Model IDs surfaced in the settings UI dropdown. Not enforced at the
/// Rust layer (the `model` field is a free `String`) — this is just the
/// authoritative display set.
#[allow(dead_code)]
pub const VALID_CURSOR_MODELS: &[&str] = &[
    "auto",
    "gpt-5.2",
    "sonnet-4.5-thinking",
    "sonnet-4.5",
    "opus-4.6",
];

const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct CursorProvider {
    manager: Arc<CursorManager>,
    model: String,
}

impl CursorProvider {
    pub fn new(manager: Arc<CursorManager>, model: Option<String>) -> Self {
        Self {
            manager,
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    /// Build the full ACP prompt: system prompt + output-schema instruction + diff.
    fn build_prompt(input: &ReviewInput) -> String {
        format!(
            "{system}\n\n\
             ## Output format\n\
             You MUST respond with a single JSON object matching this schema. \
             Do not include any prose before or after the JSON. Do not wrap it \
             in markdown code fences. Output only the JSON object:\n\n\
             {schema}\n\n\
             ## Diff to review\n\
             {diff}",
            system = input.system_prompt,
            schema = input.output_schema,
            diff = input.diff
        )
    }

    /// Extract a `CodexReviewOutput` from the accumulated agent message text.
    /// Tolerates markdown code fences and leading/trailing prose by locating
    /// the outermost JSON object.
    fn parse_output(raw: &str) -> Result<CodexReviewOutput, ProviderError> {
        let trimmed = strip_code_fences(raw.trim());
        let json_slice = locate_json_object(trimmed).unwrap_or(trimmed);
        serde_json::from_str::<CodexReviewOutput>(json_slice).map_err(|e| {
            ProviderError::CursorFailed(format!(
                "Failed to parse review output as JSON: {} — raw text: {}",
                e,
                truncate_for_log(raw)
            ))
        })
    }
}

#[async_trait]
impl ReviewProvider for CursorProvider {
    fn provider_name(&self) -> &str {
        "cursor"
    }

    async fn health_check(&self) -> ProviderHealth {
        if !CursorManager::has_auth_env() {
            return ProviderHealth {
                available: false,
                version: None,
                message: Some(
                    "CURSOR_API_KEY not set. Generate a key from the Cursor Dashboard \
                     (Cloud Agents → User API Keys) and export it before launching SignalPR."
                        .into(),
                ),
            };
        }
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some(format!("cursor-acp/{}", self.model)),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("Cursor not available: {}", e)),
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

        // Belt-and-braces: always attempt `session/set_mode("ask")`. The
        // manager's set_session_mode tolerates "method not found" from
        // builds that don't expose the unstable call, and the spawn-time
        // `--mode ask` flag is the primary enforcement path anyway. We
        // log (not warn) the non-advertised case because the parent
        // plan's mode-parsing change means `available_modes` may be
        // empty for either shape reason.
        if !handle.available_modes.iter().any(|m| m == "ask") {
            debug!(
                "Cursor session {} did not advertise ask mode explicitly; \
                 calling set_mode anyway as belt-and-braces",
                session_id
            );
        }
        if let Err(e) = self.manager.set_session_mode(&session_id, "ask").await {
            warn!(
                "Failed to enable ask mode for Cursor session {}: {} — \
                 review will still run under deny-by-default permissions \
                 and the spawn-time --mode flag",
                session_id, e
            );
        }

        if let Err(e) = self
            .manager
            .set_session_model(&session_id, &self.model)
            .await
        {
            warn!(
                "Failed to set Cursor session {} model to {}: {} — \
                 falling back to server default model",
                session_id, self.model, e
            );
        }

        let prompt = Self::build_prompt(input);

        info!(
            "Starting Cursor review for lane {} (session {}, model {})",
            input.lane_id, session_id, self.model
        );

        let prompt_fut = self.manager.prompt(&session_id, &prompt);
        tokio::pin!(prompt_fut);

        let prompt_result = tokio::time::timeout(REVIEW_TIMEOUT, async {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Cursor review cancelled for lane {}", input.lane_id);
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
                return Err(ProviderError::CursorFailed("Review timeout (300s)".into()));
            }
        };

        self.manager.unregister_session(&session_id).await;

        if raw_text.trim().is_empty() {
            return Err(ProviderError::CursorFailed(
                "Cursor returned empty response".into(),
            ));
        }

        let mut output = Self::parse_output(&raw_text)?;
        output.provider_session_id = Some(session_id.clone());
        debug!(
            "Cursor review for lane {} produced {} findings",
            input.lane_id,
            output.findings.len()
        );
        Ok(output)
    }
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

fn locate_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

fn truncate_for_log(s: &str) -> String {
    const MAX: usize = 500;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut cut = MAX;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...(truncated)", &s[..cut])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_provider_name() {
        let manager = Arc::new(CursorManager::new());
        let provider = CursorProvider::new(manager, None);
        assert_eq!(provider.provider_name(), "cursor");
    }

    #[test]
    fn test_default_model() {
        let manager = Arc::new(CursorManager::new());
        let provider = CursorProvider::new(manager, None);
        assert_eq!(provider.model, DEFAULT_MODEL);
        assert_eq!(DEFAULT_MODEL, "auto");
        assert!(VALID_CURSOR_MODELS.contains(&DEFAULT_MODEL));
    }

    #[test]
    fn test_custom_model() {
        let manager = Arc::new(CursorManager::new());
        let provider = CursorProvider::new(manager, Some("gpt-5.2".into()));
        assert_eq!(provider.model, "gpt-5.2");
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

        let output = CursorProvider::parse_output(&raw).unwrap();
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].title, "SQL injection");
    }

    #[test]
    fn test_parse_output_with_markdown_fences() {
        let raw = "```json\n{\"findings\": [], \"overall_assessment\": \"clean\", \"overall_confidence\": 0.9}\n```";
        let output = CursorProvider::parse_output(raw).unwrap();
        assert_eq!(output.findings.len(), 0);
        assert_eq!(output.overall_assessment.as_deref(), Some("clean"));
    }

    #[test]
    fn test_parse_output_invalid_returns_error() {
        let raw = "not json at all";
        let result = CursorProvider::parse_output(raw);
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
        let prompt = CursorProvider::build_prompt(&input);
        assert!(prompt.contains("You are a security reviewer"));
        assert!(prompt.contains("{\"type\": \"object\"}"));
        assert!(prompt.contains("diff --git a/x b/x"));
        assert!(prompt.contains("Output format"));
    }

    #[tokio::test]
    async fn test_health_check_without_api_key() {
        let prior = std::env::var("CURSOR_API_KEY").ok();
        std::env::remove_var("CURSOR_API_KEY");

        let manager = Arc::new(CursorManager::new());
        let provider = CursorProvider::new(manager, None);
        let health = provider.health_check().await;
        assert!(!health.available);
        assert!(health
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("CURSOR_API_KEY"));

        if let Some(v) = prior {
            std::env::set_var("CURSOR_API_KEY", v);
        }
    }

    #[test]
    fn test_truncate_for_log_cjk_boundary_no_panic() {
        let s = "日".repeat(200);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("...(truncated)"));
    }
}
