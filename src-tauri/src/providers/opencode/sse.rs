use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// A parsed Server-Sent Event from the OpenCode SSE endpoint.
/// After parsing, the bus envelope `{type, properties}` is unwrapped so that
/// `event_type` is the inner type and `data` is the inner `properties` object.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SseEvent {
    pub event_type: String,
    pub data: Value,
    pub id: Option<String>,
}

/// Parse raw SSE text lines into structured events.
///
/// SSE protocol: lines prefixed with `event:`, `data:`, `id:`, or `:` (comment).
/// A blank line terminates and emits the current event.
///
/// OpenCode events arrive as bus envelopes: `data: {"type":"...", "properties":{...}}`.
/// This function unwraps the envelope so callers get the inner type and properties directly.
pub fn parse_sse_events(text: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current_event_type = String::new();
    let mut data_lines: Vec<String> = Vec::new();
    let mut current_id: Option<String> = None;

    for line in text.lines() {
        if line.is_empty() {
            if !data_lines.is_empty() {
                let raw_data = data_lines.join("\n");
                let parsed =
                    serde_json::from_str::<Value>(&raw_data).unwrap_or(Value::String(raw_data));

                // Unwrap bus envelope: { type, properties }
                let (final_type, final_data) =
                    if let Some(bus_type) = parsed.get("type").and_then(|v| v.as_str()) {
                        let properties = parsed
                            .get("properties")
                            .cloned()
                            .unwrap_or(Value::Object(Default::default()));
                        (bus_type.to_string(), properties)
                    } else {
                        let et = if current_event_type.is_empty() {
                            "message".to_string()
                        } else {
                            current_event_type.clone()
                        };
                        (et, parsed)
                    };

                events.push(SseEvent {
                    event_type: final_type,
                    data: final_data,
                    id: current_id.clone(),
                });
            }
            current_event_type.clear();
            data_lines.clear();
            current_id = None;
        } else if let Some(rest) = line.strip_prefix("event:") {
            current_event_type = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start_matches(' ').to_string());
        } else if let Some(rest) = line.strip_prefix("id:") {
            current_id = Some(rest.trim().to_string());
        } else if line.starts_with(':') {
            // Comment line, ignore
        }
    }

    events
}

fn auth_username() -> String {
    std::env::var("OPENCODE_SERVER_USERNAME").unwrap_or_else(|_| "opencode".to_string())
}

/// Connect to the OpenCode SSE endpoint and yield events via an mpsc channel.
/// Uses true incremental streaming via `bytes_stream()`.
/// Reconnects automatically on disconnect (with backoff, up to 10 retries).
pub async fn sse_listener(
    base_url: String,
    auth_password: Option<String>,
    event_tx: mpsc::Sender<SseEvent>,
    cancel: CancellationToken,
) {
    let client = reqwest::Client::new();
    let mut retries = 0u32;
    const MAX_RETRIES: u32 = 10;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let url = format!("{}/global/event", base_url);
        let mut request = client.get(&url).header("Accept", "text/event-stream");
        if let Some(ref pw) = auth_password {
            request = request.basic_auth(auth_username(), Some(pw));
        }

        let response = match request.send().await {
            Ok(resp) if resp.status().is_success() => {
                retries = 0;
                resp
            }
            Ok(resp) => {
                warn!("SSE endpoint returned {}", resp.status());
                retries += 1;
                if retries > MAX_RETRIES {
                    warn!("SSE listener exceeded max retries, stopping");
                    break;
                }
                backoff_sleep(retries, &cancel).await;
                continue;
            }
            Err(e) => {
                warn!("SSE connection failed: {}", e);
                retries += 1;
                if retries > MAX_RETRIES {
                    warn!("SSE listener exceeded max retries, stopping");
                    break;
                }
                backoff_sleep(retries, &cancel).await;
                continue;
            }
        };

        // True incremental streaming via bytes_stream()
        let mut stream = response.bytes_stream();
        let mut line_buf = String::new();
        let mut event_buf = String::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            line_buf.push_str(&String::from_utf8_lossy(&bytes));
                            // Process complete lines
                            while let Some(pos) = line_buf.find('\n') {
                                let line = line_buf[..pos].to_string();
                                line_buf = line_buf[pos + 1..].to_string();

                                if line.is_empty() || line == "\r" {
                                    // Blank line = event delimiter
                                    if !event_buf.is_empty() {
                                        for event in parse_sse_events(
                                            &format!("{}\n\n", event_buf),
                                        ) {
                                            if event_tx.send(event).await.is_err() {
                                                return;
                                            }
                                        }
                                        event_buf.clear();
                                    }
                                } else {
                                    event_buf.push_str(&line);
                                    event_buf.push('\n');
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!("SSE stream error: {}", e);
                            break; // Will reconnect
                        }
                        None => {
                            warn!("SSE stream ended");
                            break; // Will reconnect
                        }
                    }
                }
            }
        }

        retries += 1;
        if retries > MAX_RETRIES {
            warn!("SSE listener exceeded max retries, stopping");
            break;
        }
        backoff_sleep(retries, &cancel).await;
    }
}

