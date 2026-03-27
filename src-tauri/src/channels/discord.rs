use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::manager::ChannelManager;
use super::ws_manager::{next_backoff, WsConfig};
use super::{ChannelEvent, ChannelStatus};

/// Discord Gateway opcodes
const OP_DISPATCH: u64 = 0;
const OP_HEARTBEAT: u64 = 1;
const OP_IDENTIFY: u64 = 2;
const OP_RECONNECT: u64 = 7;
const OP_INVALID_SESSION: u64 = 9;
const OP_HELLO: u64 = 10;
const OP_HEARTBEAT_ACK: u64 = 11;

/// Discord Gateway intents: GUILD_MESSAGES (1 << 9) | DIRECT_MESSAGES (1 << 12)
/// plus MESSAGE_CONTENT (1 << 15) for PR URL extraction.
const GATEWAY_INTENTS: u64 = 4608 | 32768;

/// Build the identify payload for Discord Gateway.
fn build_identify_payload(bot_token: &str) -> serde_json::Value {
    serde_json::json!({
        "op": OP_IDENTIFY,
        "d": {
            "token": bot_token,
            "intents": GATEWAY_INTENTS,
            "properties": {
                "os": "macos",
                "browser": "signalpr",
                "device": "signalpr"
            }
        }
    })
}

/// Build a heartbeat payload with the last known sequence number.
fn build_heartbeat_payload(sequence: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "op": OP_HEARTBEAT,
        "d": sequence
    })
}

/// Start a Discord Gateway listener that receives PR review mentions over WebSocket.
///
/// `bot_token` is the Discord bot token (without the "Bot " prefix).
#[allow(dead_code)]
pub async fn start_discord_listener(
    bot_token: String,
    manager: Arc<ChannelManager>,
    cancel: CancellationToken,
) {
    let config = WsConfig::default();
    let mut backoff = config.initial_backoff;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // 1. Fetch gateway URL
        let gateway_url = tokio::select! {
            _ = cancel.cancelled() => break,
            result = fetch_gateway_url(&bot_token) => {
                match result {
                    Ok(url) => url,
                    Err(e) => {
                        tracing::warn!("Failed to fetch Discord gateway URL: {}", e);
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = next_backoff(backoff, &config);
                        continue;
                    }
                }
            }
        };

        // Append gateway version and encoding params
        let ws_url = format!("{}/?v=10&encoding=json", gateway_url);

        // 2. Connect
        let ws_stream = tokio::select! {
            _ = cancel.cancelled() => break,
            result = connect_async(&ws_url) => {
                match result {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        tracing::warn!("Discord WebSocket connect failed: {}", e);
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = next_backoff(backoff, &config);
                        continue;
                    }
                }
            }
        };

        backoff = config.initial_backoff;
        tracing::debug!("Discord WebSocket connected");

        // 3. Run gateway session
        let session_result = run_gateway_session(ws_stream, &bot_token, &manager, &cancel).await;

        manager.update_status(ChannelStatus {
            source: "discord".into(),
            connected: false,
            message: Some("Disconnected".into()),
        });

        if cancel.is_cancelled() {
            break;
        }

        // If the session ended due to invalid session, wait a bit longer
        if let Err(ref reason) = session_result {
            tracing::debug!("Discord session ended: {}", reason);
        }

        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = next_backoff(backoff, &config);
    }

    manager.update_status(ChannelStatus {
        source: "discord".into(),
        connected: false,
        message: Some("Disconnected".into()),
    });
}

/// Fetch the Gateway WebSocket URL from Discord API.
async fn fetch_gateway_url(bot_token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", format!("Bot {}", bot_token))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    body.get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            format!("Discord API error: {}", msg)
        })
}

