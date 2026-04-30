use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot, Mutex};
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeEvent {
    pub event_type: String,
    pub lane_id: String,
    pub delta: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodePermissionRequest {
    pub lane_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub reason: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeCodeHealthInfo {
    pub status: String,
    pub bridge_version: String,
    pub mode: String,
    pub sdk_version: String,
}

type SharedChild = Arc<Mutex<Option<Child>>>;

#[derive(Clone)]
struct RunningReview {
    process: SharedChild,
}

struct Inner {
    reviews: HashMap<String, RunningReview>,
}

pub struct ClaudeCodeManager {
    inner: Mutex<Inner>,
    events_tx: broadcast::Sender<ClaudeCodeEvent>,
    permissions_tx: broadcast::Sender<ClaudeCodePermissionRequest>,
}

impl ClaudeCodeManager {
    pub fn new() -> Self {
        let (events_tx, _) = broadcast::channel(256);
        let (permissions_tx, _) = broadcast::channel(64);
        Self {
            inner: Mutex::new(Inner {
                reviews: HashMap::new(),
            }),
            events_tx,
            permissions_tx,
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ClaudeCodeEvent> {
        self.events_tx.subscribe()
    }

    pub fn subscribe_permissions(&self) -> broadcast::Receiver<ClaudeCodePermissionRequest> {
        self.permissions_tx.subscribe()
    }

    async fn register_review(
        &self,
        lane_id: &str,
        child: Child,
    ) -> Result<SharedChild, crate::errors::ProviderError> {
        let mut inner = self.inner.lock().await;
        if inner.reviews.contains_key(lane_id) {
            return Err(crate::errors::ProviderError::ClaudeCodeFailed(format!(
                "Lane '{}' already has an active Claude Code review",
                lane_id
            )));
        }

        let process = Arc::new(Mutex::new(Some(child)));
        inner.reviews.insert(
            lane_id.to_string(),
            RunningReview {
                process: process.clone(),
            },
        );
        Ok(process)
    }

    async fn remove_review(&self, lane_id: &str) -> Option<SharedChild> {
        let mut inner = self.inner.lock().await;
        inner.reviews.remove(lane_id).map(|review| review.process)
    }

    pub async fn cancel_lane(&self, lane_id: &str) {
        if let Some(process) = self.remove_review(lane_id).await {
            Self::kill_process(process).await;
        }
    }

    async fn kill_process(process: SharedChild) {
        let mut guard = process.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn validate_sidecar_binary(sidecar_path: &Path) -> Result<(), String> {
        let metadata = std::fs::metadata(sidecar_path).map_err(|e| {
            format!(
                "claude-code-bridge sidecar not found at {}: {}",
                sidecar_path.display(),
                e
            )
        })?;

        if !metadata.is_file() {
            return Err(format!(
                "claude-code-bridge sidecar path is not a file: {}",
                sidecar_path.display()
            ));
        }

        if metadata.len() == 0 {
            return Err(format!(
                "claude-code-bridge sidecar is empty: {}. Rebuild it with `pnpm sidecar:claude-code:build`.",
                sidecar_path.display()
            ));
        }

        Ok(())
    }

    /// Spawn the claude-code-bridge sidecar process and run a review.
    /// Each review gets its own sidecar process for isolation.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_review(
        self: &Arc<Self>,
        lane_id: &str,
        system_prompt: &str,
        diff: &str,
        output_schema: &str,
        cwd: &str,
        app_data_dir: &std::path::Path,
        sidecar_path: &str,
        mock_mode: bool,
    ) -> Result<serde_json::Value, crate::errors::ProviderError> {
        Self::validate_sidecar_binary(Path::new(sidecar_path))
            .map_err(crate::errors::ProviderError::ClaudeCodeFailed)?;

        let config_dir = app_data_dir.join("claude_code").join("config");
        let tmp_dir = app_data_dir.join("claude_code").join("tmp");
        std::fs::create_dir_all(&config_dir).ok();
        std::fs::create_dir_all(&tmp_dir).ok();

        let mut cmd = Command::new(sidecar_path);
        if mock_mode {
            cmd.arg("--mock");
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(cwd)
            .env("CLAUDE_CONFIG_DIR", config_dir.to_string_lossy().as_ref())
            .env("CLAUDE_CODE_TMPDIR", tmp_dir.to_string_lossy().as_ref())
            .env("CLAUDE_CODE_SKIP_PROMPT_HISTORY", "1");

        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            cmd.env("ANTHROPIC_API_KEY", key);
        }

        let mut child = cmd.spawn().map_err(|e| {
            crate::errors::ProviderError::ClaudeCodeFailed(format!(
                "Failed to spawn sidecar: {}",
                e
            ))
        })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            crate::errors::ProviderError::ClaudeCodeFailed("No stdin handle".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            crate::errors::ProviderError::ClaudeCodeFailed("No stdout handle".into())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            crate::errors::ProviderError::ClaudeCodeFailed("No stderr handle".into())
        })?;

        let _process = self.register_review(lane_id, child).await?;
        let lane_id_owned = lane_id.to_string();
        let events_tx = self.events_tx.clone();
        let permissions_tx = self.permissions_tx.clone();

        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(content) => {
                        let trimmed = if content.len() > 1024 {
                            &content[..1024]
                        } else {
                            content.as_str()
                        };
                        debug!("claude_code stderr: {}", trimmed);
                    }
                    Err(err) => {
                        debug!("claude_code stderr read error: {}", err);
                        break;
                    }
                }
            }
        });

        let (result_tx, result_rx) =
            oneshot::channel::<Result<serde_json::Value, crate::errors::ProviderError>>();

        let reader_lane_id = lane_id_owned.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut final_result: Option<serde_json::Value> = None;

            for line in reader.lines() {
                let line = match line {
                    Ok(content) => content,
                    Err(err) => {
                        let _ =
                            result_tx.send(Err(crate::errors::ProviderError::ClaudeCodeFailed(
                                format!("stdout read failed: {}", err),
                            )));
                        return;
                    }
                };

                if line.trim().is_empty() {
                    continue;
                }

                let msg: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("claude_code stdout JSON parse failed: {}", err);
                        continue;
                    }
                };

                if let Some(method) = msg.get("method").and_then(|value| value.as_str()) {
                    let params = msg
                        .get("params")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    match method {
                        "review.delta" => {
                            let delta = params
                                .get("chunk")
                                .and_then(|value| value.as_str())
                                .unwrap_or_default();
                            let _ = events_tx.send(ClaudeCodeEvent {
                                event_type: "review.delta".into(),
                                lane_id: reader_lane_id.clone(),
                                delta: delta.to_string(),
                                data: params,
                            });
                        }
                        "review.permission_requested" => {
                            let request = ClaudeCodePermissionRequest {
                                lane_id: reader_lane_id.clone(),
                                tool_name: params
                                    .get("tool_name")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                tool_input: params
                                    .get("tool_input")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null),
                                reason: params
                                    .get("reason")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                action: params
                                    .get("action")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("denied")
                                    .to_string(),
                            };
                            let _ = permissions_tx.send(request);
                        }
                        "review.completed" => {
                            final_result = params.get("output").cloned();
                            let _ = events_tx.send(ClaudeCodeEvent {
                                event_type: "review.completed".into(),
                                lane_id: reader_lane_id.clone(),
                                delta: String::new(),
                                data: params,
                            });
                        }
                        "review.error" => {
                            let error_message = params
                                .get("error")
                                .and_then(|value| value.as_str())
                                .unwrap_or("Unknown error")
                                .to_string();
                            let _ = events_tx.send(ClaudeCodeEvent {
                                event_type: "review.error".into(),
                                lane_id: reader_lane_id.clone(),
                                delta: String::new(),
                                data: params,
                            });
                            let _ = result_tx.send(Err(
                                crate::errors::ProviderError::ClaudeCodeFailed(error_message),
                            ));
                            return;
                        }
                        _ => {}
                    }
                }
            }

            match final_result {
                Some(output) => {
                    let _ = result_tx.send(Ok(output));
                }
                None => {
                    let _ = result_tx.send(Err(crate::errors::ProviderError::ClaudeCodeFailed(
                        "Bridge closed without completing review".into(),
                    )));
                }
            }
        });

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "review.start",
            "params": {
                "lane_id": lane_id,
                "system_prompt": system_prompt,
                "diff": diff,
                "output_schema": output_schema,
                "cwd": cwd,
            }
        });

        let request_str = serde_json::to_string(&request).map_err(|e| {
            crate::errors::ProviderError::ClaudeCodeFailed(format!("JSON serialize error: {}", e))
        })?;

        stdin.write_all(request_str.as_bytes()).map_err(|e| {
            crate::errors::ProviderError::ClaudeCodeFailed(format!("Write to stdin failed: {}", e))
        })?;
        stdin.write_all(b"\n").map_err(|e| {
            crate::errors::ProviderError::ClaudeCodeFailed(format!("Write newline failed: {}", e))
        })?;
        stdin.flush().map_err(|e| {
            crate::errors::ProviderError::ClaudeCodeFailed(format!("Flush failed: {}", e))
        })?;

        let result = result_rx.await.map_err(|_| {
            crate::errors::ProviderError::ClaudeCodeFailed("Result channel closed".into())
        })?;

        if let Some(process) = self.remove_review(lane_id).await {
            Self::kill_process(process).await;
        }

        result
    }

    /// Run health.check against the sidecar.
    pub fn check_health(
        sidecar_path: &str,
        app_data_dir: &std::path::Path,
        mock_mode: bool,
    ) -> Result<ClaudeCodeHealthInfo, String> {
        Self::validate_sidecar_binary(Path::new(sidecar_path))?;

        let config_dir = app_data_dir.join("claude_code").join("config");
        let tmp_dir = app_data_dir.join("claude_code").join("tmp");
        std::fs::create_dir_all(&config_dir).ok();
        std::fs::create_dir_all(&tmp_dir).ok();

        let mut cmd = Command::new(sidecar_path);
        if mock_mode {
            cmd.arg("--mock");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CLAUDE_CONFIG_DIR", config_dir.to_string_lossy().as_ref())
            .env("CLAUDE_CODE_TMPDIR", tmp_dir.to_string_lossy().as_ref())
            .env("CLAUDE_CODE_SKIP_PROMPT_HISTORY", "1")
            .spawn()
            .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;

        let mut stdin = child.stdin.take().ok_or("No stdin")?;
        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stderr = child.stderr.take().ok_or("No stderr")?;

        let stderr_capture = Arc::new(std::sync::Mutex::new(String::new()));
        {
            let capture = stderr_capture.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(content) => {
                            let mut guard = capture.lock().expect("stderr capture poisoned");
                            if guard.len() < 8 * 1024 {
                                guard.push_str(&content);
                                guard.push('\n');
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "health.check",
            "params": {}
        });
        stdin
            .write_all(
                serde_json::to_string(&request)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;

        let shutdown = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "bridge.shutdown",
            "params": {}
        });
        stdin
            .write_all(
                serde_json::to_string(&shutdown)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        let _ = stdin.flush();
        drop(stdin);

        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(content) => content,
                Err(err) => {
                    let _ = child.wait();
                    return Err(format!("health.check stdout read failed: {}", err));
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            let msg: serde_json::Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(err) => {
                    warn!("claude_code health JSON parse failed: {}", err);
                    continue;
                }
            };

            if let Some(result) = msg.get("result") {
                if result.get("status").and_then(|value| value.as_str()) == Some("ok") {
                    let info = ClaudeCodeHealthInfo {
                        status: "ok".into(),
                        bridge_version: result
                            .get("bridge_version")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown")
                            .into(),
                        mode: result
                            .get("mode")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown")
                            .into(),
                        sdk_version: result
                            .get("sdk_version")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown")
                            .into(),
                    };
                    let _ = child.wait();
                    return Ok(info);
                }
            }

            if let Some(error) = msg.get("error") {
                let _ = child.wait();
                return Err(format!("health.check returned error: {}", error));
            }
        }

        let stderr_output = stderr_capture
            .lock()
            .map(|guard| guard.trim().to_string())
            .unwrap_or_default();
        let _ = child.kill();
        let _ = child.wait();

        if stderr_output.is_empty() {
            Err("health.check did not return ok".into())
        } else {
            Err(format!(
                "health.check did not return ok. stderr: {}",
                stderr_output
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClaudeCodeManager, RunningReview};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;

    fn spawn_long_running_child() -> Child {
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "ping -n 30 127.0.0.1 > NUL"]);
            cmd
        };

        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "sleep 30"]);
            cmd
        };

        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("test child should spawn")
    }

    #[tokio::test]
    async fn cancel_lane_only_kills_matching_process() {
        let manager = ClaudeCodeManager::new();
        let lane_a = "lane-a";
        let lane_b = "lane-b";

        let child_a = spawn_long_running_child();
        let child_b = spawn_long_running_child();

        let process_a = Arc::new(tokio::sync::Mutex::new(Some(child_a)));
        let process_b = Arc::new(tokio::sync::Mutex::new(Some(child_b)));

        {
            let mut inner = manager.inner.lock().await;
            inner.reviews.insert(
                lane_a.to_string(),
                RunningReview {
                    process: process_a.clone(),
                },
            );
            inner.reviews.insert(
                lane_b.to_string(),
                RunningReview {
                    process: process_b.clone(),
                },
            );
        }

        manager.cancel_lane(lane_a).await;

        assert!(
            manager.inner.lock().await.reviews.get(lane_a).is_none(),
            "cancelled lane should be removed"
        );
        assert!(
            manager.inner.lock().await.reviews.contains_key(lane_b),
            "other lanes must remain registered"
        );

        let lane_b_alive = {
            let mut guard = process_b.lock().await;
            let child = guard.as_mut().expect("lane-b child should still exist");
            child.try_wait().expect("try_wait should succeed").is_none()
        };
        assert!(
            lane_b_alive,
            "cancelling one lane must not kill another lane"
        );

        manager.cancel_lane(lane_b).await;
        assert!(
            manager.inner.lock().await.reviews.is_empty(),
            "all lane registrations should be cleaned up"
        );
    }

    #[test]
    fn validate_sidecar_binary_rejects_empty_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let empty_file = dir.path().join("claude-code-bridge");
        std::fs::write(&empty_file, "").expect("write temp file");

        let error =
            ClaudeCodeManager::validate_sidecar_binary(&empty_file).expect_err("empty file");
        assert!(error.contains("sidecar is empty"));
    }
}
