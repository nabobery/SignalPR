use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;

use super::transport::{JsonRpcTransport, ServerNotification, ServerRequest};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// An approval request forwarded from the codex app-server.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// The JSON-RPC id that must be echoed back in the response.
    pub request_id: Value,
    /// The JSON-RPC method name (e.g. `codex/approveExec`).
    pub method: String,
    /// Thread id extracted from params, if present.
    pub thread_id: String,
    /// Turn id extracted from params, if present.
    pub turn_id: String,
    /// Item id extracted from params, if present.
    pub item_id: String,
    /// The full params object.
    pub params: Value,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct Inner {
    transport: Option<JsonRpcTransport>,
    child: Option<tokio::process::Child>,
}

/// Manages the lifecycle of a persistent `codex app-server` child process and
/// provides high-level methods that map to the app-server JSON-RPC API.
#[allow(dead_code)]
pub struct CodexAppServerManager {
    inner: Arc<Mutex<Inner>>,
    cancel: CancellationToken,
    /// Broadcast of server-initiated requests (approvals) to any interested consumers.
    approval_tx: broadcast::Sender<ApprovalRequest>,
    /// Broadcast of server notifications (streaming deltas, etc) to any interested consumers.
    notification_tx: broadcast::Sender<ServerNotification>,
    /// Mapping from app-server threadId -> SignalPR lane_id (e.g. "security").
    lane_by_thread: Arc<Mutex<HashMap<String, String>>>,
}

