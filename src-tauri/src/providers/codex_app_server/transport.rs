use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

#[allow(dead_code)]
use crate::errors::ProviderError;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// A JSON-RPC error object embedded in an error response.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Outbound message we can serialize onto the wire.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OutboundMessage {
    Request {
        id: Value,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Response {
        id: Value,
        result: Value,
    },
    Notification {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
}

/// Discriminated inbound message parsed from the wire.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum InboundMessage {
    /// Has `id` + `result`
    Response { id: Value, result: Value },
    /// Has `id` + `error`
    ErrorResponse { id: Value, error: JsonRpcError },
    /// Has `id` + `method` (server-initiated request, e.g. approval)
    ServerRequest {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    /// Has `method` but no `id`
    Notification {
        method: String,
        params: Option<Value>,
    },
}

/// Messages forwarded to the approval handler.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ServerRequest {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

/// Messages forwarded to the notification handler.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ServerNotification {
    pub method: String,
    pub params: Option<Value>,
}

// ---------------------------------------------------------------------------
// Parsing helper
// ---------------------------------------------------------------------------

/// Discriminate a raw JSON `Value` into a typed `InboundMessage`.
#[allow(dead_code)]
pub fn parse_inbound(raw: Value) -> Option<InboundMessage> {
    let obj = raw.as_object()?;

    let has_id = obj.contains_key("id");
    let has_method = obj.contains_key("method");
    let has_result = obj.contains_key("result");
    let has_error = obj.contains_key("error");

    if has_id && has_method {
        // Server request (needs a response from us)
        Some(InboundMessage::ServerRequest {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            method: obj
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            params: obj.get("params").cloned(),
        })
    } else if has_id && has_error {
        // Error response
        let error: JsonRpcError =
            serde_json::from_value(obj.get("error").cloned().unwrap_or(Value::Null)).ok()?;
        Some(InboundMessage::ErrorResponse {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            error,
        })
    } else if has_id && has_result {
        // Successful response
        Some(InboundMessage::Response {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            result: obj.get("result").cloned().unwrap_or(Value::Null),
        })
    } else if has_method && !has_id {
        // Notification
        Some(InboundMessage::Notification {
            method: obj
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            params: obj.get("params").cloned(),
        })
    } else {
        warn!(
            "Unrecognized JSON-RPC message shape: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        None
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

#[allow(dead_code)]
type PendingMap = HashMap<String, oneshot::Sender<Result<Value, JsonRpcError>>>;

#[allow(dead_code)]
pub struct JsonRpcTransport {
    /// Channel to send outbound messages to the writer task.
    writer_tx: mpsc::Sender<OutboundMessage>,
    /// Pending request map keyed by stringified request id.
    pending: Arc<Mutex<PendingMap>>,
    /// Monotonic request-id counter.
    next_id: Arc<AtomicU64>,
    /// Token used to signal shutdown to reader/writer tasks.
    cancel: CancellationToken,
}

impl Clone for JsonRpcTransport {
    fn clone(&self) -> Self {
        Self {
            writer_tx: self.writer_tx.clone(),
            pending: Arc::clone(&self.pending),
            next_id: Arc::clone(&self.next_id),
            cancel: self.cancel.clone(),
        }
    }
}

#[allow(dead_code)]
impl JsonRpcTransport {
    /// Create a new transport, spawning reader and writer tasks.
    ///
    /// * `stdin`  - the child-process stdin handle (we write to it).
    /// * `stdout` - the child-process stdout handle (we read from it).
    /// * `cancel` - cancellation token; dropping or cancelling stops the tasks.
    /// * `server_request_tx` - channel for server-initiated requests (approvals).
    /// * `notification_tx`   - channel for server notifications (streaming deltas).
    pub fn new(
        stdin: ChildStdin,
        stdout: tokio::process::ChildStdout,
        cancel: CancellationToken,
        server_request_tx: mpsc::Sender<ServerRequest>,
        notification_tx: mpsc::Sender<ServerNotification>,
    ) -> Self {
        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let (writer_tx, writer_rx) = mpsc::channel::<OutboundMessage>(64);

        // Spawn writer task
        let writer_cancel = cancel.clone();
        tokio::spawn(Self::writer_loop(stdin, writer_rx, writer_cancel));

        // Spawn reader task
        let reader_cancel = cancel.clone();
        let reader_pending = Arc::clone(&pending);
        tokio::spawn(Self::reader_loop(
            stdout,
            reader_pending,
            server_request_tx,
            notification_tx,
            reader_cancel,
        ));

        Self {
            writer_tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            cancel,
        }
    }

    // -- outbound helpers ---------------------------------------------------

    /// Send a JSON-RPC request and wait for the response.
    pub async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ProviderError> {
        let id_num = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id = Value::Number(serde_json::Number::from(id_num));
        let id_key = id.to_string();

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id_key.clone(), tx);
        }

        let msg = OutboundMessage::Request {
            id: id.clone(),
            method: method.to_string(),
            params,
        };

        self.writer_tx
            .send(msg)
            .await
            .map_err(|_| ProviderError::CodexFailed("Transport writer channel closed".into()))?;

        // Wait for the response or cancellation.
        tokio::select! {
            _ = self.cancel.cancelled() => {
                let mut map = self.pending.lock().await;
                map.remove(&id_key);
                Err(ProviderError::Cancelled)
            }
            result = rx => {
                match result {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(rpc_err)) => Err(ProviderError::CodexFailed(
                        format!("JSON-RPC error {}: {}", rpc_err.code, rpc_err.message),
                    )),
                    Err(_) => Err(ProviderError::CodexFailed(
                        "Response channel dropped (transport shut down)".into(),
                    )),
                }
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), ProviderError> {
        let msg = OutboundMessage::Notification {
            method: method.to_string(),
            params,
        };
        self.writer_tx
            .send(msg)
            .await
            .map_err(|_| ProviderError::CodexFailed("Transport writer channel closed".into()))
    }

    /// Send a response to a server-initiated request (e.g. approval decision).
    pub async fn send_response(&self, id: Value, result: Value) -> Result<(), ProviderError> {
        let msg = OutboundMessage::Response { id, result };
        self.writer_tx
            .send(msg)
            .await
            .map_err(|_| ProviderError::CodexFailed("Transport writer channel closed".into()))
    }

    /// Cancel all pending requests and signal tasks to stop.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    // -- internal loops -----------------------------------------------------

    async fn writer_loop(
        mut stdin: ChildStdin,
        mut rx: mpsc::Receiver<OutboundMessage>,
        cancel: CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("JSON-RPC writer shutting down (cancelled)");
                    break;
                }
                msg = rx.recv() => {
                    match msg {
                        Some(m) => {
                            match serde_json::to_string(&m) {
                                Ok(mut line) => {
                                    line.push('\n');
                                    if let Err(e) = stdin.write_all(line.as_bytes()).await {
                                        error!("Failed to write to codex stdin: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        error!("Failed to flush codex stdin: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to serialize outbound message: {}", e);
                                }
                            }
                        }
                        None => {
                            debug!("JSON-RPC writer shutting down (channel closed)");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn reader_loop(
        stdout: tokio::process::ChildStdout,
        pending: Arc<Mutex<PendingMap>>,
        server_request_tx: mpsc::Sender<ServerRequest>,
        notification_tx: mpsc::Sender<ServerNotification>,
        cancel: CancellationToken,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("JSON-RPC reader shutting down (cancelled)");
                    break;
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            let text = text.trim().to_string();
                            if text.is_empty() {
                                continue;
                            }
                            let raw: Value = match serde_json::from_str(&text) {
                                Ok(v) => v,
                                Err(e) => {
                                    warn!("Non-JSON line from codex stdout: {} ({})", text, e);
                                    continue;
                                }
                            };

                            match parse_inbound(raw) {
                                Some(InboundMessage::Response { id, result }) => {
                                    let key = id.to_string();
                                    let mut map = pending.lock().await;
                                    if let Some(tx) = map.remove(&key) {
                                        let _ = tx.send(Ok(result));
                                    } else {
                                        warn!("Received response for unknown id: {}", key);
                                    }
                                }
                                Some(InboundMessage::ErrorResponse { id, error }) => {
                                    let key = id.to_string();
                                    let mut map = pending.lock().await;
                                    if let Some(tx) = map.remove(&key) {
                                        let _ = tx.send(Err(error));
                                    } else {
                                        warn!("Received error response for unknown id: {}", key);
                                    }
                                }
                                Some(InboundMessage::ServerRequest { id, method, params }) => {
                                    let req = ServerRequest { id, method, params };
                                    if server_request_tx.send(req).await.is_err() {
                                        warn!("Server-request channel closed, dropping request");
                                    }
                                }
                                Some(InboundMessage::Notification { method, params }) => {
                                    let notif = ServerNotification { method, params };
                                    if notification_tx.send(notif).await.is_err() {
                                        warn!("Notification channel closed, dropping notification");
                                    }
                                }
                                None => {
                                    // Already warned inside parse_inbound
                                }
                            }
                        }
                        Ok(None) => {
                            debug!("Codex stdout EOF - reader exiting");
                            break;
                        }
                        Err(e) => {
                            error!("Error reading codex stdout: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        // Clean up: fail all pending requests so callers don't hang.
        let mut map = pending.lock().await;
        for (_, tx) in map.drain() {
            let _ = tx.send(Err(JsonRpcError {
                code: -1,
                message: "Transport reader exited".into(),
                data: None,
            }));
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
    fn test_parse_response() {
        let raw = json!({"id": 1, "result": {"ok": true}});
        match parse_inbound(raw) {
            Some(InboundMessage::Response { id, result }) => {
                assert_eq!(id, json!(1));
                assert_eq!(result, json!({"ok": true}));
            }
            other => panic!("Expected Response, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_error_response() {
        let raw = json!({
            "id": 2,
            "error": {"code": -32600, "message": "Invalid Request"}
        });
        match parse_inbound(raw) {
            Some(InboundMessage::ErrorResponse { id, error }) => {
                assert_eq!(id, json!(2));
                assert_eq!(error.code, -32600);
                assert_eq!(error.message, "Invalid Request");
                assert!(error.data.is_none());
            }
            other => panic!("Expected ErrorResponse, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_server_request() {
        let raw = json!({
            "id": "approve-1",
            "method": "codex/approveExec",
            "params": {"command": "rm -rf /"}
        });
        match parse_inbound(raw) {
            Some(InboundMessage::ServerRequest { id, method, params }) => {
                assert_eq!(id, json!("approve-1"));
                assert_eq!(method, "codex/approveExec");
                assert!(params.is_some());
                assert_eq!(params.unwrap()["command"], "rm -rf /");
            }
            other => panic!("Expected ServerRequest, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_notification() {
        let raw = json!({"method": "codex/turnDelta", "params": {"delta": "hello"}});
        match parse_inbound(raw) {
            Some(InboundMessage::Notification { method, params }) => {
                assert_eq!(method, "codex/turnDelta");
                assert!(params.is_some());
            }
            other => panic!("Expected Notification, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_notification_without_params() {
        let raw = json!({"method": "initialized"});
        match parse_inbound(raw) {
            Some(InboundMessage::Notification { method, params }) => {
                assert_eq!(method, "initialized");
                assert!(params.is_none());
            }
            other => panic!("Expected Notification, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_garbage_returns_none() {
        let raw = json!({"foo": "bar"});
        assert!(parse_inbound(raw).is_none());
    }

    #[test]
    fn test_parse_non_object_returns_none() {
        assert!(parse_inbound(json!(42)).is_none());
        assert!(parse_inbound(json!("hello")).is_none());
        assert!(parse_inbound(json!(null)).is_none());
    }

    #[test]
    fn test_outbound_request_serialization() {
        let msg = OutboundMessage::Request {
            id: json!(1),
            method: "initialize".into(),
            params: Some(json!({"capabilities": {}})),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "initialize");
        assert!(v.get("params").is_some());
    }

    #[test]
    fn test_outbound_notification_omits_params_when_none() {
        let msg = OutboundMessage::Notification {
            method: "initialized".into(),
            params: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["method"], "initialized");
        assert!(v.get("params").is_none());
    }

    #[test]
    fn test_outbound_response_serialization() {
        let msg = OutboundMessage::Response {
            id: json!("approve-1"),
            result: json!({"approved": true}),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["id"], "approve-1");
        assert_eq!(v["result"]["approved"], true);
    }

    /// Test round-trip: serialize a request, parse a response, verify IDs match.
    #[tokio::test]
    async fn test_transport_request_response_roundtrip() {
        let request = OutboundMessage::Request {
            id: json!(42),
            method: "test/echo".into(),
            params: Some(json!({"msg": "hello"})),
        };
        let serialized = serde_json::to_string(&request).unwrap();

        // Simulate server receiving the request and sending a response
        let response_line = serde_json::to_string(&json!({
            "id": 42,
            "result": {"echo": "hello"}
        }))
        .unwrap();

        let parsed = parse_inbound(serde_json::from_str(&response_line).unwrap());
        match parsed {
            Some(InboundMessage::Response { id, result }) => {
                assert_eq!(id, json!(42));
                assert_eq!(result["echo"], "hello");
            }
            other => panic!("Expected Response, got {:?}", other),
        }

        // Verify the request we'd write is valid JSON
        let req_parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(req_parsed["method"], "test/echo");
    }
}
