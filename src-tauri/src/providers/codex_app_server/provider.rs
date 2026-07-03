use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;

use super::manager::CodexAppServerManager;
use crate::providers::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};

/// Default timeout for a single review turn.
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(300);

/// Maximum buffer size for streamed text (16 KB).
const MAX_STREAM_BUFFER: usize = 16 * 1024;

fn push_capped(buf: &mut String, delta: &str) {
    if delta.is_empty() {
        return;
    }

    buf.push_str(delta);
    if buf.len() > MAX_STREAM_BUFFER {
        let overflow = buf.len() - MAX_STREAM_BUFFER;
        buf.drain(..overflow);
    }
}

fn extract_authoritative_text_from_item(item: &serde_json::Value) -> Option<String> {
    match item.get("type").and_then(|v| v.as_str()) {
        Some("agentMessage") => item
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        Some("exitedReviewMode") => item
            .get("review")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

/// A `ReviewProvider` backed by a persistent `codex app-server` process.
///
/// Unlike the one-shot `CodexProvider` (which uses `codex exec`), this provider
/// maintains a long-running child process and communicates via JSON-RPC over
/// stdio. This enables streaming, multi-turn conversations, and interactive
/// approval flows.
pub struct CodexAppServerProvider {
    manager: Arc<CodexAppServerManager>,
    /// None means "use the user's configured default model" — a hardcoded
    /// API-only model id is rejected by ChatGPT-subscription accounts.
    model: Option<String>,
}

#[allow(dead_code)]
impl CodexAppServerProvider {
    /// Create a new provider backed by the shared app-server manager.
    ///
    /// This does NOT start the child process; `health_check` or `run_review` trigger lazy startup.
    pub fn new(manager: Arc<CodexAppServerManager>) -> Self {
        Self {
            manager,
            model: None,
        }
    }

    pub fn with_model(manager: Arc<CodexAppServerManager>, model: String) -> Self {
        Self {
            manager,
            model: Some(model),
        }
    }

    /// Get a reference to the underlying manager for direct thread/turn control.
    pub fn manager(&self) -> &Arc<CodexAppServerManager> {
        &self.manager
    }
}

#[async_trait]
impl ReviewProvider for CodexAppServerProvider {
    fn provider_name(&self) -> &str {
        "codex-app-server"
    }

    async fn health_check(&self) -> ProviderHealth {
        match self.manager.ensure_started().await {
            Ok(()) => ProviderHealth {
                available: true,
                version: Some("app-server".into()),
                message: None,
            },
            Err(e) => ProviderHealth {
                available: false,
                version: None,
                message: Some(format!("Failed to start codex app-server: {}", e)),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }

        // Ensure the server is running
        self.manager.ensure_started().await?;

        // Start an ephemeral thread for this lane
        let thread_id = self
            .manager
            .start_thread(cwd, self.model.as_deref())
            .await?;
        info!("Started app-server thread {} for review", thread_id);
        self.manager
            .register_thread_lane(thread_id.clone(), input.lane_id.clone())
            .await;

        // Build input as the Codex app-server expects: array of content items
        let prompt = format!("{}\n\nPR Diff:\n{}", input.system_prompt, input.diff);
        let user_input = vec![json!({
            "type": "text",
            "text": prompt,
        })];

        // Parse the output schema
        let output_schema = if input.output_schema.is_empty() {
            None
        } else {
            serde_json::from_str(&input.output_schema).ok()
        };

        // Start the turn
        let turn_id = self
            .manager
            .start_turn(&thread_id, user_input, output_schema)
            .await?;

        // Collect streaming notifications until turn/completed or timeout
        let mut stream_buffer = String::new();
        let mut final_text: Option<String> = None;
        let timeout = tokio::time::sleep(DEFAULT_TURN_TIMEOUT);
        tokio::pin!(timeout);

        let mut rx = self.manager.subscribe_notifications();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    // Try to interrupt the turn before returning
                    let _ = self.manager.interrupt_turn(&thread_id, &turn_id).await;
                    return Err(ProviderError::Cancelled);
                }
                _ = &mut timeout => {
                    warn!("Turn timed out on thread {} (turn {})", thread_id, turn_id);
                    let _ = self.manager.interrupt_turn(&thread_id, &turn_id).await;
                    return Err(ProviderError::CodexFailed("Turn timed out".into()));
                }
                notif = rx.recv() => {
                    let n = match notif {
                        Ok(n) => n,
                        Err(broadcast::error::RecvError::Closed) => {
                            return Err(ProviderError::CodexFailed(
                                "Notification channel closed unexpectedly".into(),
                            ));
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    };

                    match n.method.as_str() {
                        "turn/completed" => {
                            // Docs: params.turn.id (no threadId), so match by turn id.
                            let matches_turn = n
                                .params
                                .as_ref()
                                .and_then(|p| p.get("turn"))
                                .and_then(|t| t.get("id"))
                                .and_then(|v| v.as_str())
                                .is_some_and(|tid| tid == turn_id);

                            if matches_turn {
                                debug!("Turn completed on thread {}", thread_id);
                                break;
                            }
                        }
                        "item/agentMessage/delta" => {
                            // Filter to our ephemeral thread (item events include threadId).
                            let matches_thread = n
                                .params
                                .as_ref()
                                .and_then(|p| p.get("threadId"))
                                .and_then(|v| v.as_str())
                                .is_some_and(|tid| tid == thread_id);
                            if !matches_thread {
                                continue;
                            }
                            if let Some(delta) = n.params
                                .as_ref()
                                .and_then(|p| p.get("delta"))
                                .and_then(|d| d.as_str())
                            {
                                push_capped(&mut stream_buffer, delta);
                            }
                        }
                        "item/completed" => {
                            let matches_thread = n
                                .params
                                .as_ref()
                                .and_then(|p| p.get("threadId"))
                                .and_then(|v| v.as_str())
                                .is_some_and(|tid| tid == thread_id);
                            if !matches_thread {
                                continue;
                            }
                            if let Some(item) = n.params
                                .as_ref()
                                .and_then(|p| p.get("item"))
                            {
                                if let Some(text) = extract_authoritative_text_from_item(item) {
                                    final_text = Some(text);
                                }
                            }
                        }
                        _ => {
                            debug!("Notification: {}", n.method);
                        }
                    }
                }
            }
        }

        // Parse the collected text as CodexReviewOutput JSON
        let text = final_text.as_deref().unwrap_or(&stream_buffer);
        if text.trim().is_empty() {
            return Err(ProviderError::CodexFailed(
                "Empty response from codex app-server".into(),
            ));
        }

        // Try to extract JSON from the text buffer (may be embedded in markdown)
        let json_text = extract_json_from_text(text);
        let output: CodexReviewOutput = serde_json::from_str(&json_text).map_err(|e| {
            ProviderError::CodexFailed(format!(
                "Failed to parse app-server response as CodexReviewOutput: {}. First 500 chars: {}",
                e,
                &text[..text.len().min(500)]
            ))
        })?;

        self.manager.unregister_thread(&thread_id).await;
        Ok(output)
    }
}

/// Try to extract a JSON object from text that may contain markdown code fences.
fn extract_json_from_text(text: &str) -> String {
    let trimmed = text.trim();

    // If it already starts with {, use as-is
    if trimmed.starts_with('{') {
        return trimmed.to_string();
    }

    // Look for JSON in code fences: ```json\n{...}\n```
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_plain_json() {
        let input = r#"{"findings": []}"#;
        assert_eq!(extract_json_from_text(input), r#"{"findings": []}"#);
    }

    #[test]
    fn test_extract_json_from_code_fence() {
        let input = "```json\n{\"findings\": []}\n```";
        assert_eq!(extract_json_from_text(input), r#"{"findings": []}"#);
    }

    #[test]
    fn test_extract_json_from_text_with_prefix() {
        let input = "Here is the review:\n{\"findings\": []}";
        assert_eq!(extract_json_from_text(input), r#"{"findings": []}"#);
    }

    #[test]
    fn test_provider_creation() {
        let manager = Arc::new(CodexAppServerManager::new());
        let provider = CodexAppServerProvider::new(manager);
        assert_eq!(provider.provider_name(), "codex-app-server");
    }

    #[test]
    fn test_extract_authoritative_text_from_agent_message_item() {
        let item = json!({
            "type": "agentMessage",
            "id": "item_1",
            "text": "{\"findings\": []}"
        });
        assert_eq!(
            extract_authoritative_text_from_item(&item),
            Some("{\"findings\": []}".into())
        );
    }

    #[test]
    fn test_extract_authoritative_text_from_exited_review_mode_item() {
        let item = json!({
            "type": "exitedReviewMode",
            "id": "item_1",
            "review": "Looks good."
        });
        assert_eq!(
            extract_authoritative_text_from_item(&item),
            Some("Looks good.".into())
        );
    }

    #[test]
    fn test_push_capped_trims_prefix_on_overflow() {
        let mut buf = String::new();
        push_capped(&mut buf, &"a".repeat(MAX_STREAM_BUFFER));
        assert_eq!(buf.len(), MAX_STREAM_BUFFER);

        push_capped(&mut buf, "b");
        assert_eq!(buf.len(), MAX_STREAM_BUFFER);
        assert!(buf.ends_with('b'));
    }
}
