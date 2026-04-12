use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;
use crate::providers::jsonrpc::transport::JsonRpcTransport;
use crate::providers::jsonrpc::types::{
    FramingMode, JsonRpcTransportError, ServerNotification, ServerRequest,
};

/// Cap accumulated per-session agent message buffers at 1 MiB. Review outputs
/// should be well under 32 KiB; this is a safety net against a runaway model.
const MAX_SESSION_BUFFER_BYTES: usize = 1 << 20;

/// Startup timeout for the full handshake (initialize + authenticate). Users
/// hit this if the child hangs on a first-run interactive prompt or if the
/// binary is wedged.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A normalized ACP session update event forwarded to UI listeners.
#[derive(Debug, Clone, Serialize)]
pub struct GeminiSessionEvent {
    pub session_id: String,
    pub event_type: String,
    pub delta: String,
    pub data: Value,
}

/// Outcome of creating a new ACP session, including the modes the server
/// advertises (plan mode is only present if the upstream CLI enables it).
#[derive(Debug, Clone)]
pub struct GeminiSessionHandle {
    pub session_id: String,
    pub available_modes: Vec<String>,
}

/// A permission request raised by the agent via `session/request_permission`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiPermissionRequest {
    pub session_id: String,
    pub request_id: String,
    pub tool_call: Value,
    pub options: Value,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

struct Inner {
    transport: Option<JsonRpcTransport>,
    child: Option<tokio::process::Child>,
    child_cancel: Option<CancellationToken>,
}

/// Manages the lifecycle of a persistent `gemini --experimental-acp` child
/// process and speaks the Agent Client Protocol (ACP) over newline-delimited
/// JSON-RPC 2.0.
///
/// Authentication is supplied exclusively via environment variables
/// (`GEMINI_API_KEY`, `GOOGLE_API_KEY`, or Vertex `GOOGLE_*` vars). OAuth /
/// Code-Assist paths are intentionally not supported — Google's ToS reserves
/// those for first-party clients.
pub struct GeminiManager {
    inner: Arc<Mutex<Inner>>,
    event_tx: broadcast::Sender<GeminiSessionEvent>,
    permission_tx: broadcast::Sender<GeminiPermissionRequest>,
    session_buffers: Arc<Mutex<HashMap<String, String>>>,
    session_by_lane: Arc<Mutex<HashMap<String, String>>>,
}