/// Run a single Discord Gateway session (connect -> identify -> dispatch loop).
async fn run_gateway_session(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    bot_token: &str,
    manager: &Arc<ChannelManager>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let (mut write, mut read) = ws_stream.split();
    let mut sequence: Option<u64> = None;
    let mut heartbeat_interval: Option<Duration> = None;
    let mut identified = false;

    // We track heartbeat timing manually
    let mut last_heartbeat = tokio::time::Instant::now();
    let mut got_ack = true;
    let mut bot_user_id: Option<String> = None;

    loop {
        // Calculate time until next heartbeat
        let hb_sleep = if let Some(interval) = heartbeat_interval {
            let elapsed = last_heartbeat.elapsed();
            if elapsed >= interval {
                Duration::from_millis(0)
            } else {
                interval - elapsed
            }
        } else {
            // No heartbeat interval yet -- wait a long time
            Duration::from_secs(300)
        };

        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = write.send(Message::Close(None)).await;
                return Ok(());
            }
            _ = tokio::time::sleep(hb_sleep) => {
                // Time to send heartbeat
                if heartbeat_interval.is_some() {
                    if !got_ack {
                        tracing::warn!("Discord: no heartbeat ACK received, reconnecting");
                        return Err("heartbeat timeout".into());
                    }
                    let hb = build_heartbeat_payload(sequence);
                    if let Err(e) = write.send(Message::Text(hb.to_string().into())).await {
                        return Err(format!("Failed to send heartbeat: {}", e));
                    }
                    got_ack = false;
                    last_heartbeat = tokio::time::Instant::now();
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let parsed: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!("Failed to parse Discord message: {}", e);
                                continue;
                            }
                        };

                        // Update sequence number
                        if let Some(s) = parsed.get("s").and_then(|v| v.as_u64()) {
                            sequence = Some(s);
                        }

                        let op = parsed.get("op").and_then(|v| v.as_u64()).unwrap_or(999);

                        match op {
                            OP_HELLO => {
                                // Extract heartbeat_interval from d.heartbeat_interval
                                if let Some(interval_ms) = parsed
                                    .get("d")
                                    .and_then(|d| d.get("heartbeat_interval"))
                                    .and_then(|v| v.as_u64())
                                {
                                    heartbeat_interval = Some(Duration::from_millis(interval_ms));
                                    last_heartbeat = tokio::time::Instant::now();
                                    got_ack = true;
                                    tracing::debug!("Discord heartbeat interval: {}ms", interval_ms);
                                }

                                // Send Identify
                                if !identified {
                                    let identify = build_identify_payload(bot_token);
                                    if let Err(e) = write.send(Message::Text(identify.to_string().into())).await {
                                        return Err(format!("Failed to send identify: {}", e));
                                    }
                                    identified = true;
                                    tracing::debug!("Discord identify sent");
                                }
                            }
                            OP_HEARTBEAT_ACK => {
                                got_ack = true;
                            }
                            OP_HEARTBEAT => {
                                // Server requested immediate heartbeat
                                let hb = build_heartbeat_payload(sequence);
                                if let Err(e) = write.send(Message::Text(hb.to_string().into())).await {
                                    return Err(format!("Failed to send heartbeat: {}", e));
                                }
                                last_heartbeat = tokio::time::Instant::now();
                            }
                            OP_DISPATCH => {
                                let event_name = parsed.get("t").and_then(|v| v.as_str()).unwrap_or("");

                                if event_name == "READY" {
                                    bot_user_id = parsed
                                        .get("d")
                                        .and_then(|d| d.get("user"))
                                        .and_then(|u| u.get("id"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    manager.update_status(ChannelStatus {
                                        source: "discord".into(),
                                        connected: true,
                                        message: Some("Connected via Gateway".into()),
                                    });
                                    tracing::debug!("Discord READY received");
                                } else if event_name == "MESSAGE_CREATE" {
                                    if let Some(d) = parsed.get("d") {
                                        process_discord_message(d, manager, bot_user_id.as_deref());
                                    }
                                }
                            }
                            OP_RECONNECT => {
                                tracing::debug!("Discord requested reconnect");
                                return Err("reconnect requested".into());
                            }
                            OP_INVALID_SESSION => {
                                tracing::warn!("Discord invalid session");
                                // Wait before reconnecting as Discord recommends
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                return Err("invalid session".into());
                            }
                            _ => {
                                tracing::debug!("Discord unknown opcode: {}", op);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::debug!("Discord WebSocket closed");
                        return Err("connection closed".into());
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        return Err(format!("WebSocket error: {}", e));
                    }
                    None => {
                        return Err("stream ended".into());
                    }
                }
            }
        }
    }
}

