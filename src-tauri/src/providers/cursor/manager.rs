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
use crate::secrets::credentials::{self, ProviderCredentialField};

/// Cap accumulated per-session agent message buffers at 1 MiB. Review outputs
/// should be well under 32 KiB; this is a safety net against a runaway model.
const MAX_SESSION_BUFFER_BYTES: usize = 1 << 20;

/// Startup timeout for the full handshake (initialize + authenticate). Users
/// hit this if the child hangs on a first-run interactive prompt or if the
/// binary is wedged.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Hard cap on bytes returned from `fs/read_text_file`. Large enough for any
/// realistic source file, small enough that a compromised agent can't
/// scrape gigabytes of disk into the transcript.
const MAX_READ_BYTES: usize = 200 * 1024;

/// Default per-request line limit when the agent doesn't specify one.
/// Matches what typical editors cap at in ACP hosts.
const DEFAULT_READ_LIMIT_LINES: usize = 2000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A normalized ACP session update event forwarded to UI listeners.
#[derive(Debug, Clone, Serialize)]
pub struct CursorSessionEvent {
    pub session_id: String,
    pub event_type: String,
    pub delta: String,
    pub data: Value,
}

/// Outcome of creating a new ACP session, including any modes the server
/// advertises.
#[derive(Debug, Clone)]
pub struct CursorSessionHandle {
    pub session_id: String,
    pub available_modes: Vec<String>,
}

/// A permission request raised by the agent via `session/request_permission`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPermissionRequest {
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

