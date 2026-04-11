use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use super::types::{
    inject_jsonrpc, parse_inbound, FramingMode, InboundMessage, JsonRpcError,
    JsonRpcTransportError, OutboundMessage, ServerNotification, ServerRequest,
};

type PendingMap = HashMap<String, oneshot::Sender<Result<Value, JsonRpcError>>>;

/// A bidirectional JSON-RPC 2.0 transport over child-process stdio.
///
/// Supports two framing modes:
/// - `NewlineDelimited`: one JSON object per `\n` (Codex CLI)
/// - `ContentLength`: `Content-Length: N\r\n\r\n{body}` (Copilot CLI, LSP-style)
///
/// Provider-agnostic: returns `JsonRpcTransportError` on failure.
/// Each provider maps these to its own `ProviderError` variant at the call site.
pub struct JsonRpcTransport {
    writer_tx: mpsc::Sender<OutboundMessage>,
    pending: Arc<Mutex<PendingMap>>,
    next_id: Arc<AtomicU64>,
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

impl JsonRpcTransport {
    /// Create a new transport, spawning reader and writer tasks.
    pub fn new(
        stdin: ChildStdin,
        stdout: tokio::process::ChildStdout,
        cancel: CancellationToken,
        server_request_tx: mpsc::Sender<ServerRequest>,
        notification_tx: mpsc::Sender<ServerNotification>,
        framing: FramingMode,
    ) -> Self {
        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let (writer_tx, writer_rx) = mpsc::channel::<OutboundMessage>(64);

        let writer_cancel = cancel.clone();
        tokio::spawn(Self::writer_loop(stdin, writer_rx, writer_cancel, framing));

        let reader_cancel = cancel.clone();
        let reader_pending = Arc::clone(&pending);
        tokio::spawn(Self::reader_loop(
            stdout,
            reader_pending,
            server_request_tx,
            notification_tx,
            reader_cancel,
            framing,
        ));

        Self {
            writer_tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            cancel,
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    pub async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcTransportError> {
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
            .map_err(|_| JsonRpcTransportError::ChannelClosed)?;

        tokio::select! {
            _ = self.cancel.cancelled() => {
                let mut map = self.pending.lock().await;
                map.remove(&id_key);
                Err(JsonRpcTransportError::Cancelled)
            }
            result = rx => {
                match result {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(rpc_err)) => Err(JsonRpcTransportError::RpcError {
                        code: rpc_err.code,
                        message: rpc_err.message,
                    }),
                    Err(_) => Err(JsonRpcTransportError::ResponseDropped),
                }
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), JsonRpcTransportError> {
        let msg = OutboundMessage::Notification {
            method: method.to_string(),
            params,
        };
        self.writer_tx
            .send(msg)
            .await
            .map_err(|_| JsonRpcTransportError::ChannelClosed)
    }

    /// Send a response to a server-initiated request.
    pub async fn send_response(
        &self,
        id: Value,
        result: Value,
    ) -> Result<(), JsonRpcTransportError> {
        let msg = OutboundMessage::Response { id, result };
        self.writer_tx
            .send(msg)
            .await
            .map_err(|_| JsonRpcTransportError::ChannelClosed)
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
        framing: FramingMode,
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
                            // Always inject "jsonrpc":"2.0" then serialize
                            let body = match serde_json::to_string(&inject_jsonrpc(&m)) {
                                Ok(s) => s,
                                Err(e) => {
                                    error!("Failed to serialize outbound message: {}", e);
                                    continue;
                                }
                            };

                            let wire_bytes = match framing {
                                FramingMode::NewlineDelimited => {
                                    format!("{}\n", body).into_bytes()
                                }
                                FramingMode::ContentLength => {
                                    let len = body.len();
                                    format!("Content-Length: {}\r\n\r\n{}", len, body).into_bytes()
                                }
                            };

                            if let Err(e) = stdin.write_all(&wire_bytes).await {
                                error!("Failed to write to server stdin: {}", e);
                                break;
                            }
                            if let Err(e) = stdin.flush().await {
                                error!("Failed to flush server stdin: {}", e);
                                break;
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
        framing: FramingMode,
    ) {
        let mut reader = BufReader::new(stdout);

        loop {
            let raw: Value = match framing {
                FramingMode::NewlineDelimited => {
                    match Self::read_newline_delimited(&mut reader, &cancel).await {
                        ReadResult::Value(v) => v,
                        ReadResult::Skip => continue,
                        ReadResult::Stop => break,
                    }
                }
                FramingMode::ContentLength => {
                    match Self::read_content_length(&mut reader, &cancel).await {
                        ReadResult::Value(v) => v,
                        ReadResult::Skip => continue,
                        ReadResult::Stop => break,
                    }
                }
            };

            Self::dispatch_inbound(raw, &pending, &server_request_tx, &notification_tx).await;
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

    /// Read one newline-delimited JSON message.
    async fn read_newline_delimited(
        reader: &mut BufReader<tokio::process::ChildStdout>,
        cancel: &CancellationToken,
    ) -> ReadResult {
        let mut line = String::new();
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!("JSON-RPC reader shutting down (cancelled)");
                ReadResult::Stop
            }
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => {
                        debug!("Server stdout EOF - reader exiting");
                        ReadResult::Stop
                    }
                    Ok(_) => {
                        let text = line.trim().to_string();
                        if text.is_empty() {
                            return ReadResult::Skip;
                        }
                        match serde_json::from_str(&text) {
                            Ok(v) => ReadResult::Value(v),
                            Err(e) => {
                                warn!("Non-JSON line from server stdout: {} ({})", text, e);
                                ReadResult::Skip
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading server stdout: {}", e);
                        ReadResult::Stop
                    }
                }
            }
        }
    }

    /// Read one Content-Length framed JSON-RPC message (LSP-style).
    ///
    /// Format: `Content-Length: N\r\n[Header: V\r\n]*\r\n<N bytes of JSON body>`
    async fn read_content_length(
        reader: &mut BufReader<tokio::process::ChildStdout>,
        cancel: &CancellationToken,
    ) -> ReadResult {
        // 1. Read headers until blank line
        let mut content_length: Option<usize> = None;
        loop {
            let mut header_line = String::new();
            let read_result = tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("JSON-RPC reader shutting down (cancelled)");
                    return ReadResult::Stop;
                }
                r = reader.read_line(&mut header_line) => r
            };

            match read_result {
                Ok(0) => {
                    debug!("Server stdout EOF during header read");
                    return ReadResult::Stop;
                }
                Ok(_) => {
                    let trimmed = header_line.trim();
                    if trimmed.is_empty() {
                        // Blank line = end of headers
                        break;
                    }
                    if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                        if let Ok(len) = value.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                    // Ignore other headers (e.g. Content-Type)
                }
                Err(e) => {
                    error!("Error reading header: {}", e);
                    return ReadResult::Stop;
                }
            }
        }

        // 2. Read exactly N bytes of body
        let Some(len) = content_length else {
            warn!("Content-Length header missing; skipping message");
            return ReadResult::Skip;
        };

        let mut body = vec![0u8; len];
        let read_result = tokio::select! {
            _ = cancel.cancelled() => {
                debug!("JSON-RPC reader shutting down (cancelled)");
                return ReadResult::Stop;
            }
            r = reader.read_exact(&mut body) => r
        };

        match read_result {
            Ok(_) => match serde_json::from_slice(&body) {
                Ok(v) => ReadResult::Value(v),
                Err(e) => {
                    warn!("Invalid JSON in Content-Length body: {}", e);
                    ReadResult::Skip
                }
            },
            Err(e) => {
                error!("Error reading Content-Length body: {}", e);
                ReadResult::Stop
            }
        }
    }

    /// Route a parsed inbound message to the appropriate handler.
    async fn dispatch_inbound(
        raw: Value,
        pending: &Arc<Mutex<PendingMap>>,
        server_request_tx: &mpsc::Sender<ServerRequest>,
        notification_tx: &mpsc::Sender<ServerNotification>,
    ) {
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
}

enum ReadResult {
    Value(Value),
    Skip,
    Stop,
}