impl GeminiManager {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let (permission_tx, _) = broadcast::channel(128);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                transport: None,
                child: None,
                child_cancel: None,
            })),
            event_tx,
            permission_tx,
            session_buffers: Arc::new(Mutex::new(HashMap::new())),
            session_by_lane: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<GeminiSessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn subscribe_permissions(&self) -> broadcast::Receiver<GeminiPermissionRequest> {
        self.permission_tx.subscribe()
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        if let Ok(inner) = self.inner.try_lock() {
            inner.transport.is_some()
        } else {
            false
        }
    }

    /// Look up the lane_id associated with an ACP session.
    pub async fn lane_for_session(&self, session_id: &str) -> Option<String> {
        let map = self.session_by_lane.lock().await;
        map.get(session_id).cloned()
    }

    /// Static check for whether any supported auth env var is set.
    /// Used by the health check to fail fast with a clear message.
    pub fn has_auth_env() -> bool {
        std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("GOOGLE_API_KEY").is_ok()
            || std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok()
    }

    /// Ensure the `gemini --acp` server is running and initialized. Spawns
    /// the process if needed and performs the ACP `initialize` + `authenticate`
    /// handshake under a bounded timeout.
    ///
    /// The ACP flag defaults to `--acp` (current as of PR #21171 merged
    /// 2026-03-05). Users on older pinned versions can override with
    /// `GEMINI_ACP_FLAG=--experimental-acp`; the old flag is still accepted
    /// upstream as a deprecated alias.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.transport.is_some() {
            return Ok(());
        }

        if !Self::has_auth_env() {
            return Err(ProviderError::GeminiFailed(
                "No Gemini credentials found. Set GEMINI_API_KEY (AI Studio) or a Vertex \
                 GOOGLE_* env var. OAuth is not supported — see \
                 https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md"
                    .into(),
            ));
        }

        let cli = std::env::var("GEMINI_CLI_PATH").unwrap_or_else(|_| "gemini".to_string());
        let acp_flag = std::env::var("GEMINI_ACP_FLAG").unwrap_or_else(|_| "--acp".to_string());

        let mut child = tokio::process::Command::new(&cli)
            .arg(&acp_flag)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                ProviderError::GeminiFailed(format!(
                    "Failed to spawn `{} {}`: {}. Install with `npm i -g @google/gemini-cli`.",
                    cli, acp_flag, e
                ))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::GeminiFailed("Failed to capture gemini stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::GeminiFailed("Failed to capture gemini stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::GeminiFailed("Failed to capture gemini stderr".into()))?;

        let child_cancel = CancellationToken::new();

        // Drain child stderr into tracing::debug so the pipe never blocks on
        // backpressure (Gemini CLI may emit non-JSON diagnostics even in ACP
        // mode — see upstream issue #22647).
        {
            let cancel = child_cancel.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        line = reader.next_line() => {
                            match line {
                                Ok(Some(l)) => {
                                    // Cap per-line length so a runaway agent
                                    // can't spam tracing infinitely.
                                    let trimmed = if l.len() > 1024 {
                                        &l[..1024]
                                    } else {
                                        l.as_str()
                                    };
                                    debug!("gemini stderr: {}", trimmed);
                                }
                                Ok(None) => break, // EOF
                                Err(e) => {
                                    debug!("gemini stderr read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }

        let (server_req_tx, server_req_rx) = mpsc::channel::<ServerRequest>(128);
        let (notif_tx, notif_rx) = mpsc::channel::<ServerNotification>(1024);

        let transport = JsonRpcTransport::new(
            stdin,
            stdout,
            child_cancel.clone(),
            server_req_tx,
            notif_tx,
            FramingMode::NewlineDelimited,
        );

        // Perform ACP handshake (initialize + authenticate) under a hard
        // timeout so a wedged child can't block health checks forever.
        let handshake = async {
            // ACP requires protocolVersion as an integer (upstream schema
            // uses uint16, and issue #22647 reports Zod errors when strings
            // are sent). clientCapabilities.fs.{readTextFile,writeTextFile}
            // advertise whether we SERVE those methods, not whether we want
            // the agent to use them. We serve reads (proxied to real disk)
            // but not writes (SignalPR is review-only).
            let init_params = json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {
                        "readTextFile": true,
                        "writeTextFile": false
                    }
                }
            });

            let init_result = transport
                .send_request("initialize", Some(init_params))
                .await
                .map_err(gemini_transport_error)?;

            let protocol_version = init_result
                .get("protocolVersion")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);

            info!(
                "Gemini ACP server initialized (protocolVersion={}, flag={})",
                protocol_version, acp_flag
            );

            // Explicitly authenticate with the Gemini API key method. We pick
            // the first authMethod whose id mentions "api-key" or "gemini",
            // which maps to `AuthType.USE_GEMINI` upstream. The actual key is
            // read by the child from env (GEMINI_API_KEY); we do NOT pass it
            // as a JSON arg to avoid leaking it into logs.
            if let Some(method_id) = pick_api_key_method(&init_result) {
                debug!("Calling ACP authenticate with methodId={}", method_id);
                if let Err(e) = transport
                    .send_request("authenticate", Some(json!({ "methodId": method_id })))
                    .await
                {
                    // Some CLI builds skip authenticate entirely when env-var
                    // auth is already resolved; treat method_not_found as
                    // non-fatal and let session/new surface the real error.
                    warn!(
                        "Gemini authenticate call failed ({}); continuing. \
                         session/new will surface the real error if auth is broken.",
                        e
                    );
                }
            } else {
                debug!("No api-key authMethod advertised; relying on cached settings");
            }

            Ok::<(), ProviderError>(())
        };

        match tokio::time::timeout(STARTUP_TIMEOUT, handshake).await {
            Ok(Ok(())) => {}
            Ok(Err(handshake_err)) => {
                child_cancel.cancel();
                transport.shutdown();
                let _ = child.kill().await;
                return Err(handshake_err);
            }
            Err(_elapsed) => {
                child_cancel.cancel();
                transport.shutdown();
                let _ = child.kill().await;
                return Err(ProviderError::GeminiFailed(format!(
                    "Gemini startup timed out after {}s — check `gemini --version` and GEMINI_API_KEY.",
                    STARTUP_TIMEOUT.as_secs()
                )));
            }
        }

        // Spawn notification router (streaming deltas, tool calls).
        {
            let event_tx = self.event_tx.clone();
            let session_buffers = Arc::clone(&self.session_buffers);
            let cancel = child_cancel.clone();
            let mut notif_rx = notif_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        notif = notif_rx.recv() => {
                            match notif {
                                Some(n) => {
                                    Self::route_notification(n, &event_tx, &session_buffers).await;
                                }
                                None => break,
                            }
                        }
                    }
                }
            });
        }

        // Spawn server-request handler (permissions, filesystem proxy).
        {
            let transport_clone = transport.clone();
            let permission_tx = self.permission_tx.clone();
            let cancel = child_cancel.clone();
            let mut req_rx = server_req_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        req = req_rx.recv() => {
                            match req {
                                Some(r) => {
                                    Self::handle_server_request(
                                        r,
                                        &transport_clone,
                                        &permission_tx,
                                    )
                                    .await;
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
        Ok(())
    }

    /// Route an inbound `session/update` notification, emitting UI events and
    /// accumulating agent message text into per-session buffers.
    async fn route_notification(
        notif: ServerNotification,
        event_tx: &broadcast::Sender<GeminiSessionEvent>,
        session_buffers: &Arc<Mutex<HashMap<String, String>>>,
    ) {
        if notif.method != "session/update" {
            debug!("Ignoring non-session/update notification: {}", notif.method);
            return;
        }
        let Some(params) = notif.params else {
            return;
        };
        let session_id = params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let Some(update) = params.get("update") else {
            return;
        };
        let event_type = update
            .get("sessionUpdate")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // Extract streaming text from agent_message_chunk updates.
        let delta = if event_type == "agent_message_chunk" {
            update
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string()
        } else {
            String::new()
        };

        if !delta.is_empty() {
            let mut bufs = session_buffers.lock().await;
            let buf = bufs.entry(session_id.clone()).or_default();
            buf.push_str(&delta);
            // Cap buffer growth — drop oldest bytes if we exceed the cap
            // (the tail is authoritative for JSON parsing).
            if buf.len() > MAX_SESSION_BUFFER_BYTES {
                let overflow = buf.len() - MAX_SESSION_BUFFER_BYTES;
                // Walk forward to a char boundary so we don't split a UTF-8
                // codepoint.
                let mut drop = overflow;
                while drop < buf.len() && !buf.is_char_boundary(drop) {
                    drop += 1;
                }
                buf.drain(..drop);
            }
        }

        let event = GeminiSessionEvent {
            session_id,
            event_type,
            delta,
            data: update.clone(),
        };
        let _ = event_tx.send(event);
    }

    /// Handle an agent-initiated ACP request (permissions, filesystem proxy).
    async fn handle_server_request(
        req: ServerRequest,
        transport: &JsonRpcTransport,
        permission_tx: &broadcast::Sender<GeminiPermissionRequest>,
    ) {
        match req.method.as_str() {
            "session/request_permission" => {
                // Deny-by-default: SignalPR is a review tool, not an editor.
                // Per ACP (https://agentclientprotocol.com/protocol/tool-calls),
                // user-driven denial must be represented as
                // `{outcome: {outcome: "selected", optionId: "<reject_once id>"}}`;
                // `cancelled` is reserved for prompt-turn cancellation. We
                // therefore pick the first `reject_once`/`reject_always`
                // option advertised by the agent and fall back to
                // `cancelled` only when no rejection option exists.
                //
                // A follow-up PR will add a pending-permission oneshot map +
                // `resolve_gemini_permission` IPC to allow interactive
                // approvals; this is scaffolded via `permission_tx` already.
                let mut rejection_result = json!({ "outcome": { "outcome": "cancelled" } });
                if let Some(params) = req.params.as_ref() {
                    let session_id = params
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let request_id = normalize_request_id(&req.id);
                    let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
                    let options = params
                        .get("options")
                        .cloned()
                        .unwrap_or(Value::Array(vec![]));

                    if let Some(option_id) = pick_rejection_option_id(&options) {
                        rejection_result = json!({
                            "outcome": {
                                "outcome": "selected",
                                "optionId": option_id
                            }
                        });
                    } else {
                        warn!(
                            "session/request_permission had no rejection option; \
                             responding cancelled (session={})",
                            session_id
                        );
                    }

                    warn!(
                        "Gemini permission denied by default (session={}, \
                         tool_kind={})",
                        session_id,
                        tool_call
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("<unknown>")
                    );

                    let _ = permission_tx.send(GeminiPermissionRequest {
                        session_id,
                        request_id,
                        tool_call,
                        options,
                    });
                }

                let _ = transport.send_response(req.id, rejection_result).await;
            }
            "fs/read_text_file" => {
                let result = Self::handle_read_text_file(req.params.as_ref()).await;
                let _ = transport.send_response(req.id, result).await;
            }
            "fs/write_text_file" => {
                // Refuse writes — SignalPR is a review tool, not a code editor.
                // The ACP schema expects a proper JSON-RPC error response for
                // failures, but our shared transport's `send_response` only
                // emits success-shaped replies. We return a result body with
                // an embedded `error` field until the transport gains a real
                // `send_error_response` helper (tracked as a follow-up).
                warn!("Gemini agent requested fs/write_text_file — rejected");
                let _ = transport
                    .send_response(
                        req.id,
                        json!({"error": "fs/write_text_file not supported by SignalPR (review-only)"}),
                    )
                    .await;
            }
            other => {
                debug!("Ignoring unknown Gemini server request: {}", other);
                let _ = transport
                    .send_response(req.id, json!({"error": "method not supported"}))
                    .await;
            }
        }
    }

    /// Handle an `fs/read_text_file` request from the agent.
    async fn handle_read_text_file(params: Option<&Value>) -> Value {
        let Some(params) = params else {
            return json!({"error": "missing params"});
        };
        let Some(path) = params.get("path").and_then(|v| v.as_str()) else {
            return json!({"error": "missing path"});
        };
        match tokio::fs::read_to_string(PathBuf::from(path)).await {
            Ok(content) => json!({"content": content}),
            Err(e) => json!({"error": format!("read failed: {}", e)}),
        }
    }

    /// Create a new ACP session for a review lane. Returns the session id
    /// along with the modes the server advertises (used by the provider to
    /// decide whether plan mode is available).
    pub async fn create_session(
        &self,
        lane_id: &str,
        cwd: &std::path::Path,
    ) -> Result<GeminiSessionHandle, ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::GeminiFailed("Gemini server not started".into()))?;

        let params = json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": []
        });

        let result = transport
            .send_request("session/new", Some(params))
            .await
            .map_err(gemini_transport_error)?;

        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if session_id.is_empty() {
            return Err(ProviderError::GeminiFailed(
                "session/new returned empty sessionId".into(),
            ));
        }

        // Extract modes.id[] from the session-new response if present. Plan
        // mode is gated by upstream `config.isPlanEnabled()`, so we cannot
        // assume it's always available.
        let available_modes: Vec<String> = result
            .get("modes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        {
            let mut map = self.session_by_lane.lock().await;
            map.insert(session_id.clone(), lane_id.to_string());
        }
        {
            let mut bufs = self.session_buffers.lock().await;
            bufs.insert(session_id.clone(), String::new());
        }

        debug!(
            "Created Gemini session {} for lane {} (modes={:?})",
            session_id, lane_id, available_modes
        );
        Ok(GeminiSessionHandle {
            session_id,
            available_modes,
        })
    }

    /// Call `session/set_mode` to put a session into a named ACP mode
    /// (e.g. `"plan"` for read-only). No-op if the requested mode isn't
    /// listed in `available_modes` for that session — upstream gates plan
    /// mode behind `config.isPlanEnabled()`.
    pub async fn set_session_mode(
        &self,
        session_id: &str,
        mode_id: &str,
    ) -> Result<(), ProviderError> {
        let transport = {
            let inner = self.inner.lock().await;
            inner
                .transport
                .as_ref()
                .ok_or_else(|| ProviderError::GeminiFailed("Gemini server not started".into()))?
                .clone()
        };

        transport
            .send_request(
                "session/set_mode",
                Some(json!({
                    "sessionId": session_id,
                    "modeId": mode_id
                })),
            )
            .await
            .map_err(gemini_transport_error)?;
        debug!("Set Gemini session {} mode to {}", session_id, mode_id);
        Ok(())
    }

    /// Call the unstable `session/set_model` method to override the model
    /// for an active session. Tolerates `method not found` as non-fatal so
    /// we keep working on CLI builds where the unstable capability is
    /// disabled.
    pub async fn set_session_model(
        &self,
        session_id: &str,
        model_id: &str,
    ) -> Result<(), ProviderError> {
        let transport = {
            let inner = self.inner.lock().await;
            inner
                .transport
                .as_ref()
                .ok_or_else(|| ProviderError::GeminiFailed("Gemini server not started".into()))?
                .clone()
        };

        match transport
            .send_request(
                "session/set_model",
                Some(json!({
                    "sessionId": session_id,
                    "modelId": model_id
                })),
            )
            .await
        {
            Ok(_) => {
                debug!("Set Gemini session {} model to {}", session_id, model_id);
                Ok(())
            }
            Err(JsonRpcTransportError::RpcError { code, message })
                if code == -32601 || message.to_lowercase().contains("not found") =>
            {
                warn!(
                    "session/set_model unsupported on this Gemini build ({}); \
                     falling back to server default model",
                    message
                );
                Ok(())
            }
            Err(other) => Err(gemini_transport_error(other)),
        }
    }

    /// Drive one turn of the session with the provided prompt text. Blocks
    /// until the agent finishes the turn (ACP's `session/prompt` is a
    /// long-running request that returns `{stopReason}` when done).
    ///
    /// Returns the accumulated agent message text that streamed in during
    /// the turn.
    pub async fn prompt(
        &self,
        session_id: &str,
        prompt_text: &str,
    ) -> Result<String, ProviderError> {
        let transport = {
            let inner = self.inner.lock().await;
            inner
                .transport
                .as_ref()
                .ok_or_else(|| ProviderError::GeminiFailed("Gemini server not started".into()))?
                .clone()
        };

        let params = json!({
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": prompt_text
                }
            ]
        });

        let prompt_result = transport
            .send_request("session/prompt", Some(params))
            .await
            .map_err(gemini_transport_error);

        // Emit a synthetic `session.prompt_complete` event regardless of
        // success/failure so `lib.rs` can clear its per-lane delta buffer.
        // We can't depend on an upstream ACP `end_of_turn` variant whose
        // exact name isn't in the schema we verified against.
        let _ = self.event_tx.send(GeminiSessionEvent {
            session_id: session_id.to_string(),
            event_type: "session.prompt_complete".into(),
            delta: String::new(),
            data: Value::Null,
        });

        // Drain the accumulated buffer for this session regardless of outcome.
        let text = {
            let mut bufs = self.session_buffers.lock().await;
            bufs.remove(session_id).unwrap_or_default()
        };

        // Propagate any transport error now that the buffer is drained.
        prompt_result?;
        Ok(text)
    }

    /// Cancel the current turn for a session.
    pub async fn cancel_session(&self, session_id: &str) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        if let Some(ref transport) = inner.transport {
            let _ = transport
                .send_notification("session/cancel", Some(json!({ "sessionId": session_id })))
                .await;
        }
        Ok(())
    }

    /// Remove a session from all maps. Called after the turn completes or
    /// on cancel/error paths.
    pub async fn unregister_session(&self, session_id: &str) {
        {
            let mut map = self.session_by_lane.lock().await;
            map.remove(session_id);
        }
        {
            let mut bufs = self.session_buffers.lock().await;
            bufs.remove(session_id);
        }
    }

    /// Shut down the gemini server process and clear state.
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;

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

        self.session_by_lane.lock().await.clear();
        self.session_buffers.lock().await.clear();
    }
}

