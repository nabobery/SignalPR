use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::errors::ProviderError;
use crate::providers::jsonrpc::transport::JsonRpcTransport;
use crate::providers::jsonrpc::types::{
    FramingMode, JsonRpcTransportError, ServerNotification, ServerRequest,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A normalized session event unwrapped from the `session.event` notification envelope.
#[derive(Debug, Clone, Serialize)]
pub struct CopilotSessionEvent {
    pub session_id: String,
    pub event_type: String,
    pub event_id: String,
    pub event: Value,
}

/// A permission request extracted from a `permission.requested` session event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotPermissionRequest {
    pub session_id: String,
    pub event_id: String,
    pub kind: String,
    pub command: Option<String>,
    pub file_name: Option<String>,
    pub event: Value,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

struct Inner {
    transport: Option<JsonRpcTransport>,
    child: Option<tokio::process::Child>,
    child_cancel: Option<CancellationToken>,
    protocol_version: u64,
}

/// Manages the lifecycle of a persistent `copilot --server` child process and
/// provides high-level methods that map to the Copilot SDK v3 JSON-RPC API.
pub struct CopilotManager {
    inner: Arc<Mutex<Inner>>,
    permission_tx: broadcast::Sender<CopilotPermissionRequest>,
    event_tx: broadcast::Sender<CopilotSessionEvent>,
    session_by_lane: Arc<Mutex<HashMap<String, String>>>,
}

impl CopilotManager {
    pub fn new() -> Self {
        let (permission_tx, _) = broadcast::channel(128);
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                transport: None,
                child: None,
                child_cancel: None,
                protocol_version: 0,
            })),
            permission_tx,
            event_tx,
            session_by_lane: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_permissions(&self) -> broadcast::Receiver<CopilotPermissionRequest> {
        self.permission_tx.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<CopilotSessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn is_running(&self) -> bool {
        if let Ok(inner) = self.inner.try_lock() {
            inner.transport.is_some()
        } else {
            false
        }
    }

    /// Ensure the copilot server is running. Spawns the process if needed and
    /// performs a `ping` handshake to verify the connection.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.transport.is_some() {
            return Ok(());
        }

        let cli = std::env::var("COPILOT_CLI_PATH").unwrap_or_else(|_| "copilot".to_string());

        let mut child = tokio::process::Command::new(&cli)
            .arg("--server")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                ProviderError::CopilotFailed(format!("Failed to spawn copilot --server: {}", e))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ProviderError::CopilotFailed("Failed to capture copilot stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ProviderError::CopilotFailed("Failed to capture copilot stdout".into())
        })?;

        // Child-scoped cancellation token (allows manager restart after shutdown)
        let child_cancel = CancellationToken::new();

        let (_server_req_tx, mut _server_req_rx) = mpsc::channel::<ServerRequest>(128);
        let (notif_tx, mut notif_rx) = mpsc::channel::<ServerNotification>(1024);

        let transport = JsonRpcTransport::new(
            stdin,
            stdout,
            child_cancel.clone(),
            _server_req_tx,
            notif_tx,
            FramingMode::ContentLength,
        );

        // Perform ping handshake and detect protocol version
        let ping_result = transport
            .send_request("ping", None)
            .await
            .map_err(copilot_transport_error)?;

        let protocol_version = ping_result
            .get("protocolVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(3);

        info!("Copilot server v{} handshake successful", protocol_version);

        // Spawn task to unwrap session.event notifications and route by event.type
        {
            let event_tx = self.event_tx.clone();
            let permission_tx = self.permission_tx.clone();
            let cancel = child_cancel.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        notif = notif_rx.recv() => {
                            match notif {
                                Some(n) => {
                                    Self::route_notification(n, &event_tx, &permission_tx);
                                }
                                None => break,
                            }
                        }
                    }
                }
            });
        }

        inner.transport = Some(transport);
        inner.child = Some(child);
        inner.child_cancel = Some(child_cancel);
        inner.protocol_version = protocol_version;
        Ok(())
    }

    /// Unwrap a `session.event` notification into `CopilotSessionEvent` and
    /// route permission events to the permission broadcast.
    fn route_notification(
        notif: ServerNotification,
        event_tx: &broadcast::Sender<CopilotSessionEvent>,
        permission_tx: &broadcast::Sender<CopilotPermissionRequest>,
    ) {
        if notif.method == "session.event" {
            let Some(params) = notif.params else {
                return;
            };
            let session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let Some(event) = params.get("event").cloned() else {
                return;
            };
            let event_type = event
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let event_id = event
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            // Broadcast the normalized session event
            let session_event = CopilotSessionEvent {
                session_id: session_id.clone(),
                event_type: event_type.clone(),
                event_id: event_id.clone(),
                event: event.clone(),
            };
            let _ = event_tx.send(session_event);

            // If this is a permission request, also broadcast it on the permission channel
            if event_type == "permission.requested" {
                let kind = event
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let command = event
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let file_name = event
                    .get("fileName")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let perm_req = CopilotPermissionRequest {
                    session_id,
                    event_id,
                    kind,
                    command,
                    file_name,
                    event,
                };
                let _ = permission_tx.send(perm_req);
            }
        } else {
            // v2 fallback: wrap bare notification as a session event
            let event_type = notif.method.clone();
            let session_id = notif
                .params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let session_event = CopilotSessionEvent {
                session_id,
                event_type,
                event_id: String::new(),
                event: notif.params.unwrap_or(Value::Null),
            };
            let _ = event_tx.send(session_event);
        }
    }

    /// Create a new Copilot session for a review lane.
    pub async fn create_session(
        &self,
        lane_id: &str,
        system_prompt: &str,
        output_schema: &str,
        cwd: &std::path::Path,
        model: &str,
    ) -> Result<String, ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CopilotFailed("Copilot server not started".into()))?;

        let schema: Value = serde_json::from_str(output_schema).unwrap_or(json!({}));

        let params = json!({
            "model": model,
            "streaming": true,
            "systemMessage": {
                "content": system_prompt
            },
            "workingDirectory": cwd.to_string_lossy(),
            "tools": [{
                "name": "submit_review",
                "description": "Submit structured code review findings. Call this tool with your review results.",
                "parameters": schema
            }]
        });

        let result = transport
            .send_request("session.create", Some(params))
            .await
            .map_err(copilot_transport_error)?;

        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if session_id.is_empty() {
            return Err(ProviderError::CopilotFailed(
                "session.create returned empty sessionId".into(),
            ));
        }

        {
            let mut map = self.session_by_lane.lock().await;
            map.insert(session_id.clone(), lane_id.to_string());
        }

        debug!(
            "Created Copilot session {} for lane {}",
            session_id, lane_id
        );
        Ok(session_id)
    }

    /// Send a message (the diff) to an active session.
    pub async fn send_message(&self, session_id: &str, prompt: &str) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CopilotFailed("Copilot server not started".into()))?;

        let params = json!({
            "sessionId": session_id,
            "message": prompt,
            "mode": "send"
        });

        transport
            .send_request("session.send", Some(params))
            .await
            .map_err(copilot_transport_error)?;

        Ok(())
    }

    /// Respond to a v3 permission request via RPC.
    pub async fn respond_to_permission(
        &self,
        session_id: &str,
        event_id: &str,
        decision: &str,
    ) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CopilotFailed("Copilot server not started".into()))?;

        transport
            .send_request(
                "session.permissions.handlePendingPermissionRequest",
                Some(json!({
                    "sessionId": session_id,
                    "eventId": event_id,
                    "decision": decision
                })),
            )
            .await
            .map_err(copilot_transport_error)?;

        Ok(())
    }

    /// Respond to a v3 external tool call via RPC.
    pub async fn respond_to_tool_call(
        &self,
        session_id: &str,
        event_id: &str,
        result: Value,
    ) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CopilotFailed("Copilot server not started".into()))?;

        transport
            .send_request(
                "session.tools.handlePendingToolCall",
                Some(json!({
                    "sessionId": session_id,
                    "eventId": event_id,
                    "result": result
                })),
            )
            .await
            .map_err(copilot_transport_error)?;

        Ok(())
    }

    /// Abort current processing for a session.
    pub async fn abort_session(&self, session_id: &str) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CopilotFailed("Copilot server not started".into()))?;

        let _ = transport
            .send_request("session.abort", Some(json!({ "sessionId": session_id })))
            .await;

        Ok(())
    }

    /// Destroy a session and clean up the lane mapping.
    pub async fn destroy_session(&self, session_id: &str) -> Result<(), ProviderError> {
        {
            let inner = self.inner.lock().await;
            if let Some(ref transport) = inner.transport {
                let _ = transport
                    .send_request("session.destroy", Some(json!({ "sessionId": session_id })))
                    .await;
            }
        }
        self.unregister_session(session_id).await;
        Ok(())
    }

    /// Remove a session from the lane mapping.
    pub async fn unregister_session(&self, session_id: &str) {
        let mut map = self.session_by_lane.lock().await;
        map.remove(session_id);
    }

    /// Look up the lane_id associated with a session.
    pub async fn lane_for_session(&self, session_id: &str) -> Option<String> {
        let map = self.session_by_lane.lock().await;
        map.get(session_id).cloned()
    }

    /// Shut down the copilot server process and clean up state.
    /// The manager can be restarted by calling `ensure_started()` again.
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;

        // Cancel the child-scoped token (not a manager-wide one)
        if let Some(ref cancel) = inner.child_cancel {
            cancel.cancel();
        }
        if let Some(ref transport) = inner.transport {
            transport.shutdown();
        }
        if let Some(ref mut child) = inner.child {
            let _ = child.kill().await;
        }

        inner.transport = None;
        inner.child = None;
        inner.child_cancel = None;
        inner.protocol_version = 0;

        let mut map = self.session_by_lane.lock().await;
        map.clear();
    }
}