async fn backoff_sleep(retries: u32, cancel: &CancellationToken) {
    let delay = std::time::Duration::from_millis(1000 * 2u64.pow(retries.min(5)));
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = tokio::time::sleep(delay) => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_bus_envelope_unwrap() {
        let input = "data: {\"type\":\"message.part.updated\",\"properties\":{\"delta\":\"hello\",\"sessionID\":\"s1\"}}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message.part.updated");
        assert_eq!(events[0].data["delta"], "hello");
        assert_eq!(events[0].data["sessionID"], "s1");
    }

    #[test]
    fn test_parse_sse_bus_envelope_permission() {
        let input = "data: {\"type\":\"permission.asked\",\"properties\":{\"requestID\":\"perm-1\",\"sessionID\":\"s2\"}}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "permission.asked");
        assert_eq!(events[0].data["requestID"], "perm-1");
        assert_eq!(events[0].data["sessionID"], "s2");
    }

    #[test]
    fn test_parse_sse_bus_envelope_session_status() {
        let input = "data: {\"type\":\"session.status\",\"properties\":{\"sessionID\":\"s3\",\"status\":\"idle\"}}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "session.status");
        assert_eq!(events[0].data["status"], "idle");
        assert_eq!(events[0].data["sessionID"], "s3");
    }

    #[test]
    fn test_parse_sse_non_envelope_fallback() {
        let input = "data: {\"hello\":\"world\"}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
        assert_eq!(events[0].data["hello"], "world");
    }

    #[test]
    fn test_parse_sse_with_explicit_event_field() {
        let input = "event: custom\ndata: {\"key\":\"val\"}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        // No bus type in data, so falls back to SSE event: field
        assert_eq!(events[0].event_type, "custom");
    }

    #[test]
    fn test_parse_sse_with_id() {
        let input = "data: {\"type\":\"session.status\",\"properties\":{\"status\":\"idle\"}}\nid: evt-42\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id.as_deref(), Some("evt-42"));
    }

    #[test]
    fn test_parse_sse_ignores_comments() {
        let input = ": heartbeat\ndata: {\"type\":\"session.status\",\"properties\":{\"status\":\"active\"}}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "session.status");
    }

    #[test]
    fn test_parse_sse_multiple_events() {
        let input = "data: {\"type\":\"a\",\"properties\":{\"n\":1}}\n\ndata: {\"type\":\"b\",\"properties\":{\"n\":2}}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "a");
        assert_eq!(events[1].event_type, "b");
    }

    #[test]
    fn test_parse_sse_empty_data_ignored() {
        let input = "event: keepalive\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_sse_non_json_data() {
        let input = "data: plain text here\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data.as_str(), Some("plain text here"));
        assert_eq!(events[0].event_type, "message");
    }

    #[test]
    fn test_parse_sse_bus_envelope_missing_properties() {
        let input = "data: {\"type\":\"server.connected\"}\n\n";
        let events = parse_sse_events(input);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "server.connected");
        assert!(events[0].data.is_object());
    }
}
