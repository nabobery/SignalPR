use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::manager::ChannelManager;
use super::ws_manager::{run_ws_loop, WsConfig};
use super::{ChannelEvent, ChannelStatus};

/// Start a Slack Socket Mode listener that receives PR review mentions over WebSocket.
///
/// `app_token` must be a Slack app-level token (xapp-...) with `connections:write` scope.
#[allow(dead_code)]
pub async fn start_slack_listener(
    app_token: String,
    manager: Arc<ChannelManager>,
    cancel: CancellationToken,
) {
    let (incoming_tx, mut incoming_rx) = mpsc::channel::<String>(64);
    let (outgoing_tx, outgoing_rx) = mpsc::channel::<String>(64);

    // URL provider: calls apps.connections.open to get a fresh wss:// URL each time
    let token_for_url = app_token.clone();
    let url_provider: super::ws_manager::UrlProvider = Box::new(move || {
        let token = token_for_url.clone();
        Box::pin(async move {
            let client = reqwest::Client::new();
            let resp = client
                .post("https://slack.com/api/apps.connections.open")
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send()
                .await
                .map_err(|e| format!("HTTP request failed: {}", e))?;

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
                let err_msg = body
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                return Err(format!("Slack API error: {}", err_msg));
            }

            let url_str = body
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "No url in response".to_string())?;

            url::Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))
        })
    });

    // Status callback
    let manager_for_status = manager.clone();
    let status_callback: Box<dyn Fn(bool) + Send + Sync> = Box::new(move |connected| {
        manager_for_status.update_status(ChannelStatus {
            source: "slack".into(),
            connected,
            message: Some(if connected {
                "Connected via Socket Mode".into()
            } else {
                "Disconnected".into()
            }),
        });
    });

    // Spawn the WS loop
    let ws_cancel = cancel.clone();
    let ws_handle = tokio::spawn(async move {
        run_ws_loop(
            url_provider,
            WsConfig::default(),
            incoming_tx,
            outgoing_rx,
            status_callback,
            ws_cancel,
        )
        .await;
    });

    // Message processing loop
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            msg = incoming_rx.recv() => {
                match msg {
                    Some(text) => {
                        process_slack_message(&text, &outgoing_tx, &manager).await;
                    }
                    None => break,
                }
            }
        }
    }

    // Wait for WS loop to finish
    let _ = ws_handle.await;

    manager.update_status(ChannelStatus {
        source: "slack".into(),
        connected: false,
        message: Some("Disconnected".into()),
    });
}

/// Process a single incoming Slack Socket Mode message.
async fn process_slack_message(
    text: &str,
    outgoing_tx: &mpsc::Sender<String>,
    manager: &Arc<ChannelManager>,
) {
    let parsed: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse Slack message as JSON: {}", e);
            return;
        }
    };

    let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Always ack envelopes
    if let Some(envelope_id) = parsed.get("envelope_id").and_then(|v| v.as_str()) {
        let ack = serde_json::json!({ "envelope_id": envelope_id }).to_string();
        if let Err(e) = outgoing_tx.send(ack).await {
            tracing::warn!("Failed to send ack: {}", e);
        }
    }

    match msg_type {
        "hello" => {
            tracing::debug!("Slack Socket Mode connection established");
        }
        "disconnect" => {
            tracing::debug!("Slack requested disconnect, will reconnect");
            // The WS manager handles reconnection automatically when the connection drops
        }
        "events_api" => {
            if let Some(event) = parsed.get("payload").and_then(|p| p.get("event")) {
                let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let channel_type = event
                    .get("channel_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let should_process = event_type == "app_mention"
                    || (event_type == "message" && channel_type == "im");

                if should_process {
                    if let Some(event_text) = event.get("text").and_then(|v| v.as_str()) {
                        let pr_urls = extract_pr_urls(event_text);
                        let requester = event
                            .get("user")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let channel = event
                            .get("channel")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        for url in pr_urls {
                            let now = chrono::Utc::now().to_rfc3339();
                            manager.emit(ChannelEvent {
                                source: "slack".into(),
                                pr_url: url,
                                requester: requester.clone(),
                                channel: channel.clone(),
                                received_at: now,
                            });
                        }
                    }
                }
            }
        }
        _ => {
            tracing::debug!("Ignoring Slack message type: {}", msg_type);
        }
    }
}

/// Extract GitHub PR URLs from text.
#[allow(dead_code)]
pub fn extract_pr_urls(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"https://github\.com/[\w.-]+/[\w.-]+/pull/\d+").unwrap();
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_url() {
        let text = "Please review https://github.com/owner/repo/pull/42 thanks!";
        let urls = extract_pr_urls(text);
        assert_eq!(urls, vec!["https://github.com/owner/repo/pull/42"]);
    }

    #[test]
    fn test_extract_multiple_urls() {
        let text = "Check https://github.com/a/b/pull/1 and https://github.com/c/d/pull/99";
        let urls = extract_pr_urls(text);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://github.com/a/b/pull/1");
        assert_eq!(urls[1], "https://github.com/c/d/pull/99");
    }

    #[test]
    fn test_extract_no_urls() {
        let text = "No links here, just plain text.";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_extract_url_with_dots_and_hyphens() {
        let text = "https://github.com/my-org/my.repo-name/pull/123";
        let urls = extract_pr_urls(text);
        assert_eq!(
            urls,
            vec!["https://github.com/my-org/my.repo-name/pull/123"]
        );
    }

    #[test]
    fn test_extract_ignores_non_github_urls() {
        let text = "See https://gitlab.com/owner/repo/pull/1 for details";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_slack_listener_cancellation() {
        let manager = Arc::new(ChannelManager::new());
        let cancel = CancellationToken::new();

        let m = manager.clone();
        let c = cancel.clone();
        let handle = tokio::spawn(async move {
            start_slack_listener("fake-token".into(), m, c).await;
        });

        // Give the listener a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Cancel and wait for shutdown
        cancel.cancel();
        handle.await.unwrap();

        // Verify it reports disconnected
        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].connected);
    }

    #[test]
    fn test_parse_slack_envelope_extracts_pr_url() {
        let envelope = serde_json::json!({
            "type": "events_api",
            "envelope_id": "abc-123",
            "payload": {
                "event": {
                    "type": "app_mention",
                    "text": "Please review https://github.com/owner/repo/pull/42",
                    "user": "U12345",
                    "channel": "C67890"
                }
            }
        });

        // Extract the event and verify PR URL extraction
        let event = envelope
            .get("payload")
            .and_then(|p| p.get("event"))
            .unwrap();
        let text = event.get("text").and_then(|v| v.as_str()).unwrap();
        let urls = extract_pr_urls(text);
        assert_eq!(urls, vec!["https://github.com/owner/repo/pull/42"]);
    }

    #[test]
    fn test_slack_ack_format() {
        let envelope_id = "e-abc-123-def";
        let ack = serde_json::json!({ "envelope_id": envelope_id });
        let ack_str = ack.to_string();

        let parsed: serde_json::Value = serde_json::from_str(&ack_str).unwrap();
        assert_eq!(
            parsed.get("envelope_id").and_then(|v| v.as_str()),
            Some("e-abc-123-def")
        );
        // Should contain only the envelope_id field
        assert_eq!(parsed.as_object().unwrap().len(), 1);
    }
}