/// Manages the lifecycle of a persistent `agent acp` child process and
/// speaks the Agent Client Protocol (ACP) over newline-delimited JSON-RPC
/// 2.0 — same wire format as the Gemini CLI provider.
///
/// Authentication is supplied via the `CURSOR_API_KEY` environment variable,
/// which the Cursor CLI reads on startup. Browser-based `agent login` is
/// also honored if the user has already logged in via the CLI, but SignalPR
/// only health-checks on the env var to keep the non-interactive path
/// deterministic.
pub struct CursorManager {
    inner: Arc<Mutex<Inner>>,
    event_tx: broadcast::Sender<CursorSessionEvent>,
    permission_tx: broadcast::Sender<CursorPermissionRequest>,
    session_buffers: Arc<Mutex<HashMap<String, String>>>,
    session_by_lane: Arc<Mutex<HashMap<String, String>>>,
    /// Per-session canonical cwd; used to sandbox `fs/read_text_file`
    /// requests so a runaway agent can't exfiltrate arbitrary paths.
    session_roots: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl CursorManager {
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
            session_roots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<CursorSessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn subscribe_permissions(&self) -> broadcast::Receiver<CursorPermissionRequest> {
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

    /// Static check for whether `CURSOR_API_KEY` is set. Used by the health
    /// check to fail fast with a clear message.
    pub fn has_auth_env() -> bool {
        credentials::resolve_credential(ProviderCredentialField::CursorApiKey)
            .ok()
            .and_then(|(value, _)| value)
            .is_some()
    }

    /// Ensure the `agent acp` server is running and initialized. Spawns the
    /// process if needed and performs the ACP `initialize` + `authenticate`
    /// handshake under a bounded timeout.
    ///
    /// Binary path defaults to `agent` (Cursor CLI install name); users on
    /// custom paths can override with `CURSOR_CLI_PATH`.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.transport.is_some() {
            return Ok(());
        }

        if !Self::has_auth_env() {
            return Err(ProviderError::CursorFailed(
                "CURSOR_API_KEY not set. Generate a key from the Cursor Dashboard \
                 (Cloud Agents → User API Keys) and export it before launching \
                 SignalPR — see https://cursor.com/docs/cli/reference/authentication"
                    .into(),
            ));
        }

        let cli = std::env::var("CURSOR_CLI_PATH").unwrap_or_else(|_| "agent".to_string());

        // Belt-and-braces: pass `--mode ask` at spawn so the agent is
        // locked into read-only exploration at the policy layer, with the
        // deny-by-default permission gate as the second line of defence.
        // Cursor's `--mode` is a global flag and `acp` is a subcommand
        // (see https://cursor.com/docs/cli/reference/parameters), but the
        // combo isn't explicitly documented together — opt-out via
        // `CURSOR_ACP_DISABLE_MODE_FLAG=1` for builds that reject it.
        let disable_mode_flag = std::env::var("CURSOR_ACP_DISABLE_MODE_FLAG")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let mut cmd = tokio::process::Command::new(&cli);
        if !disable_mode_flag {
            cmd.arg("--mode").arg("ask");
        }
        cmd.arg("acp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        if let Some(key) = credentials::resolve_credential(ProviderCredentialField::CursorApiKey)
            .ok()
            .and_then(|(value, _)| value)
        {
            cmd.env("CURSOR_API_KEY", key);
        }

        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::CursorFailed(format!(
                "Failed to spawn `{} acp`: {}. Install Cursor CLI via \
                 `curl https://cursor.com/install -fsS | bash`.",
                cli, e
            ))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::CursorFailed("Failed to capture cursor stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::CursorFailed("Failed to capture cursor stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::CursorFailed("Failed to capture cursor stderr".into()))?;

        let child_cancel = CancellationToken::new();

        // Drain child stderr into tracing::debug so the pipe never blocks on
        // backpressure. Cursor CLI may emit non-JSON diagnostics even in ACP
        // mode.
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
                                    let trimmed = if l.len() > 1024 {
                                        &l[..1024]
                                    } else {
                                        l.as_str()
                                    };
                                    debug!("cursor stderr: {}", trimmed);
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    debug!("cursor stderr read error: {}", e);
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
        //
        // Protocol references:
        // - https://agentclientprotocol.com/protocol/initialization.md
        // - https://cursor.com/docs/cli/acp
        let handshake = async {
            let init_params = json!({
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "signalpr",
                    "title": "SignalPR",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "clientCapabilities": {
                    "fs": {
                        "readTextFile": true,
                        "writeTextFile": false
                    },
                    "terminal": false
                }
            });

            let init_result = transport
                .send_request("initialize", Some(init_params))
                .await
                .map_err(cursor_transport_error)?;

            let protocol_version = init_result
                .get("protocolVersion")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);

            info!(
                "Cursor ACP server initialized (protocolVersion={})",
                protocol_version
            );

            // ACP authentication is a JSON-RPC `authenticate` method call
            // with `{methodId: "..."}` params, NOT a method literally named
            // after the auth scheme. Prefer an advertised authMethod whose
            // id matches Cursor's documented `cursor_login`; fall back to
            // sending `cursor_login` directly if the server doesn't expose
            // `authMethods[]` on initialize (as is the case for some
            // pinned Cursor CLI builds).
            let method_id =
                pick_cursor_auth_method(&init_result).unwrap_or_else(|| "cursor_login".to_string());

            debug!("Calling ACP authenticate with methodId={}", method_id);
            match transport
                .send_request("authenticate", Some(json!({ "methodId": method_id })))
                .await
            {
                Ok(_) => debug!("Cursor authenticate succeeded"),
                Err(JsonRpcTransportError::RpcError { code, message })
                    if code == -32601 || message.to_lowercase().contains("not found") =>
                {
                    // Some Cursor builds skip authenticate entirely when
                    // CURSOR_API_KEY is already present in the child env.
                    // Tolerate method_not_found; session/new will surface
                    // the real error if auth is actually broken.
                    debug!(
                        "authenticate method not advertised on this build; \
                         relying on env auth"
                    );
                }
                Err(e) => {
                    warn!(
                        "Cursor authenticate failed ({}); continuing — \
                         session/new will surface the real error if auth \
                         is broken.",
                        e
                    );
                }
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
                return Err(ProviderError::CursorFailed(format!(
                    "Cursor startup timed out after {}s — check `agent status` \
                     and CURSOR_API_KEY.",
                    STARTUP_TIMEOUT.as_secs()
                )));
            }
        }

        // Spawn notification router (streaming deltas, cursor/* extensions).
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

        // Spawn server-request handler (permissions, filesystem proxy,
        // cursor/* blocking extensions).
        {
            let transport_clone = transport.clone();
            let permission_tx = self.permission_tx.clone();
            let session_roots = Arc::clone(&self.session_roots);
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
                                        &session_roots,
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

    /// Route an inbound notification. Handles `session/update` (streaming
    /// deltas) and silently drops the fire-and-forget `cursor/*` extension
    /// notifications (`cursor/update_todos`, `cursor/task`,
    /// `cursor/generate_image`).
    async fn route_notification(
        notif: ServerNotification,
        event_tx: &broadcast::Sender<CursorSessionEvent>,
        session_buffers: &Arc<Mutex<HashMap<String, String>>>,
    ) {
        if notif.method.starts_with("cursor/") {
            debug!("Cursor extension notification ignored: {}", notif.method);
            return;
        }
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
            if buf.len() > MAX_SESSION_BUFFER_BYTES {
                let overflow = buf.len() - MAX_SESSION_BUFFER_BYTES;
                let mut drop = overflow;
                while drop < buf.len() && !buf.is_char_boundary(drop) {
                    drop += 1;
                }
                buf.drain(..drop);
            }
        }

        let event = CursorSessionEvent {
            session_id,
            event_type,
            delta,
            data: update.clone(),
        };
        let _ = event_tx.send(event);
    }

    /// Handle an agent-initiated ACP request.
    ///
    /// Methods handled:
    /// - `session/request_permission`: deny-by-default by selecting a
    ///   `reject_once` option (spec-compliant denial — `cancelled` is
    ///   reserved for prompt-turn cancellation per
    ///   <https://agentclientprotocol.com/protocol/tool-calls>)
    /// - `fs/read_text_file`: proxy to local disk, sandboxed to the
    ///   session's cwd with a 200 KiB cap
    /// - `fs/write_text_file`: refuse
    /// - `cursor/ask_question`: auto-cancel (no UI to present options yet)
    /// - `cursor/create_plan`: auto-accept (plan display is not blocking
    ///   for a read-only review session)
    /// - anything else: respond with "method not supported"
    async fn handle_server_request(
        req: ServerRequest,
        transport: &JsonRpcTransport,
        permission_tx: &broadcast::Sender<CursorPermissionRequest>,
        session_roots: &Arc<Mutex<HashMap<String, PathBuf>>>,
    ) {
        match req.method.as_str() {
            "session/request_permission" => {
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
                        "Cursor permission denied by default (session={}, \
                         tool_kind={})",
                        session_id,
                        tool_call
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("<unknown>")
                    );

                    let _ = permission_tx.send(CursorPermissionRequest {
                        session_id,
                        request_id,
                        tool_call,
                        options,
                    });
                }

                let _ = transport.send_response(req.id, rejection_result).await;
            }
            "fs/read_text_file" => {
                let result = Self::handle_read_text_file(req.params.as_ref(), session_roots).await;
                let _ = transport.send_response(req.id, result).await;
            }
            "fs/write_text_file" => {
                warn!("Cursor agent requested fs/write_text_file — rejected");
                let _ = transport
                    .send_response(
                        req.id,
                        json!({"error": "fs/write_text_file not supported by SignalPR (review-only)"}),
                    )
                    .await;
            }
            "cursor/ask_question" => {
                debug!("cursor/ask_question auto-cancelled");
                let _ = transport
                    .send_response(req.id, json!({ "outcome": { "outcome": "cancelled" } }))
                    .await;
            }
            "cursor/create_plan" => {
                debug!("cursor/create_plan auto-accepted");
                let _ = transport
                    .send_response(req.id, json!({ "outcome": { "outcome": "accepted" } }))
                    .await;
            }
            other => {
                debug!("Ignoring unknown Cursor server request: {}", other);
                let _ = transport
                    .send_response(req.id, json!({"error": "method not supported"}))
                    .await;
            }
        }
    }

    /// Handle an `fs/read_text_file` request from the agent.
    ///
    /// ACP schema (<https://agentclientprotocol.com/protocol/file-system.md>):
    /// `{sessionId, path, line?, limit?}` — path must be absolute. The
    /// spec doesn't mandate a cwd restriction, but SignalPR is a review
    /// tool with no legitimate reason to read outside the session root, so
    /// we canonicalise and prefix-check as a privacy default.
    async fn handle_read_text_file(
        params: Option<&Value>,
        session_roots: &Arc<Mutex<HashMap<String, PathBuf>>>,
    ) -> Value {
        let Some(params) = params else {
            return json!({"error": "missing params"});
        };
        let Some(session_id) = params.get("sessionId").and_then(|v| v.as_str()) else {
            return json!({"error": "missing sessionId"});
        };
        let Some(path_str) = params.get("path").and_then(|v| v.as_str()) else {
            return json!({"error": "missing path"});
        };

        let requested = PathBuf::from(path_str);
        if !requested.is_absolute() {
            return json!({"error": "path must be absolute"});
        }

        let root = {
            let map = session_roots.lock().await;
            match map.get(session_id) {
                Some(p) => p.clone(),
                None => return json!({"error": "unknown session"}),
            }
        };

        // Canonicalise both sides. If the requested file doesn't exist
        // yet, fall back to canonicalising its parent directory so we
        // still enforce the prefix check. `canonicalize` on a missing
        // path returns NotFound; we treat that as a read error rather
        // than a sandbox escape.
        let canonical_requested = match tokio::fs::canonicalize(&requested).await {
            Ok(p) => p,
            Err(e) => {
                return json!({"error": format!("read failed: {}", e)});
            }
        };
        let canonical_root = match tokio::fs::canonicalize(&root).await {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    "Session root {} is not canonicalisable: {} — rejecting read",
                    root.display(),
                    e
                );
                return json!({"error": "session root unavailable"});
            }
        };
        if !canonical_requested.starts_with(&canonical_root) {
            warn!(
                "Cursor fs/read_text_file rejected: path escapes session root \
                 (root={}, requested={})",
                canonical_root.display(),
                canonical_requested.display()
            );
            return json!({"error": "path escapes session root"});
        }

        let raw = match tokio::fs::read_to_string(&canonical_requested).await {
            Ok(content) => content,
            Err(e) => return json!({"error": format!("read failed: {}", e)}),
        };

        let line = params
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_READ_LIMIT_LINES);

        let sliced = slice_lines(&raw, line, limit);
        let capped = cap_utf8_bytes(&sliced, MAX_READ_BYTES);
        json!({"content": capped})
    }

    /// Create a new ACP session for a review lane.
    pub async fn create_session(
        &self,
        lane_id: &str,
        cwd: &std::path::Path,
    ) -> Result<CursorSessionHandle, ProviderError> {
        let inner = self.inner.lock().await;
        let transport = inner
            .transport
            .as_ref()
            .ok_or_else(|| ProviderError::CursorFailed("Cursor server not started".into()))?;

        let params = json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": []
        });

        let result = transport
            .send_request("session/new", Some(params))
            .await
            .map_err(cursor_transport_error)?;

        let session_id = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if session_id.is_empty() {
            return Err(ProviderError::CursorFailed(
                "session/new returned empty sessionId".into(),
            ));
        }

        let available_modes = extract_available_modes(&result);

        {
            let mut map = self.session_by_lane.lock().await;
            map.insert(session_id.clone(), lane_id.to_string());
        }
        {
            let mut bufs = self.session_buffers.lock().await;
            bufs.insert(session_id.clone(), String::new());
        }
        {
            let mut roots = self.session_roots.lock().await;
            roots.insert(session_id.clone(), cwd.to_path_buf());
        }

        debug!(
            "Created Cursor session {} for lane {} (modes={:?})",
            session_id, lane_id, available_modes
        );
        Ok(CursorSessionHandle {
            session_id,
            available_modes,
        })
    }

    /// Call `session/set_mode` to switch a session into a named ACP mode
    /// (e.g. `"ask"` for read-only exploration). Tolerates the method
    /// being absent on builds that don't wire it up.
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
                .ok_or_else(|| ProviderError::CursorFailed("Cursor server not started".into()))?
                .clone()
        };

        match transport
            .send_request(
                "session/set_mode",
                Some(json!({
                    "sessionId": session_id,
                    "modeId": mode_id
                })),
            )
            .await
        {
            Ok(_) => {
                debug!("Set Cursor session {} mode to {}", session_id, mode_id);
                Ok(())
            }
            Err(JsonRpcTransportError::RpcError { code, message })
                if code == -32601 || message.to_lowercase().contains("not found") =>
            {
                debug!(
                    "session/set_mode unsupported on this Cursor build ({}); \
                     relying on deny-by-default permission handler",
                    message
                );
                Ok(())
            }
            Err(other) => Err(cursor_transport_error(other)),
        }
    }

    /// Call `session/set_model` to override the model for an active
    /// session. Tolerates `method not found` as non-fatal so we keep
    /// working on CLI builds where the unstable capability is disabled.
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
                .ok_or_else(|| ProviderError::CursorFailed("Cursor server not started".into()))?
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
                debug!("Set Cursor session {} model to {}", session_id, model_id);
                Ok(())
            }
            Err(JsonRpcTransportError::RpcError { code, message })
                if code == -32601 || message.to_lowercase().contains("not found") =>
            {
                warn!(
                    "session/set_model unsupported on this Cursor build ({}); \
                     falling back to server default model",
                    message
                );
                Ok(())
            }
            Err(other) => Err(cursor_transport_error(other)),
        }
    }

    /// Drive one turn of the session with the provided prompt text. Blocks
    /// until the agent finishes the turn. Returns the accumulated agent
    /// message text streamed in during the turn.
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
                .ok_or_else(|| ProviderError::CursorFailed("Cursor server not started".into()))?
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
            .map_err(cursor_transport_error);

        // Synthetic `session.prompt_complete` event lets `lib.rs` clear its
        // per-lane delta buffer regardless of success or failure.
        let _ = self.event_tx.send(CursorSessionEvent {
            session_id: session_id.to_string(),
            event_type: "session.prompt_complete".into(),
            delta: String::new(),
            data: Value::Null,
        });

        let text = {
            let mut bufs = self.session_buffers.lock().await;
            bufs.remove(session_id).unwrap_or_default()
        };

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
        {
            let mut roots = self.session_roots.lock().await;
            roots.remove(session_id);
        }
    }

    /// Shut down the cursor server process and clear state.
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
        self.session_roots.lock().await.clear();
    }
}

