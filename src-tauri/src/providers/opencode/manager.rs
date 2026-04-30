use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::errors::ProviderError;
use crate::secrets::credentials::{self, ProviderCredentialField};

use super::sse::{self, SseEvent};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A normalized session event from the OpenCode SSE stream.
/// After bus envelope unwrap in sse.rs, `event_type` is the inner bus type
/// and `data` is the inner `properties` object.
#[derive(Debug, Clone, Serialize)]
pub struct OpenCodeSessionEvent {
    pub session_id: String,
    pub event_type: String,
    pub data: Value,
}

/// A permission request extracted from a `permission.asked` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodePermissionRequest {
    pub session_id: String,
    pub request_id: String,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: Value,
    pub tool: Option<String>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

struct Inner {
    child: Option<tokio::process::Child>,
    child_cancel: Option<CancellationToken>,
    base_url: Option<String>,
    auth_password: Option<String>,
    client: reqwest::Client,
}

/// Manages the lifecycle of a persistent `opencode serve` child process and
/// provides high-level methods that map to the OpenCode HTTP REST API.
#[allow(dead_code)]
pub struct OpenCodeManager {
    inner: Arc<Mutex<Inner>>,
    permission_tx: broadcast::Sender<OpenCodePermissionRequest>,
    event_tx: broadcast::Sender<OpenCodeSessionEvent>,
    session_by_lane: Arc<Mutex<HashMap<String, String>>>,
}

fn auth_username() -> String {
    std::env::var("OPENCODE_SERVER_USERNAME").unwrap_or_else(|_| "opencode".to_string())
}

/// Parse a model string like "anthropic/claude-sonnet-4-5" into
/// `{ "providerID": "anthropic", "modelID": "claude-sonnet-4-5" }`.
/// If no slash is present, returns just `{ "modelID": "<model>" }`.
pub fn parse_model_string(model: &str) -> Value {
    if let Some((provider, model_id)) = model.split_once('/') {
        json!({ "providerID": provider, "modelID": model_id })
    } else {
        json!({ "modelID": model })
    }
}

