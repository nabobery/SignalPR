use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

// ---------------------------------------------------------------------------
// Framing mode
// ---------------------------------------------------------------------------

/// Wire framing for the JSON-RPC transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingMode {
    /// One JSON object per line (used by Codex CLI).
    NewlineDelimited,
    /// `Content-Length: N\r\n\r\n{body}` framing (used by Copilot CLI, same as LSP).
    ContentLength,
}

// ---------------------------------------------------------------------------
// Wire types (shared across all JSON-RPC providers)
// ---------------------------------------------------------------------------

/// A JSON-RPC error object embedded in an error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Outbound message we can serialize onto the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OutboundMessage {
    Request {
        id: Value,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Response {
        id: Value,
        result: Value,
    },
    Notification {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
}

/// Discriminated inbound message parsed from the wire.
#[derive(Debug, Clone)]
pub enum InboundMessage {
    /// Has `id` + `result`
    Response { id: Value, result: Value },
    /// Has `id` + `error`
    ErrorResponse { id: Value, error: JsonRpcError },
    /// Has `id` + `method` (server-initiated request, e.g. approval)
    ServerRequest {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    /// Has `method` but no `id`
    Notification {
        method: String,
        params: Option<Value>,
    },
}

/// A server-initiated request forwarded to the consumer.
#[derive(Debug, Clone)]
pub struct ServerRequest {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

/// A server notification forwarded to the consumer.
#[derive(Debug, Clone)]
pub struct ServerNotification {
    pub method: String,
    pub params: Option<Value>,
}

// ---------------------------------------------------------------------------
// Transport-level error (provider-agnostic)
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum JsonRpcTransportError {
    #[error("Transport channel closed")]
    ChannelClosed,

    #[error("Operation cancelled")]
    Cancelled,

    #[error("JSON-RPC error {code}: {message}")]
    RpcError { code: i64, message: String },

    #[error("Response channel dropped (transport shut down)")]
    ResponseDropped,
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 compliance helper
// ---------------------------------------------------------------------------

/// Serialize an `OutboundMessage` to a `Value` and inject `"jsonrpc": "2.0"`.
///
/// This avoids modifying the `OutboundMessage` enum (which would break all call
/// sites) while ensuring the wire output is JSON-RPC 2.0 compliant.
pub fn inject_jsonrpc(msg: &OutboundMessage) -> Value {
    let mut v = serde_json::to_value(msg).unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.insert("jsonrpc".into(), Value::String("2.0".into()));
    }
    v
}

// ---------------------------------------------------------------------------
// Parsing helper
// ---------------------------------------------------------------------------

/// Discriminate a raw JSON `Value` into a typed `InboundMessage`.
pub fn parse_inbound(raw: Value) -> Option<InboundMessage> {
    let obj = raw.as_object()?;

    let has_id = obj.contains_key("id");
    let has_method = obj.contains_key("method");
    let has_result = obj.contains_key("result");
    let has_error = obj.contains_key("error");

    if has_id && has_method {
        Some(InboundMessage::ServerRequest {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            method: obj
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            params: obj.get("params").cloned(),
        })
    } else if has_id && has_error {
        let error: JsonRpcError =
            serde_json::from_value(obj.get("error").cloned().unwrap_or(Value::Null)).ok()?;
        Some(InboundMessage::ErrorResponse {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            error,
        })
    } else if has_id && has_result {
        Some(InboundMessage::Response {
            id: obj.get("id").cloned().unwrap_or(Value::Null),
            result: obj.get("result").cloned().unwrap_or(Value::Null),
        })
    } else if has_method && !has_id {
        Some(InboundMessage::Notification {
            method: obj
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            params: obj.get("params").cloned(),
        })
    } else {
        warn!(
            "Unrecognized JSON-RPC message shape: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_inject_jsonrpc_request() {
        let msg = OutboundMessage::Request {
            id: json!(1),
            method: "ping".into(),
            params: None,
        };
        let v = inject_jsonrpc(&msg);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "ping");
    }

    #[test]
    fn test_inject_jsonrpc_response() {
        let msg = OutboundMessage::Response {
            id: json!(1),
            result: json!({"ok": true}),
        };
        let v = inject_jsonrpc(&msg);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["ok"], true);
    }

    #[test]
    fn test_inject_jsonrpc_notification() {
        let msg = OutboundMessage::Notification {
            method: "initialized".into(),
            params: Some(json!({"ready": true})),
        };
        let v = inject_jsonrpc(&msg);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "initialized");
        assert_eq!(v["params"]["ready"], true);
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
}
