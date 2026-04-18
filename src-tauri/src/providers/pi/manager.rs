use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::errors::ProviderError;

/// Cap accumulated per-session agent message buffers at 1 MiB. Review outputs
/// should be well under 32 KiB; this is a safety net against a runaway model.
const MAX_SESSION_BUFFER_BYTES: usize = 1 << 20;

/// Startup timeout — PI spawns a Node.js process which may take a few seconds
/// on first run.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Protocol types (PI RPC — LF-delimited JSONL)
//
// Upstream reference: packages/coding-agent/src/modes/rpc/rpc-types.ts
//
// Commands use `type` as the discriminator (NOT `command`). Each command type
// carries its own fields. An optional `id` enables response correlation.
//
// Events also use `type`; streaming text arrives as `message_update` with
// deltas nested at `assistantMessageEvent.delta` (only when
// `assistantMessageEvent.type == "text_delta"`). The agent run completes
// with an `agent_end` event (NOT `turn_end` — that's per-turn).
// ---------------------------------------------------------------------------

/// Outbound message to the PI RPC process.
#[derive(Debug, Clone, Serialize)]
struct PiRpcMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(flatten)]
    fields: Value,
}

/// Inbound event/response from the PI RPC process.
#[derive(Debug, Clone, Deserialize)]
struct PiEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(flatten)]
    data: Value,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A normalized PI session event forwarded to UI listeners.
