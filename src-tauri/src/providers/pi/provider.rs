use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::errors::ProviderError;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

use super::manager::PiManager;

const REVIEW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct PiProvider {
    manager: Arc<PiManager>,
    model: Option<String>,
}

impl PiProvider {
    pub fn new(manager: Arc<PiManager>, model: Option<String>) -> Self {
        Self { manager, model }
    }

    /// Build the full prompt: system prompt + output-schema instruction + diff.
    /// PI does not support structured-output tools, so we instruct the agent to
    /// respond with a single JSON object and parse it from the accumulated text.
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
            ProviderError::PiFailed(format!(
                "Failed to parse review output as JSON: {} — raw text: {}",
                e,
                truncate_for_log(raw)
            ))
        })
    }
}

#[async_trait]
impl ReviewProvider for PiProvider {
    fn provider_name(&self) -> &str {
        "pi"
    }

    async fn health_check(&self) -> ProviderHealth {
        if !PiManager::has_pi_binary() {
            return ProviderHealth {
                available: false,
                version: None,
                message: Some(
                    "PI CLI not found. Install with `npm i -g @mariozechner/pi-coding-agent`."
                        .into(),
                ),
            };
        }
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some("pi-rpc".to_string()),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("PI not available: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        _cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        self.manager.ensure_started().await?;

        // Serialize the entire session lifecycle. PI is single-session —
        // new_session + prompt + wait agent_end must be atomic across lanes.
        let _session_guard = self.manager.acquire_session_guard().await;

        self.manager.new_session(&input.lane_id).await?;

        let prompt = Self::build_prompt(input);

        info!(
            "Starting PI review for lane {} (model {:?})",
            input.lane_id,
            self.model.as_deref().unwrap_or("default")
        );

        let prompt_fut = self.manager.prompt(&input.lane_id, &prompt);
        tokio::pin!(prompt_fut);

        let prompt_result = tokio::time::timeout(REVIEW_TIMEOUT, async {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("PI review cancelled for lane {}", input.lane_id);
                    let _ = self.manager.abort().await;
                    Err(ProviderError::Cancelled)
                }
                res = &mut prompt_fut => res,
            }
        })
        .await;

        let raw_text = match prompt_result {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => return Err(e),
            Err(_timeout) => {
                let _ = self.manager.abort().await;
                return Err(ProviderError::PiFailed("Review timeout (300s)".into()));
            }
        };

        if raw_text.trim().is_empty() {
            return Err(ProviderError::PiFailed("PI returned empty response".into()));
        }

        let output = Self::parse_output(&raw_text)?;
        debug!(
            "PI review for lane {} produced {} findings",
            input.lane_id,
            output.findings.len()
        );
        Ok(output)
    }
}

/// Strip ` ```json ... ``` ` or ` ``` ... ``` ` wrappers if present.
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

/// Locate the outermost JSON object (`{...}`) in a string, tolerating any
/// prose before or after it.
fn locate_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

/// UTF-8-safe log truncation.
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
        let manager = Arc::new(PiManager::new());
        let provider = PiProvider::new(manager, None);
        assert_eq!(provider.provider_name(), "pi");
    }

    #[test]
    fn test_build_prompt_includes_sections() {
        let input = ReviewInput {
            lane_id: "security".into(),
            system_prompt: "You are a security reviewer".into(),
            diff: "diff --git a/x b/x".into(),
            output_schema: "{\"type\": \"object\"}".into(),
        };
        let prompt = PiProvider::build_prompt(&input);
        assert!(prompt.contains("You are a security reviewer"));
        assert!(prompt.contains("{\"type\": \"object\"}"));
        assert!(prompt.contains("diff --git a/x b/x"));
        assert!(prompt.contains("Output format"));
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

        let output = PiProvider::parse_output(&raw).unwrap();
        assert_eq!(output.findings.len(), 1);
        assert_eq!(output.findings[0].title, "SQL injection");
        assert_eq!(output.findings[0].severity, "critical");
    }

    #[test]
    fn test_parse_output_with_markdown_fences() {
        let raw = "```json\n{\"findings\": [], \"overall_assessment\": \"clean\", \"overall_confidence\": 0.9}\n```";
        let output = PiProvider::parse_output(raw).unwrap();
        assert_eq!(output.findings.len(), 0);
        assert_eq!(output.overall_assessment.as_deref(), Some("clean"));
    }

    #[test]
    fn test_parse_output_with_leading_prose() {
        let raw = "Sure, here is my review:\n{\"findings\": [], \"overall_assessment\": null, \"overall_confidence\": null}";
        let output = PiProvider::parse_output(raw).unwrap();
        assert_eq!(output.findings.len(), 0);
    }

    #[test]
    fn test_parse_output_invalid_returns_error() {
        let raw = "not json at all";
        let result = PiProvider::parse_output(raw);
        assert!(result.is_err());
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
    fn test_truncate_for_log_short_string_unchanged() {
        assert_eq!(truncate_for_log("short"), "short");
    }

    #[test]
    fn test_truncate_for_log_cjk_boundary_no_panic() {
        let s = "日".repeat(200);
        let out = truncate_for_log(&s);
        assert!(out.ends_with("...(truncated)"));
        assert!(out.chars().count() > 0);
    }
}