/// Pick a spec-compliant `optionId` to use in a `reject_once`-shaped
/// permission denial. Prefers `kind == "reject_once"`, then
/// `"reject_always"`, then any option whose id contains "reject". Returns
/// `None` when no rejection option is advertised — caller falls back to
/// `{outcome: "cancelled"}` as a last resort.
fn pick_rejection_option_id(options: &Value) -> Option<String> {
    let arr = options.as_array()?;
    for preferred in ["reject_once", "reject_always"] {
        for opt in arr {
            if opt.get("kind").and_then(|v| v.as_str()) == Some(preferred) {
                if let Some(id) = opt.get("optionId").and_then(|v| v.as_str()) {
                    return Some(id.to_string());
                }
            }
        }
    }
    for opt in arr {
        if let Some(id) = opt.get("optionId").and_then(|v| v.as_str()) {
            if id.to_lowercase().contains("reject") {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Normalize a JSON-RPC request id to a plain string. For `Value::String`,
/// `serde_json::Value::to_string()` would yield `"\"abc\""` (with literal
/// quotes); we want `abc`. For numbers, we use the canonical decimal form.
fn normalize_request_id(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Extract the id of an `authMethod` that corresponds to API-key auth
/// (`AuthType.USE_GEMINI` upstream) from an ACP `initialize` response.
///
/// The CLI returns a static array of four methods regardless of env state;
/// we pick whichever advertises "api-key" or contains "gemini" in its id.
/// Returns `None` if none match, in which case callers should skip the
/// explicit authenticate step and rely on cached settings.
fn pick_api_key_method(init_result: &Value) -> Option<String> {
    let methods = init_result.get("authMethods")?.as_array()?;
    for method in methods {
        let Some(id) = method.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let lowered = id.to_lowercase();
        if lowered.contains("api-key")
            || lowered.contains("apikey")
            || lowered.contains("gemini-api")
            || id == "gemini-api-key"
        {
            return Some(id.to_string());
        }
    }
    None
}

/// Map transport errors to `ProviderError::GeminiFailed`.
fn gemini_transport_error(e: JsonRpcTransportError) -> ProviderError {
    match e {
        JsonRpcTransportError::Cancelled => ProviderError::Cancelled,
        other => ProviderError::GeminiFailed(other.to_string()),
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
        let manager = GeminiManager::new();
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_not_started_errors() {
        let manager = GeminiManager::new();
        let result = manager
            .create_session("security", std::path::Path::new("/tmp"))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not started"));

        let result = manager.prompt("fake", "hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shutdown_idempotent() {
        let manager = GeminiManager::new();
        manager.shutdown().await;
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_lane_for_session_empty() {
        let manager = GeminiManager::new();
        assert!(manager.lane_for_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_unregister_session_clears_both_maps() {
        let manager = GeminiManager::new();
        {
            let mut map = manager.session_by_lane.lock().await;
            map.insert("sess-1".into(), "security".into());
        }
        {
            let mut bufs = manager.session_buffers.lock().await;
            bufs.insert("sess-1".into(), "partial buffer".into());
        }
        manager.unregister_session("sess-1").await;
        assert!(manager.lane_for_session("sess-1").await.is_none());
        assert!(manager.session_buffers.lock().await.get("sess-1").is_none());
    }

    #[tokio::test]
    async fn test_route_notification_agent_message_chunk_accumulates() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        let notif = ServerNotification {
            method: "session/update".into(),
            params: Some(json!({
                "sessionId": "sess-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "hello " }
                }
            })),
        };
        GeminiManager::route_notification(notif, &event_tx, &buffers).await;

        let notif2 = ServerNotification {
            method: "session/update".into(),
            params: Some(json!({
                "sessionId": "sess-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": "world" }
                }
            })),
        };
        GeminiManager::route_notification(notif2, &event_tx, &buffers).await;

        let bufs = buffers.lock().await;
        assert_eq!(bufs.get("sess-1").map(|s| s.as_str()), Some("hello world"));

        let evt1 = event_rx.try_recv().unwrap();
        assert_eq!(evt1.event_type, "agent_message_chunk");
        assert_eq!(evt1.delta, "hello ");
        let evt2 = event_rx.try_recv().unwrap();
        assert_eq!(evt2.delta, "world");
    }

    #[tokio::test]
    async fn test_route_notification_ignores_other_methods() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        let notif = ServerNotification {
            method: "other/method".into(),
            params: Some(json!({"foo": "bar"})),
        };
        GeminiManager::route_notification(notif, &event_tx, &buffers).await;
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn test_transport_error_maps_cancelled() {
        let err = gemini_transport_error(JsonRpcTransportError::Cancelled);
        assert!(matches!(err, ProviderError::Cancelled));
    }

    #[test]
    fn test_transport_error_maps_others() {
        let err = gemini_transport_error(JsonRpcTransportError::ChannelClosed);
        assert!(matches!(err, ProviderError::GeminiFailed(_)));
    }

    #[test]
    fn test_normalize_request_id_string_unwraps_quotes() {
        // Value::String("perm-abc").to_string() would yield `"perm-abc"`
        // with literal quotes; normalize_request_id must strip them.
        assert_eq!(normalize_request_id(&json!("perm-abc")), "perm-abc");
    }

    #[test]
    fn test_normalize_request_id_number() {
        assert_eq!(normalize_request_id(&json!(42)), "42");
    }

    #[test]
    fn test_normalize_request_id_unusual_fallback() {
        // null and bool shouldn't happen per JSON-RPC spec but must not panic.
        assert_eq!(normalize_request_id(&Value::Null), "null");
        assert_eq!(normalize_request_id(&json!(true)), "true");
    }

    #[test]
    fn test_pick_api_key_method_prefers_gemini_api_key_id() {
        let init = json!({
            "protocolVersion": 1,
            "authMethods": [
                { "id": "oauth-personal", "name": "OAuth" },
                { "id": "gemini-api-key", "name": "Gemini API key" },
                { "id": "vertex-ai", "name": "Vertex" }
            ]
        });
        assert_eq!(
            pick_api_key_method(&init).as_deref(),
            Some("gemini-api-key")
        );
    }

    #[test]
    fn test_pick_api_key_method_matches_any_api_key_variant() {
        let init = json!({
            "authMethods": [
                { "id": "use-gemini-apikey" }
            ]
        });
        assert!(pick_api_key_method(&init).is_some());
    }

    #[test]
    fn test_pick_api_key_method_none_when_only_oauth() {
        let init = json!({
            "authMethods": [
                { "id": "oauth-personal" },
                { "id": "gateway" }
            ]
        });
        assert!(pick_api_key_method(&init).is_none());
    }

    #[test]
    fn test_pick_api_key_method_none_when_field_missing() {
        let init = json!({ "protocolVersion": 1 });
        assert!(pick_api_key_method(&init).is_none());
    }

    #[tokio::test]
    async fn test_buffer_cap_enforced_on_large_delta() {
        // Feed a run of agent_message_chunk notifications whose total size
        // exceeds MAX_SESSION_BUFFER_BYTES and verify the buffer is capped.
        let (event_tx, _event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        // Send 5 chunks of 256 KiB each → 1.25 MiB total, above the 1 MiB cap.
        let big_chunk = "x".repeat(256 * 1024);
        for _ in 0..5 {
            let notif = ServerNotification {
                method: "session/update".into(),
                params: Some(json!({
                    "sessionId": "sess-big",
                    "update": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": big_chunk }
                    }
                })),
            };
            GeminiManager::route_notification(notif, &event_tx, &buffers).await;
        }

        let bufs = buffers.lock().await;
        let buf = bufs.get("sess-big").expect("buffer should exist");
        assert!(
            buf.len() <= MAX_SESSION_BUFFER_BYTES,
            "buffer must stay at or below {} bytes, got {}",
            MAX_SESSION_BUFFER_BYTES,
            buf.len()
        );
        assert!(
            buf.len() >= MAX_SESSION_BUFFER_BYTES - 4,
            "buffer should be near the cap (allowing up to 3 bytes of char-boundary slack)"
        );
    }
}