#[allow(dead_code)]
impl CodexAppServerManager {
    /// Create a new manager. Channels are used to surface server-initiated
    /// messages to the calling layer (usually Tauri commands).
    pub fn new() -> Self {
        let (approval_tx, _) = broadcast::channel(128);
        let (notification_tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                transport: None,
                child: None,
            })),
            cancel: CancellationToken::new(),
            approval_tx,
            notification_tx,
            lane_by_thread: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_approvals(&self) -> broadcast::Receiver<ApprovalRequest> {
        self.approval_tx.subscribe()
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<ServerNotification> {
        self.notification_tx.subscribe()
    }

    pub async fn register_thread_lane(&self, thread_id: String, lane_id: String) {
        let mut map = self.lane_by_thread.lock().await;
        map.insert(thread_id, lane_id);
    }

    pub async fn unregister_thread(&self, thread_id: &str) {
        let mut map = self.lane_by_thread.lock().await;
        map.remove(thread_id);
    }

    pub async fn lane_for_thread(&self, thread_id: &str) -> Option<String> {
        let map = self.lane_by_thread.lock().await;
        map.get(thread_id).cloned()
    }

    async fn get_transport(&self) -> Result<JsonRpcTransport, ProviderError> {
        let inner = self.inner.lock().await;
        inner
            .transport
            .as_ref()
            .cloned()
            .ok_or_else(|| ProviderError::CodexFailed("App-server not started".into()))
    }

    // -- lifecycle ----------------------------------------------------------

    /// Ensure the app-server process is running and the initialize handshake
    /// has been completed. Idempotent - if already running, this is a no-op.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.transport.is_some() {
            return Ok(());
        }

        info!("Spawning codex app-server child process");

        let mut child = tokio::process::Command::new("codex")
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ProviderError::CodexFailed(format!("Failed to spawn codex app-server: {}", e))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::CodexFailed("codex app-server has no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::CodexFailed("codex app-server has no stdout".into()))?;

        // Spawn a task that logs stderr
        if let Some(stderr) = child.stderr.take() {
            let cancel = self.cancel.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        line = lines.next_line() => {
                            match line {
                                Ok(Some(text)) => debug!("codex stderr: {}", text),
                                Ok(None) => break,
                                Err(e) => {
                                    warn!("Error reading codex stderr: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }

        // Set up channels for server requests / notifications
        let (srv_req_tx, mut srv_req_rx) = mpsc::channel::<ServerRequest>(64);
        let (notif_tx, mut notif_rx) = mpsc::channel::<ServerNotification>(256);

        let transport =
            JsonRpcTransport::new(stdin, stdout, self.cancel.clone(), srv_req_tx, notif_tx);

        // Forward server requests -> ApprovalRequest on our approval broadcast
        let approval_tx = self.approval_tx.clone();
        tokio::spawn(async move {
            while let Some(req) = srv_req_rx.recv().await {
                let params = req.params.clone().unwrap_or(Value::Null);
                let approval = ApprovalRequest {
                    request_id: req.id,
                    method: req.method,
                    thread_id: params
                        .get("threadId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    turn_id: params
                        .get("turnId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    item_id: params
                        .get("itemId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    params,
                };
                // Ignore errors when there are no receivers.
                let _ = approval_tx.send(approval);
            }
        });

        // Forward notifications directly
        let ext_notif_tx = self.notification_tx.clone();
        tokio::spawn(async move {
            while let Some(notif) = notif_rx.recv().await {
                let _ = ext_notif_tx.send(notif);
            }
        });

        // Perform the initialize handshake
        info!("Performing initialize handshake with codex app-server");
        let init_result = transport
            .send_request(
                "initialize",
                Some(json!({
                    "clientInfo": {
                        "name": "signalpr",
                        "title": "SignalPR",
                        "version": "0.1.0"
                    },
                    "capabilities": {}
                })),
            )
            .await?;

        debug!("Initialize response: {:?}", init_result);

        // Send initialized notification to complete the handshake
        transport.send_notification("initialized", None).await?;

        info!("Codex app-server handshake complete");

        inner.transport = Some(transport);
        inner.child = Some(child);
        Ok(())
    }

    // -- thread / turn API --------------------------------------------------

    fn parse_thread_id(result: &Value) -> Option<String> {
        // Docs: result.thread.id
        if let Some(id) = result
            .get("thread")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
        {
            return Some(id.to_string());
        }

        // Back-compat: result.threadId
        result
            .get("threadId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn parse_turn_id(result: &Value) -> Option<String> {
        // Docs: result.turn.id
        if let Some(id) = result
            .get("turn")
            .and_then(|t| t.get("id"))
            .and_then(|v| v.as_str())
        {
            return Some(id.to_string());
        }

        // Back-compat / other endpoints: result.turnId
        result
            .get("turnId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Start a new conversation thread. Returns the server-assigned thread id.
    pub async fn start_thread(&self, cwd: &Path, model: &str) -> Result<String, ProviderError> {
        let transport = self.get_transport().await?;

        let result = transport
            .send_request(
                "thread/start",
                Some(json!({
                    "cwd": cwd.to_string_lossy(),
                    "model": model,
                })),
            )
            .await?;

        let thread_id = Self::parse_thread_id(&result).ok_or_else(|| {
            ProviderError::CodexFailed(format!(
                "thread/start response missing thread.id/threadId: {:?}",
                result
            ))
        })?;

        debug!("Started thread: {}", thread_id);
        Ok(thread_id)
    }

    /// Start a turn within an existing thread. The server streams results back
    /// via notifications, so this returns once the request is acknowledged.
    pub async fn start_turn(
        &self,
        thread_id: &str,
        input: Vec<Value>,
        output_schema: Option<Value>,
    ) -> Result<String, ProviderError> {
        let transport = self.get_transport().await?;

        let mut params = json!({
            "threadId": thread_id,
            "input": input,
        });

        if let Some(schema) = output_schema {
            params
                .as_object_mut()
                .unwrap()
                .insert("outputSchema".into(), schema);
        }

        let result = transport.send_request("turn/start", Some(params)).await?;
        let turn_id = Self::parse_turn_id(&result).ok_or_else(|| {
            ProviderError::CodexFailed(format!(
                "turn/start response missing turn.id/turnId: {:?}",
                result
            ))
        })?;

        Ok(turn_id)
    }

    /// Interrupt a running turn.
    pub async fn interrupt_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<(), ProviderError> {
        let transport = self.get_transport().await?;

        transport
            .send_request(
                "turn/interrupt",
                Some(json!({
                    "threadId": thread_id,
                    "turnId": turn_id,
                })),
            )
            .await?;

        Ok(())
    }

    /// Respond to a server-initiated approval request.
    pub async fn respond_to_approval(
        &self,
        request_id: Value,
        decision: Value,
    ) -> Result<(), ProviderError> {
        let transport = self.get_transport().await?;

        transport.send_response(request_id, decision).await
    }

    /// Shut down the child process and cancel all in-flight operations.
    pub async fn shutdown(&self) {
        info!("Shutting down codex app-server");
        self.cancel.cancel();

        let mut inner = self.inner.lock().await;
        if let Some(ref mut child) = inner.child {
            if let Err(e) = child.kill().await {
                warn!("Failed to kill codex app-server process: {}", e);
            }
        }
        inner.transport = None;
        inner.child = None;
    }

    /// Check whether the app-server process is currently running.
    pub async fn is_running(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.transport.is_some()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thread_id_prefers_documented_shape() {
        let result = json!({
            "thread": { "id": "thr_123" }
        });
        assert_eq!(
            CodexAppServerManager::parse_thread_id(&result),
            Some("thr_123".into())
        );
    }

    #[test]
    fn test_parse_thread_id_falls_back_to_legacy_shape() {
        let result = json!({
            "threadId": "thr_legacy"
        });
        assert_eq!(
            CodexAppServerManager::parse_thread_id(&result),
            Some("thr_legacy".into())
        );
    }

    #[test]
    fn test_parse_turn_id_prefers_documented_shape() {
        let result = json!({
            "turn": { "id": "turn_456" }
        });
        assert_eq!(
            CodexAppServerManager::parse_turn_id(&result),
            Some("turn_456".into())
        );
    }

    #[test]
    fn test_parse_turn_id_falls_back_to_legacy_shape() {
        let result = json!({
            "turnId": "turn_legacy"
        });
        assert_eq!(
            CodexAppServerManager::parse_turn_id(&result),
            Some("turn_legacy".into())
        );
    }

    #[test]
    fn test_approval_request_serialization() {
        let req = ApprovalRequest {
            request_id: json!("req-1"),
            method: "codex/approveExec".into(),
            thread_id: "t-1".into(),
            turn_id: "turn-1".into(),
            item_id: "item-1".into(),
            params: json!({"command": "ls"}),
        };
        let s = serde_json::to_string(&req).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["method"], "codex/approveExec");
        assert_eq!(v["thread_id"], "t-1");
    }

    #[test]
    fn test_manager_construction() {
        let _manager = CodexAppServerManager::new();
        // Just verify it constructs without panicking
    }

    #[tokio::test]
    async fn test_manager_not_started_errors() {
        let manager = CodexAppServerManager::new();

        // Operations before ensure_started should return errors
        let result = manager
            .start_thread(Path::new("/tmp"), "gpt-5.2-codex")
            .await;
        assert!(result.is_err());
        match result {
            Err(ProviderError::CodexFailed(msg)) => {
                assert!(msg.contains("not started"), "Unexpected error: {}", msg);
            }
            other => panic!("Expected CodexFailed, got {:?}", other),
        }

        let result = manager
            .start_turn("t-1", vec![json!({"role": "user", "content": "hi"})], None)
            .await;
        assert!(result.is_err());

        let result = manager.interrupt_turn("t-1", "turn-1").await;
        assert!(result.is_err());

        let result = manager
            .respond_to_approval(json!("req-1"), json!("approve"))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_manager_is_running_false_initially() {
        let manager = CodexAppServerManager::new();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_shutdown_is_idempotent() {
        let manager = CodexAppServerManager::new();
        // Should not panic even when nothing is running
        manager.shutdown().await;
        manager.shutdown().await;
        assert!(!manager.is_running().await);
    }

    /// Test the handshake flow using a mock process (in-memory pipes).
    /// We simulate a codex app-server that responds to initialize.
    #[tokio::test]
    async fn test_handshake_with_mock_process() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        // Create duplex channels to simulate stdin/stdout
        let (client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);

        let cancel = CancellationToken::new();
        let (_srv_req_tx, _): (mpsc::Sender<ServerRequest>, _) = mpsc::channel(16);
        let (_notif_tx, _): (mpsc::Sender<ServerNotification>, _) = mpsc::channel(16);

        // We can't directly test CodexAppServerManager.ensure_started() because it
        // spawns a real process, but we can test the transport handshake pattern.

        // Spawn a mock server that responds to initialize
        let server_cancel = cancel.clone();
        let server_task = tokio::spawn(async move {
            let reader = BufReader::new(server_read);
            let mut lines = reader.lines();
            let mut writer = server_write;

            // Read the initialize request
            let line = lines.next_line().await.unwrap().unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(req["method"], "initialize");
            let req_id = req["id"].clone();

            // Send response
            let resp = json!({
                "id": req_id,
                "result": {
                    "serverInfo": {"name": "codex-app-server", "version": "0.1.0"},
                    "capabilities": {}
                }
            });
            let mut resp_line = serde_json::to_string(&resp).unwrap();
            resp_line.push('\n');
            writer.write_all(resp_line.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            // Read the initialized notification
            let line = lines.next_line().await.unwrap().unwrap();
            let notif: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(notif["method"], "initialized");
            assert!(notif.get("id").is_none());

            // Keep alive until cancelled
            server_cancel.cancelled().await;
        });

        // Create transport with the mock pipes using ChildStdin/ChildStdout
        // workaround: use the transport directly since we have the halves.
        //
        // Note: JsonRpcTransport::new expects ChildStdin/ChildStdout, but we
        // have DuplexStream halves. We test the protocol logic by manually doing
        // what the transport does.

        // Client side: write requests, read responses
        let mut client_writer = client_write;
        let client_reader = BufReader::new(client_read);
        let mut client_lines = client_reader.lines();

        // Send initialize request
        let init_req = json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {"name": "signalpr", "title": "SignalPR", "version": "0.1.0"},
                "capabilities": {}
            }
        });
        let mut line = serde_json::to_string(&init_req).unwrap();
        line.push('\n');
        client_writer.write_all(line.as_bytes()).await.unwrap();
        client_writer.flush().await.unwrap();

        // Read response
        let resp_line = client_lines.next_line().await.unwrap().unwrap();
        let resp: Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["id"], 1);
        assert!(resp.get("result").is_some());
        assert_eq!(resp["result"]["serverInfo"]["name"], "codex-app-server");

        // Send initialized notification
        let init_notif = json!({"method": "initialized"});
        let mut line = serde_json::to_string(&init_notif).unwrap();
        line.push('\n');
        client_writer.write_all(line.as_bytes()).await.unwrap();
        client_writer.flush().await.unwrap();

        // Clean up
        cancel.cancel();
        let _ = server_task.await;
    }
}
