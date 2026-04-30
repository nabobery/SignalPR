//! Provider conformance tests for claude-code-bridge in --mock mode.
//!
//! These tests spawn the sidecar binary and verify JSON-RPC protocol
//! compliance, cancellation, and fail-closed tool policy without any
//! model calls or API keys.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn sidecar_path() -> String {
    let triple = if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    } else {
        "x86_64-pc-windows-msvc"
    };

    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("claude-code-bridge-{}{}", triple, ext));
    path.to_string_lossy().to_string()
}

fn sidecar_is_usable() -> bool {
    let path = sidecar_path();
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    meta.is_file() && meta.len() > 0
}

/// Spawn the sidecar in --mock mode and return (child, stdin, stdout_reader).
fn spawn_mock() -> (
    std::process::Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut child = Command::new(sidecar_path())
        .arg("--mock")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CLAUDE_CONFIG_DIR", "/tmp/signalpr-test-config")
        .env("CLAUDE_CODE_TMPDIR", "/tmp/signalpr-test-tmp")
        .env("CLAUDE_CODE_SKIP_PROMPT_HISTORY", "1")
        .spawn()
        .expect("Failed to spawn sidecar");

    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    (child, stdin, stdout)
}

fn send_request(stdin: &mut impl Write, id: u64, method: &str, params: serde_json::Value) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
    stdin.flush().unwrap();
}

fn read_messages(
    reader: &mut BufReader<std::process::ChildStdout>,
    timeout: Duration,
) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    let start = std::time::Instant::now();

    // Read until timeout or EOF
    loop {
        if start.elapsed() > timeout {
            break;
        }

        let mut line = String::new();
        // Non-blocking read: we'll just try and handle WouldBlock-like situations
        // by relying on the timeout
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                    messages.push(msg);
                }
            }
            Err(_) => break,
        }

        // If we got a review.completed or an error, we can stop
        if let Some(last) = messages.last() {
            if let Some(method) = last.get("method").and_then(|v| v.as_str()) {
                if method == "review.completed" || method == "review.error" {
                    break;
                }
            }
        }
    }

    messages
}

#[test]
fn test_health_check_handshake() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    send_request(&mut stdin, 1, "health.check", serde_json::json!({}));

    // Read response
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["status"], "ok");
    assert_eq!(response["result"]["mode"], "mock");
    assert!(response["result"]["bridge_version"].is_string());
    assert!(response["result"]["env"]["CLAUDE_CONFIG_DIR"].is_string());

    // Shutdown
    send_request(&mut stdin, 2, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn test_review_start_emits_deltas_then_completion() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    send_request(
        &mut stdin,
        1,
        "review.start",
        serde_json::json!({
            "lane_id": "test-lane-1",
            "system_prompt": "Review this code.",
            "diff": "--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old\n+new",
            "output_schema": "{}",
            "cwd": "/tmp"
        }),
    );

    let messages = read_messages(&mut reader, Duration::from_secs(5));

    // Should have at least: response ACK, some deltas, permission_requested, completed
    let methods: Vec<Option<&str>> = messages
        .iter()
        .map(|m| m.get("method").and_then(|v| v.as_str()))
        .collect();

    // First message is the response (has "result")
    assert!(
        messages[0].get("result").is_some(),
        "First message should be response ACK"
    );

    // Should contain review.delta notifications
    assert!(
        methods.contains(&Some("review.delta")),
        "Should have delta notifications"
    );

    // Should contain review.permission_requested (mock emits a Write denial)
    assert!(
        methods.contains(&Some("review.permission_requested")),
        "Should have permission_requested for denied tool"
    );

    // Should contain review.completed
    assert!(
        methods.contains(&Some("review.completed")),
        "Should have completion"
    );

    // Verify the completed output has findings
    let completed = messages
        .iter()
        .find(|m| m.get("method").and_then(|v| v.as_str()) == Some("review.completed"))
        .unwrap();
    let output = &completed["params"]["output"];
    assert!(output["findings"].is_array());
    assert!(!output["findings"].as_array().unwrap().is_empty());
    let first_finding = &output["findings"][0];
    assert!(
        first_finding["body"].is_string(),
        "mock output must use provider contract fields"
    );
    assert!(
        first_finding["file_path"].is_string(),
        "mock output must use file_path instead of legacy file"
    );
    assert!(first_finding["line_start"].is_number());
    assert!(first_finding["confidence"].is_number());
    assert!(first_finding["agent_type"].is_string());

    // Shutdown
    send_request(&mut stdin, 2, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn test_malformed_message_returns_error() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    // Send invalid JSON
    writeln!(stdin, "not valid json {{{{").unwrap();
    stdin.flush().unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32700);

    // Shutdown
    send_request(&mut stdin, 1, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn test_invalid_request_returns_error() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    // Valid JSON but missing jsonrpc field
    let msg = serde_json::json!({"id": 1, "method": "test"});
    writeln!(stdin, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
    stdin.flush().unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap();

    assert_eq!(response["error"]["code"], -32600);

    // Shutdown
    send_request(&mut stdin, 1, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn test_permission_requested_for_blocked_tool() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    send_request(
        &mut stdin,
        1,
        "review.start",
        serde_json::json!({
            "lane_id": "deny-test",
            "system_prompt": "test",
            "diff": "test diff",
            "output_schema": "{}",
            "cwd": "/tmp"
        }),
    );

    let messages = read_messages(&mut reader, Duration::from_secs(5));

    // Find the permission_requested notification
    let perm_req = messages
        .iter()
        .find(|m| m.get("method").and_then(|v| v.as_str()) == Some("review.permission_requested"))
        .expect("Should have a permission_requested notification");

    let params = &perm_req["params"];
    assert_eq!(params["tool_name"], "Write");
    assert_eq!(params["action"], "denied");
    assert!(params["reason"]
        .as_str()
        .unwrap()
        .contains("not in the allowed list"));

    // Shutdown
    send_request(&mut stdin, 2, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn test_review_cancel_stops_without_completion() {
    assert!(
        sidecar_is_usable(),
        "claude-code sidecar must be built before conformance tests run"
    );

    let (mut child, mut stdin, mut reader) = spawn_mock();

    send_request(
        &mut stdin,
        1,
        "review.start",
        serde_json::json!({
            "lane_id": "cancel-test",
            "system_prompt": "test",
            "diff": "test diff",
            "output_schema": "{}",
            "cwd": "/tmp"
        }),
    );

    let mut first_line = String::new();
    reader.read_line(&mut first_line).unwrap();
    let ack: serde_json::Value = serde_json::from_str(&first_line).unwrap();
    assert!(
        ack.get("result").is_some(),
        "review.start should ack before cancellation"
    );

    send_request(&mut stdin, 2, "review.cancel", serde_json::json!({}));

    let messages = read_messages(&mut reader, Duration::from_secs(5));
    let methods: Vec<Option<&str>> = messages
        .iter()
        .map(|m| m.get("method").and_then(|v| v.as_str()))
        .collect();

    assert!(
        methods.contains(&Some("review.error")),
        "cancelled review should surface a cancellation error"
    );
    assert!(
        !methods.contains(&Some("review.completed")),
        "cancelled review must not emit completion"
    );

    send_request(&mut stdin, 3, "bridge.shutdown", serde_json::json!({}));
    drop(stdin);
    let _ = child.wait();
}