#[derive(Debug, Clone, Serialize)]
pub struct PiSessionEvent {
    pub lane_id: String,
    pub event_type: String,
    pub delta: String,
    pub data: Value,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Alias to tame `clippy::type_complexity` on the nested turn-completion channel.
type TurnCompleteSender = Arc<Mutex<Option<oneshot::Sender<Result<(), ProviderError>>>>>;

struct Inner {
    writer_tx: Option<mpsc::Sender<PiRpcMessage>>,
    child: Option<tokio::process::Child>,
    child_cancel: Option<CancellationToken>,
}

/// Manages the lifecycle of a persistent `pi --mode rpc` child process and
/// speaks the PI RPC protocol (LF-delimited JSONL commands/events).
///
/// PI is single-session per process — only one review can run at a time.
/// The provider acquires `session_guard` for the entire lane lifecycle
/// (new_session → prompt → wait agent_end).
pub struct PiManager {
    inner: Arc<Mutex<Inner>>,
    event_tx: broadcast::Sender<PiSessionEvent>,
    session_buffers: Arc<Mutex<HashMap<String, String>>>,
    active_lane: Arc<Mutex<Option<String>>>,
    /// Fires when the current agent run completes (`agent_end` event).
    turn_complete_tx: TurnCompleteSender,
    /// Serializes the full review lane lifecycle — PI is single-session.
    session_guard: Arc<tokio::sync::Mutex<()>>,
}

impl PiManager {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                writer_tx: None,
                child: None,
                child_cancel: None,
            })),
            event_tx,
            session_buffers: Arc::new(Mutex::new(HashMap::new())),
            active_lane: Arc::new(Mutex::new(None)),
            turn_complete_tx: Arc::new(Mutex::new(None)),
            session_guard: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<PiSessionEvent> {
        self.event_tx.subscribe()
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        if let Ok(inner) = self.inner.try_lock() {
            inner.writer_tx.is_some()
        } else {
            false
        }
    }

    /// Look up the lane_id for the currently active session.
    #[allow(dead_code)]
    pub async fn current_lane(&self) -> Option<String> {
        self.active_lane.lock().await.clone()
    }

    /// Acquire exclusive session access. The provider holds this for the
    /// entire new_session → prompt → agent_end lifecycle so lanes don't
    /// interleave on the single-session PI process.
    pub async fn acquire_session_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.session_guard.lock().await
    }

    /// Static check for whether the `pi` CLI binary is available on PATH.
    pub fn has_pi_binary() -> bool {
        let cli = std::env::var("PI_CLI_PATH").unwrap_or_else(|_| "pi".to_string());
        std::process::Command::new(&cli)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Ensure the `pi --mode rpc` process is running. Spawns the process if
    /// needed and verifies it stays alive during startup.
    ///
    /// Security: PI is spawned with `--no-session --no-tools --no-extensions
    /// --no-skills` so it cannot write files, execute commands, persist
    /// sessions, or load extensions during a review.
    pub async fn ensure_started(&self) -> Result<(), ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.writer_tx.is_some() {
            return Ok(());
        }

        let cli = std::env::var("PI_CLI_PATH").unwrap_or_else(|_| "pi".to_string());

        let mut child = tokio::process::Command::new(&cli)
            .args([
                "--mode",
                "rpc",
                "--no-session",    // Don't persist sessions across runs
                "--no-tools",      // Disable all tool execution (read, write, edit, bash)
                "--no-extensions", // Don't load any PI extensions
                "--no-skills",     // Don't load PI skills
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                ProviderError::PiFailed(format!(
                    "Failed to spawn `{} --mode rpc`: {}. Install with \
                     `npm i -g @mariozechner/pi-coding-agent`.",
                    cli, e
                ))
            })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::PiFailed("Failed to capture pi stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::PiFailed("Failed to capture pi stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::PiFailed("Failed to capture pi stderr".into()))?;

        let child_cancel = CancellationToken::new();

        // Drain child stderr into tracing::debug so the pipe never blocks.
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
                                    debug!("pi stderr: {}", trimmed);
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    debug!("pi stderr read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
            });
        }

        // Writer task: serializes PiRpcMessage to JSON + newline, writes to stdin.
        let (writer_tx, mut writer_rx) = mpsc::channel::<PiRpcMessage>(128);
        {
            let cancel = child_cancel.clone();
            let mut stdin = stdin;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        msg = writer_rx.recv() => {
                            match msg {
                                Some(m) => {
                                    let mut line = match serde_json::to_string(&m) {
                                        Ok(s) => s,
                                        Err(e) => {
                                            warn!("pi: failed to serialize message: {}", e);
                                            continue;
                                        }
                                    };
                                    line.push('\n');
                                    if let Err(e) = stdin.write_all(line.as_bytes()).await {
                                        warn!("pi: stdin write error: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        warn!("pi: stdin flush error: {}", e);
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                }
            });
        }

        // Reader task: reads JSONL events from stdout and routes them.
        {
            let event_tx = self.event_tx.clone();
            let session_buffers = Arc::clone(&self.session_buffers);
            let active_lane = Arc::clone(&self.active_lane);
            let turn_complete_tx = Arc::clone(&self.turn_complete_tx);
            let cancel = child_cancel.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        line = reader.next_line() => {
                            match line {
                                Ok(Some(raw)) => {
                                    let raw = raw.trim().to_string();
                                    if raw.is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<PiEvent>(&raw) {
                                        Ok(event) => {
                                            Self::route_event(
                                                event,
                                                &event_tx,
                                                &session_buffers,
                                                &active_lane,
                                                &turn_complete_tx,
                                            ).await;
                                        }
                                        Err(e) => {
                                            debug!("pi: ignoring non-JSON line: {} ({})", raw, e);
                                        }
                                    }
                                }
                                Ok(None) => break, // EOF
                                Err(e) => {
                                    debug!("pi stdout read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }

                // Child exited — fire turn completion with error if pending.
                let mut tc = turn_complete_tx.lock().await;
                if let Some(tx) = tc.take() {
                    let _ = tx.send(Err(ProviderError::PiFailed(
                        "PI process exited unexpectedly".into(),
                    )));
                }
            });
        }

        // Store writer/child/cancel so send_message works for the probe.
        inner.writer_tx = Some(writer_tx);
        inner.child = Some(child);
        inner.child_cancel = Some(child_cancel);

        // Drop the inner lock so send_message can acquire it.
        drop(inner);

        // Send a lightweight probe to verify PI is alive and responsive.
        // get_available_models has no side effects and returns quickly.
        let probe_result = tokio::time::timeout(STARTUP_TIMEOUT, async {
            self.send_message(PiRpcMessage {
                msg_type: "get_available_models".into(),
                id: Some("startup-probe".into()),
                fields: json!({}),
            })
            .await
        })
        .await;

        match probe_result {
            Ok(Ok(())) => {
                // Brief yield to let the reader task process the response
                // and confirm the child is stable.
                tokio::time::sleep(Duration::from_millis(200)).await;
                info!("PI RPC process started (--no-tools --no-session)");
                Ok(())
            }
            Ok(Err(e)) => {
                // Probe send failed — child likely crashed.
                let mut inner = self.inner.lock().await;
                if let Some(ref cancel) = inner.child_cancel {
                    cancel.cancel();
                }
                if let Some(ref mut child) = inner.child {
                    let _ = child.kill().await;
                }
                inner.writer_tx = None;
                inner.child = None;
                inner.child_cancel = None;
                Err(ProviderError::PiFailed(format!(
                    "PI startup probe failed: {}",
                    e
                )))
            }
            Err(_elapsed) => {
                let mut inner = self.inner.lock().await;
                if let Some(ref cancel) = inner.child_cancel {
                    cancel.cancel();
                }
                if let Some(ref mut child) = inner.child {
                    let _ = child.kill().await;
                }
                inner.writer_tx = None;
                inner.child = None;
                inner.child_cancel = None;
                Err(ProviderError::PiFailed(format!(
                    "PI startup timed out after {}s — check `pi --version`.",
                    STARTUP_TIMEOUT.as_secs()
                )))
            }
        }
    }

    /// Route an inbound PI event, accumulating agent message text and firing
    /// completion signals.
    ///
    /// Text deltas are extracted from `assistantMessageEvent.delta` only when
    /// `assistantMessageEvent.type == "text_delta"`. Thinking and tool-call
    /// deltas are ignored to keep the output buffer clean for JSON parsing.
    ///
    /// The agent run completes on `agent_end` (not `turn_end` — PI may do
    /// multiple turns with tool calls before finishing).
    async fn route_event(
        event: PiEvent,
        event_tx: &broadcast::Sender<PiSessionEvent>,
        session_buffers: &Arc<Mutex<HashMap<String, String>>>,
        active_lane: &Arc<Mutex<Option<String>>>,
        turn_complete_tx: &TurnCompleteSender,
    ) {
        let lane_id = active_lane
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        match event.event_type.as_str() {
            "message_update" => {
                // PI nests text deltas inside assistantMessageEvent.
                // Only accumulate text_delta; skip thinking_delta, toolcall_delta.
                let ame = match event.data.get("assistantMessageEvent") {
                    Some(v) => v,
                    None => return,
                };
                let ame_type = ame.get("type").and_then(|v| v.as_str()).unwrap_or_default();

                if ame_type != "text_delta" {
                    debug!(
                        "pi: skipping non-text assistantMessageEvent type: {}",
                        ame_type
                    );
                    return;
                }

                let delta = ame
                    .get("delta")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                if !delta.is_empty() {
                    let mut bufs = session_buffers.lock().await;
                    let buf = bufs.entry(lane_id.clone()).or_default();
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

                let _ = event_tx.send(PiSessionEvent {
                    lane_id,
                    event_type: "message_update".into(),
                    delta,
                    data: event.data,
                });
            }
            "message_end" => {
                let _ = event_tx.send(PiSessionEvent {
                    lane_id,
                    event_type: "message_end".into(),
                    delta: String::new(),
                    data: event.data,
                });
            }
            "turn_end" => {
                // PI may send multiple turns before agent_end (tool loops).
                // Don't signal completion here — wait for agent_end.
                let _ = event_tx.send(PiSessionEvent {
                    lane_id,
                    event_type: "turn_end".into(),
                    delta: String::new(),
                    data: event.data,
                });
            }
            "agent_end" => {
                // Signal prompt completion — the full agent run is finished.
                let mut tc = turn_complete_tx.lock().await;
                if let Some(tx) = tc.take() {
                    let _ = tx.send(Ok(()));
                }

                let _ = event_tx.send(PiSessionEvent {
                    lane_id,
                    event_type: "agent_end".into(),
                    delta: String::new(),
                    data: event.data,
                });
            }
            "response" => {
                // Command response with matching id. Log for diagnostics.
                let id = event
                    .data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let success = event
                    .data
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                debug!("pi: response id={} success={}", id, success);
            }
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                debug!("pi tool event: {} (lane={})", event.event_type, lane_id);
            }
            other => {
                debug!("pi: ignoring event type: {} (lane={})", other, lane_id);
            }
        }
    }

    /// Send a message to the PI RPC process.
    async fn send_message(&self, msg: PiRpcMessage) -> Result<(), ProviderError> {
        let inner = self.inner.lock().await;
        let writer_tx = inner
            .writer_tx
            .as_ref()
            .ok_or_else(|| ProviderError::PiFailed("PI process not started".into()))?;

        writer_tx.send(msg).await.map_err(|e| {
            ProviderError::PiFailed(format!("Failed to send message to PI process: {}", e))
        })
    }

    /// Start a new session. Resets PI's internal state for a fresh review lane.
    pub async fn new_session(&self, lane_id: &str) -> Result<(), ProviderError> {
        // Clear previous session state for this lane.
        {
            let mut bufs = self.session_buffers.lock().await;
            bufs.insert(lane_id.to_string(), String::new());
        }
        {
            let mut lane = self.active_lane.lock().await;
            *lane = Some(lane_id.to_string());
        }

        self.send_message(PiRpcMessage {
            msg_type: "new_session".into(),
            id: None,
            fields: json!({}),
        })
        .await
    }

    /// Model switching is a no-op for v1. PI reads the model from its own
    /// config (~/.pi/config). A future version may send the `set_model`
    /// command if needed.
    #[allow(dead_code)]
    pub async fn set_model(&self, model: &str) -> Result<(), ProviderError> {
        debug!(
            "PI model switching disabled for v1 (requested: {}); \
             using PI's configured default model",
            model
        );
        Ok(())
    }

    /// Drive one agent run with the provided prompt text. Blocks until
    /// PI emits `agent_end` (the full run, including any tool-driven
    /// multi-turn work, is finished).
    ///
    /// Returns the accumulated assistant text that streamed in during
    /// the run.
    ///
    /// Callers must hold the session guard (`acquire_session_guard()`)
    /// for the entire new_session → prompt → agent_end lifecycle.
    pub async fn prompt(&self, lane_id: &str, prompt_text: &str) -> Result<String, ProviderError> {
        // Set up agent-end completion channel.
        let (tx, rx) = oneshot::channel();
        {
            let mut tc = self.turn_complete_tx.lock().await;
            *tc = Some(tx);
        }

        // Send the prompt command using the correct PI RPC format.
        self.send_message(PiRpcMessage {
            msg_type: "prompt".into(),
            id: None,
            fields: json!({"message": prompt_text}),
        })
        .await?;

        // Await agent_end completion.
        match rx.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(ProviderError::PiFailed(
                    "Completion channel dropped — PI process may have crashed".into(),
                ));
            }
        }

        // Drain the accumulated buffer.
        let text = {
            let mut bufs = self.session_buffers.lock().await;
            bufs.remove(lane_id).unwrap_or_default()
        };

        Ok(text)
    }

    /// Abort the current agent run.
    pub async fn abort(&self) -> Result<(), ProviderError> {
        self.send_message(PiRpcMessage {
            msg_type: "abort".into(),
            id: None,
            fields: json!({}),
        })
        .await
    }

    /// Shut down the PI process and clear state.
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;

        if let Some(ref cancel) = inner.child_cancel {
            cancel.cancel();
        }
        if let Some(ref mut child) = inner.child {
            let _ = child.kill().await;
        }

        inner.writer_tx = None;
        inner.child = None;
        inner.child_cancel = None;

        self.active_lane.lock().await.take();
        self.session_buffers.lock().await.clear();
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
        let manager = PiManager::new();
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_not_started_errors() {
        let manager = PiManager::new();
        let result = manager.new_session("security").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not started"));

        let result = manager.prompt("security", "hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shutdown_idempotent() {
        let manager = PiManager::new();
        manager.shutdown().await;
        manager.shutdown().await;
        assert!(!manager.is_running());
    }

    #[tokio::test]
    async fn test_route_event_message_update_accumulates() {
        let (event_tx, mut event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("security".to_string())));
        let turn_complete_tx = Arc::new(Mutex::new(None));

        session_buffers
            .lock()
            .await
            .insert("security".to_string(), String::new());

        // PI text deltas are nested inside assistantMessageEvent.
        let event = PiEvent {
            event_type: "message_update".into(),
            data: json!({
                "message": {},
                "assistantMessageEvent": {
                    "type": "text_delta",
                    "delta": "Hello "
                }
            }),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        let event2 = PiEvent {
            event_type: "message_update".into(),
            data: json!({
                "message": {},
                "assistantMessageEvent": {
                    "type": "text_delta",
                    "delta": "World"
                }
            }),
        };
        PiManager::route_event(
            event2,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        let buf = session_buffers.lock().await;
        assert_eq!(buf.get("security").unwrap(), "Hello World");

        let ev1 = event_rx.try_recv().unwrap();
        assert_eq!(ev1.event_type, "message_update");
        assert_eq!(ev1.delta, "Hello ");
        let ev2 = event_rx.try_recv().unwrap();
        assert_eq!(ev2.delta, "World");
    }

    #[tokio::test]
    async fn test_route_event_agent_end_fires_oneshot() {
        let (event_tx, _event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("perf".to_string())));
        let (tx, rx) = oneshot::channel();
        let turn_complete_tx = Arc::new(Mutex::new(Some(tx)));

        let event = PiEvent {
            event_type: "agent_end".into(),
            data: json!({"messages": []}),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        let result = rx.await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_route_event_turn_end_does_not_fire_oneshot() {
        let (event_tx, _event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("security".to_string())));
        let (tx, rx) = oneshot::channel();
        let turn_complete_tx = Arc::new(Mutex::new(Some(tx)));

        let event = PiEvent {
            event_type: "turn_end".into(),
            data: json!({}),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        // The oneshot should NOT have fired — it should still be waiting.
        // Drop the sender so rx returns Err (RecvError).
        drop(turn_complete_tx);
        assert!(rx.await.is_err());
    }

    #[tokio::test]
    async fn test_route_event_thinking_delta_not_accumulated() {
        let (event_tx, mut event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("security".to_string())));
        let turn_complete_tx = Arc::new(Mutex::new(None));

        session_buffers
            .lock()
            .await
            .insert("security".to_string(), String::new());

        let event = PiEvent {
            event_type: "message_update".into(),
            data: json!({
                "message": {},
                "assistantMessageEvent": {
                    "type": "thinking_delta",
                    "delta": "Let me think about this..."
                }
            }),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        // Buffer should remain empty — thinking deltas are not accumulated.
        let buf = session_buffers.lock().await;
        assert_eq!(buf.get("security").unwrap(), "");

        // No event should have been broadcast.
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_route_event_response_handled() {
        let (event_tx, mut event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("security".to_string())));
        let turn_complete_tx = Arc::new(Mutex::new(None));

        let event = PiEvent {
            event_type: "response".into(),
            data: json!({
                "id": "startup-probe",
                "command": "get_available_models",
                "success": true,
                "data": {}
            }),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        // Response events are logged but not broadcast.
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_route_event_ignores_unknown_types() {
        let (event_tx, mut event_rx) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("arch".to_string())));
        let turn_complete_tx = Arc::new(Mutex::new(None));

        let event = PiEvent {
            event_type: "some_unknown_event".into(),
            data: json!({"stuff": true}),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_buffer_cap_enforced() {
        let (event_tx, _) = broadcast::channel(128);
        let session_buffers = Arc::new(Mutex::new(HashMap::new()));
        let active_lane = Arc::new(Mutex::new(Some("security".to_string())));
        let turn_complete_tx = Arc::new(Mutex::new(None));

        session_buffers
            .lock()
            .await
            .insert("security".to_string(), String::new());

        let big_delta = "x".repeat(MAX_SESSION_BUFFER_BYTES + 100);
        let event = PiEvent {
            event_type: "message_update".into(),
            data: json!({
                "message": {},
                "assistantMessageEvent": {
                    "type": "text_delta",
                    "delta": big_delta
                }
            }),
        };
        PiManager::route_event(
            event,
            &event_tx,
            &session_buffers,
            &active_lane,
            &turn_complete_tx,
        )
        .await;

        let buf = session_buffers.lock().await;
        assert!(buf.get("security").unwrap().len() <= MAX_SESSION_BUFFER_BYTES);
    }
}