/// Process a Discord MESSAGE_CREATE event, extracting PR URLs and emitting events.
fn process_discord_message(
    data: &serde_json::Value,
    manager: &Arc<ChannelManager>,
    bot_user_id: Option<&str>,
) {
    // Check if this is a DM (no guild_id) or if the bot was mentioned.
    let is_dm = data.get("guild_id").is_none_or(|v| v.is_null());
    let bot_mentioned = bot_user_id.is_some_and(|bot_id| {
        data.get("mentions")
            .and_then(|v| v.as_array())
            .is_some_and(|mentions| {
                mentions
                    .iter()
                    .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(bot_id))
            })
    });

    if !is_dm && !bot_mentioned {
        return;
    }

    let content = match data.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return,
    };

    let pr_urls = extract_pr_urls(content);
    if pr_urls.is_empty() {
        return;
    }

    let requester = data
        .get("author")
        .and_then(|a| a.get("username"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let channel = data
        .get("channel_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    for url in pr_urls {
        let now = chrono::Utc::now().to_rfc3339();
        manager.emit(ChannelEvent {
            source: "discord".into(),
            pr_url: url,
            requester: requester.clone(),
            channel: channel.clone(),
            received_at: now,
        });
    }
}

/// Extract GitHub PR URLs from text (same logic as Slack).
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
    }

    #[test]
    fn test_extract_no_urls() {
        let text = "No links here.";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_discord_listener_cancellation() {
        let manager = Arc::new(ChannelManager::new());
        let cancel = CancellationToken::new();

        let m = manager.clone();
        let c = cancel.clone();
        let handle = tokio::spawn(async move {
            start_discord_listener("fake-token".into(), m, c).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        cancel.cancel();
        handle.await.unwrap();

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].connected);
    }

    #[test]
    fn test_parse_discord_message_create() {
        let message = serde_json::json!({
            "content": "Check out https://github.com/owner/repo/pull/42 please",
            "author": {
                "username": "testuser",
                "id": "123456"
            },
            "channel_id": "789012",
            "guild_id": null
        });

        let manager = Arc::new(ChannelManager::new());
        let mut rx = manager.subscribe();

        process_discord_message(&message, &manager, None);

        let event = rx.try_recv().expect("should receive event");
        assert_eq!(event.source, "discord");
        assert_eq!(event.pr_url, "https://github.com/owner/repo/pull/42");
        assert_eq!(event.requester, Some("testuser".into()));
        assert_eq!(event.channel, Some("789012".into()));
    }

    #[test]
    fn test_parse_discord_message_create_guild_not_mentioned() {
        // Guild message where bot is NOT mentioned -- should be ignored
        let message = serde_json::json!({
            "content": "Check out https://github.com/owner/repo/pull/42",
            "author": { "username": "testuser", "id": "123" },
            "channel_id": "789",
            "guild_id": "999",
            "mentions": []
        });

        let manager = Arc::new(ChannelManager::new());
        let mut rx = manager.subscribe();

        process_discord_message(&message, &manager, Some("bot-id"));

        assert!(
            rx.try_recv().is_err(),
            "should not emit event for non-mentioned guild message"
        );
    }

    #[test]
    fn test_discord_identify_payload() {
        let payload = build_identify_payload("my-bot-token");
        assert_eq!(payload["op"], OP_IDENTIFY);
        assert_eq!(payload["d"]["token"], "my-bot-token");
        assert_eq!(payload["d"]["intents"], GATEWAY_INTENTS);
        assert_eq!(payload["d"]["properties"]["os"], "macos");
        assert_eq!(payload["d"]["properties"]["browser"], "signalpr");
        assert_eq!(payload["d"]["properties"]["device"], "signalpr");
    }

    #[test]
    fn test_parse_discord_message_create_guild_mentions_other_user_is_ignored() {
        let message = serde_json::json!({
            "content": "Please review https://github.com/owner/repo/pull/42",
            "author": { "username": "testuser", "id": "123" },
            "channel_id": "789",
            "guild_id": "999",
            "mentions": [{ "id": "someone-else" }]
        });

        let manager = Arc::new(ChannelManager::new());
        let mut rx = manager.subscribe();

        process_discord_message(&message, &manager, Some("bot-id"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_parse_discord_message_create_guild_bot_mentioned_is_processed() {
        let message = serde_json::json!({
            "content": "Please review https://github.com/owner/repo/pull/42",
            "author": { "username": "testuser", "id": "123" },
            "channel_id": "789",
            "guild_id": "999",
            "mentions": [{ "id": "bot-id" }]
        });

        let manager = Arc::new(ChannelManager::new());
        let mut rx = manager.subscribe();

        process_discord_message(&message, &manager, Some("bot-id"));
        let event = rx.try_recv().expect("should receive event");
        assert_eq!(event.source, "discord");
        assert_eq!(event.pr_url, "https://github.com/owner/repo/pull/42");
    }

    #[test]
    fn test_discord_heartbeat_payload() {
        // With sequence
        let hb = build_heartbeat_payload(Some(42));
        assert_eq!(hb["op"], OP_HEARTBEAT);
        assert_eq!(hb["d"], 42);

        // Without sequence (initial)
        let hb_null = build_heartbeat_payload(None);
        assert_eq!(hb_null["op"], OP_HEARTBEAT);
        assert!(hb_null["d"].is_null());
    }
}