#[allow(dead_code)]
impl OpenCodeManager {
    pub fn new() -> Self {
        let (permission_tx, _) = broadcast::channel(128);
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                child: None,
                child_cancel: None,
                base_url: None,
                auth_password: None,
                client: reqwest::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(10))
                    .timeout(std::time::Duration::from_secs(600))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            })),
            permission_tx,
            event_tx,
            session_by_lane: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_permissions(&self) -> broadcast::Receiver<OpenCodePermissionRequest> {
        self.permission_tx.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<OpenCodeSessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn is_running(&self) -> bool {
        if let Ok(inner) = self.inner.try_lock() {
            inner.base_url.is_some()
        } else {
            false
        }
    }

    /// Ensure the OpenCode server is running. Spawns the process if needed,
    /// detects the port, starts the SSE listener, and verifies health.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.base_url.is_some() {
            return Ok(());
        }

        let cli = std::env::var("OPENCODE_CLI_PATH").unwrap_or_else(|_| "opencode".to_string());
        let auth_password =
            credentials::resolve_credential(ProviderCredentialField::OpenCodeServerPassword)
                .ok()
                .and_then(|(value, _)| value);

        let mut cmd = tokio::process::Command::new(&cli);
        cmd.args(["serve", "--port", "0", "--hostname", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(ref password) = auth_password {
            cmd.env("OPENCODE_SERVER_PASSWORD", password);
        }

        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::OpenCodeFailed(format!("Failed to spawn opencode serve: {}", e))
        })?;

        // Drain stdout in background to prevent pipe backpressure
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(_)) = lines.next_line().await {}
            });
        }

        let stderr = child.stderr.take().ok_or_else(|| {
            ProviderError::OpenCodeFailed("Failed to capture opencode stderr".into())
        })?;

        // Parse port from stderr output (e.g. "Listening on http://127.0.0.1:54321")
        let port = detect_port(stderr).await.map_err(|e| {
            ProviderError::OpenCodeFailed(format!("Failed to detect OpenCode port: {}", e))
        })?;

        let base_url = format!("http://127.0.0.1:{}", port);
        info!("OpenCode server started on {}", base_url);

        // Child-scoped cancellation token (allows manager restart after shutdown)
        let child_cancel = CancellationToken::new();

        // Start SSE listener in background
        {
            let (sse_tx, mut sse_rx) = mpsc::channel::<SseEvent>(1024);
            let sse_url = base_url.clone();
            let sse_auth = auth_password.clone();
            let sse_cancel = child_cancel.clone();
            tokio::spawn(async move {
                sse::sse_listener(sse_url, sse_auth, sse_tx, sse_cancel).await;
            });

            // Route SSE events to broadcast channels
            let event_tx = self.event_tx.clone();
            let permission_tx = self.permission_tx.clone();
            let route_cancel = child_cancel.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = route_cancel.cancelled() => break,
                        event = sse_rx.recv() => {
                            match event {
                                Some(e) => Self::route_sse_event(e, &event_tx, &permission_tx),
                                None => break,
                            }
                        }
                    }
                }
            });
        }

        // Verify health with retries (include auth if configured)
        let health_url = format!("{}/global/health", base_url);
        let mut health_ok = false;
        for attempt in 0..10 {
            let mut req = inner.client.get(&health_url);
            if let Some(ref pw) = auth_password {
                req = req.basic_auth(auth_username(), Some(pw));
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    health_ok = true;
                    break;
                }
                _ => {
                    if attempt < 9 {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }

        if !health_ok {
            let _ = child.kill().await;
            child_cancel.cancel();
            return Err(ProviderError::OpenCodeFailed(
                "OpenCode server health check failed after startup".into(),
            ));
        }

        inner.base_url = Some(base_url);
        inner.auth_password = auth_password;
        inner.child = Some(child);
        inner.child_cancel = Some(child_cancel);

        Ok(())
    }

    /// Route an unwrapped SSE event into typed broadcast channels.
    /// After bus envelope unwrap in sse.rs, `event.data` is already `properties`.
    fn route_sse_event(
        event: SseEvent,
        event_tx: &broadcast::Sender<OpenCodeSessionEvent>,
        permission_tx: &broadcast::Sender<OpenCodePermissionRequest>,
    ) {
        // Extract session ID from the unwrapped properties.
        // Different event types nest the sessionID at different paths.
        let session_id = event
            .data
            .get("sessionID")
            .or_else(|| event.data.get("part").and_then(|p| p.get("sessionID")))
            .or_else(|| event.data.get("info").and_then(|i| i.get("sessionID")))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let session_event = OpenCodeSessionEvent {
            session_id: session_id.clone(),
            event_type: event.event_type.clone(),
            data: event.data.clone(),
        };
        let _ = event_tx.send(session_event);

        // Route permission requests to the dedicated channel
        if event.event_type == "permission.asked" {
            let request_id = event
                .data
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let permission = event
                .data
                .get("permission")
                .or_else(|| event.data.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let patterns = event
                .data
                .get("patterns")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let tool = event
                .data
                .get("tool")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let metadata = event
                .data
                .get("metadata")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));

            let perm_req = OpenCodePermissionRequest {
                session_id,
                request_id,
                permission,
                patterns,
                metadata,
                tool,
            };
            let _ = permission_tx.send(perm_req);
        }
    }

    /// Send an HTTP request to the OpenCode server.
    async fn http_request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ProviderError> {
        let inner = self.inner.lock().await;
        let base_url = inner
            .base_url
            .as_ref()
            .ok_or_else(|| ProviderError::OpenCodeFailed("OpenCode server not started".into()))?;

        let url = format!("{}{}", base_url, path);
        let mut request = inner.client.request(method, &url);
        if let Some(ref pw) = inner.auth_password {
            request = request.basic_auth(auth_username(), Some(pw));
        }
        if let Some(b) = body {
            request = request.json(&b);
        }

        let response = request.send().await.map_err(|e| {
            ProviderError::OpenCodeFailed(format!("HTTP request to {} failed: {}", path, e))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(ProviderError::OpenCodeFailed(format!(
                "OpenCode {} returned {}: {}",
                path, status, body_text
            )));
        }

        let body_text = response.text().await.unwrap_or_default();
        if body_text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body_text).map_err(|e| {
            ProviderError::OpenCodeFailed(format!("Failed to parse response from {}: {}", path, e))
        })
    }

    /// Create a new OpenCode session for a review lane.
    /// Sessions are lightweight — model/prompt/tools go on the message call.
    pub async fn create_session(&self, lane_id: &str) -> Result<String, ProviderError> {
        let params = json!({
            "title": format!("SignalPR: {}", lane_id)
        });

        let result = self
            .http_request(reqwest::Method::POST, "/session", Some(params))
            .await?;

        let session_id = result
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if session_id.is_empty() {
            return Err(ProviderError::OpenCodeFailed(
                "POST /session returned empty session id".into(),
            ));
        }

        {
            let mut map = self.session_by_lane.lock().await;
            map.insert(session_id.clone(), lane_id.to_string());
        }

        debug!(
            "Created OpenCode session {} for lane {}",
            session_id, lane_id
        );
        Ok(session_id)
    }

    /// Send a message to an active session with structured output format.
    /// Uses synchronous `POST /session/{id}/message` which returns the
    /// assistant response (including `info.structured`) directly.
    pub async fn send_message(
        &self,
        session_id: &str,
        system_prompt: &str,
        diff: &str,
        output_schema: &str,
        model: &str,
    ) -> Result<Value, ProviderError> {
        let schema: Value = serde_json::from_str(output_schema).unwrap_or(json!({}));
        let model_obj = parse_model_string(model);

        let path = format!("/session/{}/message", session_id);
        let body = json!({
            "parts": [{ "type": "text", "text": diff }],
            "system": system_prompt,
            "model": model_obj,
            "format": {
                "type": "json_schema",
                "schema": schema,
                "retryCount": 2
            }
        });

        self.http_request(reqwest::Method::POST, &path, Some(body))
            .await
    }

    /// Respond to a permission request with `once`, `always`, or `reject`.
    pub async fn respond_to_permission(
        &self,
        request_id: &str,
        reply: &str,
    ) -> Result<(), ProviderError> {
        let path = format!("/permission/{}/reply", request_id);
        let body = json!({ "reply": reply });
        self.http_request(reqwest::Method::POST, &path, Some(body))
            .await?;
        Ok(())
    }

    /// Abort current processing for a session.
    pub async fn abort_session(&self, session_id: &str) -> Result<(), ProviderError> {
        let path = format!("/session/{}/abort", session_id);
        let _ = self.http_request(reqwest::Method::POST, &path, None).await;
        Ok(())
    }

    /// Destroy a session and clean up the lane mapping.
    pub async fn destroy_session(&self, session_id: &str) -> Result<(), ProviderError> {
        let path = format!("/session/{}", session_id);
        let _ = self
            .http_request(reqwest::Method::DELETE, &path, None)
            .await;
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

    /// Shut down the OpenCode server process and clean up state.
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;

        if let Some(ref cancel) = inner.child_cancel {
            cancel.cancel();
        }
        if let Some(ref mut child) = inner.child {
            let _ = child.kill().await;
        }

        inner.child = None;
        inner.child_cancel = None;
        inner.base_url = None;
        inner.auth_password = None;

        let mut map = self.session_by_lane.lock().await;
        map.clear();
    }
}