/// Map generic transport errors to `ProviderError::CopilotFailed`.
fn copilot_transport_error(e: JsonRpcTransportError) -> ProviderError {
    match e {
        JsonRpcTransportError::Cancelled => ProviderError::Cancelled,
        other => ProviderError::CopilotFailed(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_construction() {
        let manager = CopilotManager::new();
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_manager_not_started_errors() {
        let manager = CopilotManager::new();

        let result = manager
            .create_session(
                "security",
                "prompt",
                "{}",
                std::path::Path::new("/tmp"),
                "gpt-4.1",
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not started"));

        let result = manager.send_message("fake-session", "hello").await;
        assert!(result.is_err());

        let result = manager
            .respond_to_permission("session-1", "event-1", "approved")
            .await;
        assert!(result.is_err());

        let result = manager
            .respond_to_tool_call("session-1", "event-1", json!({}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shutdown_is_idempotent() {
        let manager = CopilotManager::new();
        manager.shutdown().await;
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_lane_for_session_empty() {
        let manager = CopilotManager::new();
        assert!(manager.lane_for_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_unregister_session() {
        let manager = CopilotManager::new();
        {
            let mut map = manager.session_by_lane.lock().await;
            map.insert("sess-1".into(), "security".into());
        }
        assert!(manager.lane_for_session("sess-1").await.is_some());
        manager.unregister_session("sess-1").await;
        assert!(manager.lane_for_session("sess-1").await.is_none());
    }

    #[test]
    fn test_copilot_transport_error_maps_cancelled() {
        let err = copilot_transport_error(JsonRpcTransportError::Cancelled);
        assert!(matches!(err, ProviderError::Cancelled));
    }

    #[test]
    fn test_copilot_transport_error_maps_others() {
        let err = copilot_transport_error(JsonRpcTransportError::ChannelClosed);
        assert!(matches!(err, ProviderError::CopilotFailed(_)));
    }

    #[test]
    fn test_route_notification_session_event_delta() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        let notif = ServerNotification {
            method: "session.event".into(),
            params: Some(json!({
                "sessionId": "sess-1",
                "event": {
                    "type": "assistant.message_delta",
                    "id": "evt-1",
                    "deltaContent": "hello "
                }
            })),
        };

        CopilotManager::route_notification(notif, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.session_id, "sess-1");
        assert_eq!(evt.event_type, "assistant.message_delta");
        assert_eq!(evt.event_id, "evt-1");
        assert_eq!(evt.event["deltaContent"], "hello ");
    }

    #[test]
    fn test_route_notification_permission_requested() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, mut perm_rx) = broadcast::channel(16);

        let notif = ServerNotification {
            method: "session.event".into(),
            params: Some(json!({
                "sessionId": "sess-2",
                "event": {
                    "type": "permission.requested",
                    "id": "perm-1",
                    "kind": "shell",
                    "command": "rm -rf /tmp/test"
                }
            })),
        };

        CopilotManager::route_notification(notif, &event_tx, &permission_tx);

        // Should appear on both channels
        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.event_type, "permission.requested");

        let perm = perm_rx.try_recv().unwrap();
        assert_eq!(perm.session_id, "sess-2");
        assert_eq!(perm.event_id, "perm-1");
        assert_eq!(perm.kind, "shell");
        assert_eq!(perm.command.as_deref(), Some("rm -rf /tmp/test"));
    }

    #[test]
    fn test_route_notification_v2_fallback() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        let notif = ServerNotification {
            method: "assistant.message_delta".into(),
            params: Some(json!({"sessionId": "sess-3", "deltaContent": "world"})),
        };

        CopilotManager::route_notification(notif, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.event_type, "assistant.message_delta");
        assert_eq!(evt.session_id, "sess-3");
    }

    #[test]
    fn test_route_notification_external_tool_requested() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        let notif = ServerNotification {
            method: "session.event".into(),
            params: Some(json!({
                "sessionId": "sess-4",
                "event": {
                    "type": "external_tool.requested",
                    "id": "tool-1",
                    "toolName": "submit_review",
                    "arguments": {"findings": []}
                }
            })),
        };

        CopilotManager::route_notification(notif, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.event_type, "external_tool.requested");
        assert_eq!(evt.event["toolName"], "submit_review");
    }
}
