//! Codex App Server transport layer.
//!
//! Re-exports the shared JSON-RPC transport from `providers::jsonrpc` and provides
//! a thin adapter that maps `JsonRpcTransportError` to `ProviderError::CodexFailed`.

pub use crate::providers::jsonrpc::types::{ServerNotification, ServerRequest};

use crate::errors::ProviderError;
use crate::providers::jsonrpc::transport::JsonRpcTransport as SharedTransport;
use crate::providers::jsonrpc::types::{FramingMode, JsonRpcTransportError};

use serde_json::Value;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Codex-flavored JSON-RPC transport that maps transport errors to `ProviderError::CodexFailed`.
#[allow(dead_code)]
pub struct JsonRpcTransport {
    inner: SharedTransport,
}

impl Clone for JsonRpcTransport {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[allow(dead_code)]
impl JsonRpcTransport {
    pub fn new(
        stdin: ChildStdin,
        stdout: tokio::process::ChildStdout,
        cancel: CancellationToken,
        server_request_tx: mpsc::Sender<ServerRequest>,
        notification_tx: mpsc::Sender<ServerNotification>,
    ) -> Self {
        Self {
            inner: SharedTransport::new(
                stdin,
                stdout,
                cancel,
                server_request_tx,
                notification_tx,
                FramingMode::NewlineDelimited,
            ),
        }
    }

    pub async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, ProviderError> {
        self.inner
            .send_request(method, params)
            .await
            .map_err(codex_transport_error)
    }

    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), ProviderError> {
        self.inner
            .send_notification(method, params)
            .await
            .map_err(codex_transport_error)
    }

    pub async fn send_response(&self, id: Value, result: Value) -> Result<(), ProviderError> {
        self.inner
            .send_response(id, result)
            .await
            .map_err(codex_transport_error)
    }

    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

/// Map generic transport errors to `ProviderError::CodexFailed`.
fn codex_transport_error(e: JsonRpcTransportError) -> ProviderError {
    match e {
        JsonRpcTransportError::Cancelled => ProviderError::Cancelled,
        other => ProviderError::CodexFailed(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::jsonrpc::types::{parse_inbound, InboundMessage, OutboundMessage};
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

    #[tokio::test]
    async fn test_transport_request_response_roundtrip() {
        let request = OutboundMessage::Request {
            id: json!(42),
            method: "test/echo".into(),
            params: Some(json!({"msg": "hello"})),
        };
        let serialized = serde_json::to_string(&request).unwrap();

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

        let req_parsed: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(req_parsed["method"], "test/echo");
    }

    #[test]
    fn test_codex_transport_error_maps_cancelled() {
        let err = codex_transport_error(JsonRpcTransportError::Cancelled);
        assert!(matches!(err, ProviderError::Cancelled));
    }

    #[test]
    fn test_codex_transport_error_maps_others_to_codex_failed() {
        let err = codex_transport_error(JsonRpcTransportError::ChannelClosed);
        assert!(matches!(err, ProviderError::CodexFailed(_)));

        let err = codex_transport_error(JsonRpcTransportError::RpcError {
            code: -32600,
            message: "Invalid".into(),
        });
        assert!(matches!(err, ProviderError::CodexFailed(_)));

        let err = codex_transport_error(JsonRpcTransportError::ResponseDropped);
        assert!(matches!(err, ProviderError::CodexFailed(_)));
    }
}