/// Detect the port from the OpenCode server's stderr output.
async fn detect_port(stderr: tokio::process::ChildStderr) -> Result<u16, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let reader = BufReader::new(stderr);
    let mut lines = reader.lines();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let line = tokio::time::timeout(remaining, lines.next_line())
            .await
            .map_err(|_| "Timed out waiting for port".to_string())?
            .map_err(|e| format!("IO error reading stderr: {}", e))?;

        let Some(line) = line else {
            return Err("stderr closed before port detected".into());
        };

        debug!("opencode stderr: {}", line);

        for pattern in &["http://127.0.0.1:", "http://localhost:", "http://0.0.0.0:"] {
            if let Some(idx) = line.find(pattern) {
                let port_start = idx + pattern.len();
                let port_str: String = line[port_start..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(port) = port_str.parse::<u16>() {
                    return Ok(port);
                }
            }
        }
    }

    Err("Could not detect port from opencode server output".into())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_construction() {
        let manager = OpenCodeManager::new();
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_manager_not_started_errors() {
        let manager = OpenCodeManager::new();

        let result = manager.create_session("security").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not started"));

        let result = manager
            .send_message("fake", "sys", "diff", "{}", "anthropic/claude-sonnet-4-5")
            .await;
        assert!(result.is_err());

        let result = manager.respond_to_permission("req-1", "once").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shutdown_is_idempotent() {
        let manager = OpenCodeManager::new();
        manager.shutdown().await;
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_lane_for_session_empty() {
        let manager = OpenCodeManager::new();
        assert!(manager.lane_for_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_unregister_session() {
        let manager = OpenCodeManager::new();
        {
            let mut map = manager.session_by_lane.lock().await;
            map.insert("sess-1".into(), "security".into());
        }
        assert!(manager.lane_for_session("sess-1").await.is_some());
        manager.unregister_session("sess-1").await;
        assert!(manager.lane_for_session("sess-1").await.is_none());
    }

    #[test]
    fn test_parse_model_string_with_provider() {
        let result = parse_model_string("anthropic/claude-sonnet-4-5");
        assert_eq!(result["providerID"], "anthropic");
        assert_eq!(result["modelID"], "claude-sonnet-4-5");
    }

    #[test]
    fn test_parse_model_string_without_provider() {
        let result = parse_model_string("gpt-4.1");
        assert!(result.get("providerID").is_none());
        assert_eq!(result["modelID"], "gpt-4.1");
    }

    #[test]
    fn test_route_sse_event_delta() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        // After bus unwrap, data is already the properties object
        let event = SseEvent {
            event_type: "message.part.updated".into(),
            data: json!({ "sessionID": "sess-1", "delta": "hello " }),
            id: None,
        };

        OpenCodeManager::route_sse_event(event, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.session_id, "sess-1");
        assert_eq!(evt.event_type, "message.part.updated");
        assert_eq!(evt.data["delta"], "hello ");
    }

    #[test]
    fn test_route_sse_event_permission() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, mut perm_rx) = broadcast::channel(16);

        let event = SseEvent {
            event_type: "permission.asked".into(),
            data: json!({
                "sessionID": "sess-2",
                "requestID": "perm-1",
                "permission": "shell",
                "patterns": ["rm *"],
                "tool": "bash"
            }),
            id: None,
        };

        OpenCodeManager::route_sse_event(event, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.event_type, "permission.asked");

        let perm = perm_rx.try_recv().unwrap();
        assert_eq!(perm.session_id, "sess-2");
        assert_eq!(perm.request_id, "perm-1");
        assert_eq!(perm.permission, "shell");
        assert_eq!(perm.patterns, vec!["rm *"]);
        assert_eq!(perm.tool.as_deref(), Some("bash"));
    }

    #[test]
    fn test_route_sse_event_session_status() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        let event = SseEvent {
            event_type: "session.status".into(),
            data: json!({ "sessionID": "sess-3", "status": "idle" }),
            id: None,
        };

        OpenCodeManager::route_sse_event(event, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.event_type, "session.status");
        assert_eq!(evt.data["status"], "idle");
    }

    #[test]
    fn test_route_sse_nested_session_id() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let (permission_tx, _) = broadcast::channel(16);

        // message.part.updated nests sessionID inside `part`
        let event = SseEvent {
            event_type: "message.part.updated".into(),
            data: json!({ "part": { "sessionID": "sess-nested" }, "delta": "x" }),
            id: None,
        };

        OpenCodeManager::route_sse_event(event, &event_tx, &permission_tx);

        let evt = event_rx.try_recv().unwrap();
        assert_eq!(evt.session_id, "sess-nested");
    }
}