/// Pick the most appropriate `authMethod` id from an ACP `initialize`
/// response. Prefers Cursor's documented `cursor_login` id (any casing /
/// dash-vs-underscore variant), falling back to the first advertised
/// method. Returns `None` if `authMethods[]` is absent entirely, in which
/// case callers should default to `cursor_login` per Cursor's docs.
fn pick_cursor_auth_method(init_result: &Value) -> Option<String> {
    let methods = init_result.get("authMethods")?.as_array()?;
    for method in methods {
        let Some(id) = method.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let lowered = id.to_lowercase().replace('-', "_");
        if lowered == "cursor_login"
            || lowered.contains("cursor_login")
            || lowered.contains("cursor_api_key")
        {
            return Some(id.to_string());
        }
    }
    // Fall back to whatever first method exists — keeps us working on
    // forks/enterprise builds that rename the id.
    methods
        .iter()
        .find_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
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

/// Parse the `modes` field from a `session/new` response, tolerating
/// both the modern ACP shape `{currentModeId, availableModes: [...]}`
/// (per <https://agentclientprotocol.com/protocol/session-modes.md>)
/// and the pre-standard flat array `[{id, name}]` that some builds still
/// return. Missing or unrecognised shapes yield an empty vec.
fn extract_available_modes(result: &Value) -> Vec<String> {
    let Some(modes) = result.get("modes") else {
        return Vec::new();
    };
    if let Some(arr) = modes.get("availableModes").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }
    if let Some(arr) = modes.as_array() {
        return arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }
    Vec::new()
}

/// Slice a file's contents by `line` (1-based start) and `limit` (max
/// lines returned). If `line` is `None`, starts from the first line.
/// Missing `limit` defaults to `DEFAULT_READ_LIMIT_LINES`.
fn slice_lines(raw: &str, line: Option<usize>, limit: usize) -> String {
    let start = line.unwrap_or(1).saturating_sub(1);
    let mut out = String::new();
    for (i, line_text) in raw.split_inclusive('\n').enumerate() {
        if i < start {
            continue;
        }
        if i >= start + limit {
            break;
        }
        out.push_str(line_text);
    }
    out
}

/// Truncate a string to at most `max_bytes`, respecting UTF-8 char
/// boundaries and appending a visible marker so the agent can tell its
/// read was cut short. A no-op if the input fits.
fn cap_utf8_bytes(s: &str, max_bytes: usize) -> String {
    const MARKER: &str = "\n…(truncated by SignalPR)";
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Reserve space for the marker so the final output stays under cap.
    let target = max_bytes.saturating_sub(MARKER.len());
    let mut cut = target.min(s.len());
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + MARKER.len());
    out.push_str(&s[..cut]);
    out.push_str(MARKER);
    out
}

/// Normalize a JSON-RPC request id to a plain string.
fn normalize_request_id(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Map transport errors to `ProviderError::CursorFailed`.
fn cursor_transport_error(e: JsonRpcTransportError) -> ProviderError {
    match e {
        JsonRpcTransportError::Cancelled => ProviderError::Cancelled,
        other => ProviderError::CursorFailed(other.to_string()),
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
        let manager = CursorManager::new();
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_not_started_errors() {
        let manager = CursorManager::new();
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
        let manager = CursorManager::new();
        manager.shutdown().await;
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_lane_for_session_empty() {
        let manager = CursorManager::new();
        assert!(manager.lane_for_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_unregister_session_clears_both_maps() {
        let manager = CursorManager::new();
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
        CursorManager::route_notification(notif, &event_tx, &buffers).await;

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
        CursorManager::route_notification(notif2, &event_tx, &buffers).await;

        let bufs = buffers.lock().await;
        assert_eq!(bufs.get("sess-1").map(|s| s.as_str()), Some("hello world"));

        let evt1 = event_rx.try_recv().unwrap();
        assert_eq!(evt1.event_type, "agent_message_chunk");
        assert_eq!(evt1.delta, "hello ");
        let evt2 = event_rx.try_recv().unwrap();
        assert_eq!(evt2.delta, "world");
    }

    #[tokio::test]
    async fn test_route_notification_drops_cursor_extension_notifications() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        for method in [
            "cursor/update_todos",
            "cursor/task",
            "cursor/generate_image",
        ] {
            let notif = ServerNotification {
                method: method.into(),
                params: Some(json!({"foo": "bar"})),
            };
            CursorManager::route_notification(notif, &event_tx, &buffers).await;
        }
        assert!(event_rx.try_recv().is_err());
        assert!(buffers.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_route_notification_ignores_other_methods() {
        let (event_tx, mut event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

        let notif = ServerNotification {
            method: "other/method".into(),
            params: Some(json!({"foo": "bar"})),
        };
        CursorManager::route_notification(notif, &event_tx, &buffers).await;
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn test_transport_error_maps_cancelled() {
        let err = cursor_transport_error(JsonRpcTransportError::Cancelled);
        assert!(matches!(err, ProviderError::Cancelled));
    }

    #[test]
    fn test_transport_error_maps_others() {
        let err = cursor_transport_error(JsonRpcTransportError::ChannelClosed);
        assert!(matches!(err, ProviderError::CursorFailed(_)));
    }

    #[test]
    fn test_normalize_request_id_string_unwraps_quotes() {
        assert_eq!(normalize_request_id(&json!("perm-abc")), "perm-abc");
    }

    #[test]
    fn test_normalize_request_id_number() {
        assert_eq!(normalize_request_id(&json!(42)), "42");
    }

    #[test]
    fn test_has_auth_env_rejects_empty_string() {
        let prior = std::env::var("CURSOR_API_KEY").ok();
        std::env::set_var("CURSOR_API_KEY", "");
        assert!(!CursorManager::has_auth_env());
        std::env::set_var("CURSOR_API_KEY", "test-key");
        assert!(CursorManager::has_auth_env());
        match prior {
            Some(v) => std::env::set_var("CURSOR_API_KEY", v),
            None => std::env::remove_var("CURSOR_API_KEY"),
        }
    }

    #[tokio::test]
    async fn test_buffer_cap_enforced_on_large_delta() {
        let (event_tx, _event_rx) = broadcast::channel(16);
        let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

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
            CursorManager::route_notification(notif, &event_tx, &buffers).await;
        }

        let bufs = buffers.lock().await;
        let buf = bufs.get("sess-big").expect("buffer should exist");
        assert!(
            buf.len() <= MAX_SESSION_BUFFER_BYTES,
            "buffer must stay at or below {} bytes, got {}",
            MAX_SESSION_BUFFER_BYTES,
            buf.len()
        );
    }

    // -- pick_cursor_auth_method -------------------------------------------

    #[test]
    fn test_pick_cursor_auth_method_prefers_cursor_login() {
        let init = json!({
            "protocolVersion": 1,
            "authMethods": [
                { "id": "oauth", "name": "OAuth" },
                { "id": "cursor_login", "name": "Cursor Login" }
            ]
        });
        assert_eq!(
            pick_cursor_auth_method(&init).as_deref(),
            Some("cursor_login")
        );
    }

    #[test]
    fn test_pick_cursor_auth_method_matches_dash_variant() {
        let init = json!({
            "authMethods": [
                { "id": "cursor-login", "name": "Cursor Login" }
            ]
        });
        assert_eq!(
            pick_cursor_auth_method(&init).as_deref(),
            Some("cursor-login")
        );
    }

    #[test]
    fn test_pick_cursor_auth_method_falls_back_to_first_method() {
        let init = json!({
            "authMethods": [
                { "id": "enterprise-sso", "name": "Enterprise SSO" }
            ]
        });
        assert_eq!(
            pick_cursor_auth_method(&init).as_deref(),
            Some("enterprise-sso")
        );
    }

    #[test]
    fn test_pick_cursor_auth_method_none_when_field_missing() {
        let init = json!({ "protocolVersion": 1 });
        assert!(pick_cursor_auth_method(&init).is_none());
    }

    // -- pick_rejection_option_id ------------------------------------------

    #[test]
    fn test_pick_rejection_option_id_prefers_reject_once() {
        let opts = json!([
            { "optionId": "allow-once",  "kind": "allow_once" },
            { "optionId": "reject-always", "kind": "reject_always" },
            { "optionId": "reject-once", "kind": "reject_once" }
        ]);
        assert_eq!(
            pick_rejection_option_id(&opts).as_deref(),
            Some("reject-once")
        );
    }

    #[test]
    fn test_pick_rejection_option_id_falls_back_to_reject_always() {
        let opts = json!([
            { "optionId": "allow-once", "kind": "allow_once" },
            { "optionId": "reject-always", "kind": "reject_always" }
        ]);
        assert_eq!(
            pick_rejection_option_id(&opts).as_deref(),
            Some("reject-always")
        );
    }

    #[test]
    fn test_pick_rejection_option_id_id_substring_fallback() {
        // Some builds omit `kind` but still name the option something
        // like "custom-reject". Match on the id as a final fallback.
        let opts = json!([
            { "optionId": "custom-reject-request" }
        ]);
        assert_eq!(
            pick_rejection_option_id(&opts).as_deref(),
            Some("custom-reject-request")
        );
    }

    #[test]
    fn test_pick_rejection_option_id_none_when_only_allow() {
        let opts = json!([
            { "optionId": "allow-once",  "kind": "allow_once" },
            { "optionId": "allow-always", "kind": "allow_always" }
        ]);
        assert!(pick_rejection_option_id(&opts).is_none());
    }

    #[test]
    fn test_pick_rejection_option_id_none_when_empty() {
        let opts = json!([]);
        assert!(pick_rejection_option_id(&opts).is_none());
    }

    // -- extract_available_modes -------------------------------------------

    #[test]
    fn test_extract_available_modes_modern_object_shape() {
        let result = json!({
            "sessionId": "sess",
            "modes": {
                "currentModeId": "ask",
                "availableModes": [
                    { "id": "ask",  "name": "Ask",  "description": "read-only" },
                    { "id": "code", "name": "Code", "description": "editable" }
                ]
            }
        });
        let modes = extract_available_modes(&result);
        assert_eq!(modes, vec!["ask".to_string(), "code".to_string()]);
    }

    #[test]
    fn test_extract_available_modes_legacy_array_shape() {
        let result = json!({
            "sessionId": "sess",
            "modes": [
                { "id": "plan",  "name": "Plan" },
                { "id": "agent", "name": "Agent" }
            ]
        });
        let modes = extract_available_modes(&result);
        assert_eq!(modes, vec!["plan".to_string(), "agent".to_string()]);
    }

    #[test]
    fn test_extract_available_modes_missing_returns_empty() {
        let result = json!({ "sessionId": "sess" });
        assert!(extract_available_modes(&result).is_empty());
    }

    #[test]
    fn test_extract_available_modes_unknown_shape_returns_empty() {
        let result = json!({
            "sessionId": "sess",
            "modes": "plan"
        });
        assert!(extract_available_modes(&result).is_empty());
    }

    // -- slice_lines / cap_utf8_bytes --------------------------------------

    #[test]
    fn test_slice_lines_full_file_when_line_none() {
        let raw = "a\nb\nc\n";
        assert_eq!(slice_lines(raw, None, 100), "a\nb\nc\n");
    }

    #[test]
    fn test_slice_lines_honours_start_and_limit() {
        let raw = "l1\nl2\nl3\nl4\nl5\n";
        assert_eq!(slice_lines(raw, Some(2), 2), "l2\nl3\n");
    }

    #[test]
    fn test_slice_lines_start_past_end_returns_empty() {
        let raw = "only\n";
        assert_eq!(slice_lines(raw, Some(5), 10), "");
    }

    #[test]
    fn test_cap_utf8_bytes_under_cap_unchanged() {
        let s = "hello world";
        assert_eq!(cap_utf8_bytes(s, 100), s);
    }

    #[test]
    fn test_cap_utf8_bytes_ascii_truncates_with_marker() {
        let s = "a".repeat(500);
        let out = cap_utf8_bytes(&s, 100);
        assert!(out.len() <= 100);
        assert!(out.contains("(truncated by SignalPR)"));
    }

    #[test]
    fn test_cap_utf8_bytes_cjk_no_panic() {
        let s = "日".repeat(1000);
        let out = cap_utf8_bytes(&s, 100);
        assert!(out.len() <= 100);
        assert!(out.contains("(truncated by SignalPR)"));
        // No partial codepoints — the slice we keep must be pure `日`.
        let kept = out.replace("\n…(truncated by SignalPR)", "");
        assert!(kept.chars().all(|c| c == '日'));
    }

    // -- fs/read_text_file hardening ---------------------------------------

    #[tokio::test]
    async fn test_read_text_file_requires_session_id() {
        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let result =
            CursorManager::handle_read_text_file(Some(&json!({ "path": "/tmp/foo" })), &roots)
                .await;
        assert_eq!(
            result.get("error").and_then(|v| v.as_str()),
            Some("missing sessionId")
        );
    }

    #[tokio::test]
    async fn test_read_text_file_rejects_relative_path() {
        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        roots
            .lock()
            .await
            .insert("s1".into(), PathBuf::from("/tmp"));
        let result = CursorManager::handle_read_text_file(
            Some(&json!({ "sessionId": "s1", "path": "relative/foo" })),
            &roots,
        )
        .await;
        assert_eq!(
            result.get("error").and_then(|v| v.as_str()),
            Some("path must be absolute")
        );
    }

    #[tokio::test]
    async fn test_read_text_file_rejects_unknown_session() {
        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        let result = CursorManager::handle_read_text_file(
            Some(&json!({ "sessionId": "ghost", "path": "/tmp/foo" })),
            &roots,
        )
        .await;
        assert_eq!(
            result.get("error").and_then(|v| v.as_str()),
            Some("unknown session")
        );
    }

    #[tokio::test]
    async fn test_read_text_file_rejects_path_outside_cwd() {
        // Create a real tempdir as the session root, then ask for
        // /etc/passwd. Canonicalised prefix check must reject.
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        roots.lock().await.insert("s1".into(), root.clone());

        let escape_target = if cfg!(target_os = "windows") {
            "C:\\Windows\\System32\\drivers\\etc\\hosts"
        } else {
            "/etc/hosts"
        };
        let result = CursorManager::handle_read_text_file(
            Some(&json!({ "sessionId": "s1", "path": escape_target })),
            &roots,
        )
        .await;
        let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            err.contains("escapes session root") || err.contains("read failed"),
            "expected escape rejection, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_read_text_file_honours_line_and_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let file = root.join("sample.txt");
        std::fs::write(&file, "l1\nl2\nl3\nl4\nl5\n").unwrap();

        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        roots.lock().await.insert("s1".into(), root);

        let result = CursorManager::handle_read_text_file(
            Some(&json!({
                "sessionId": "s1",
                "path": file.to_string_lossy(),
                "line": 2,
                "limit": 2
            })),
            &roots,
        )
        .await;
        assert_eq!(
            result.get("content").and_then(|v| v.as_str()),
            Some("l2\nl3\n")
        );
    }

    #[tokio::test]
    async fn test_read_text_file_caps_large_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let file = root.join("big.txt");
        // Write well over the 200 KiB cap.
        let body = "x".repeat(MAX_READ_BYTES + 50_000);
        std::fs::write(&file, &body).unwrap();

        let roots: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
        roots.lock().await.insert("s1".into(), root);

        let result = CursorManager::handle_read_text_file(
            Some(&json!({
                "sessionId": "s1",
                "path": file.to_string_lossy(),
            })),
            &roots,
        )
        .await;
        let content = result
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(content.len() <= MAX_READ_BYTES + 32);
        assert!(content.contains("(truncated by SignalPR)"));
    }
}
