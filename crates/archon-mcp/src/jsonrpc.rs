//! JSON-RPC 2.0 message shapes for MCP over stdio (one message per line).
//! Every response leaves as one line within [`MAX_MESSAGE_BYTES`]: Aether
//! treats a longer one as a dead server and withdraws every Archon tool.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::MAX_MESSAGE_BYTES;

/// MCP protocol revision this server implements, and answers with when the
/// client asks for one it does not know.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions a client may pick. Tools, `ping` and the lifecycle are the same
/// in each; `structuredContent` is simply ignored by pre-2025-06-18 clients.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2025-11-25",
    MCP_PROTOCOL_VERSION,
    "2025-03-26",
    "2024-11-05",
];

/// The spec's negotiation: echo a supported revision, else offer ours.
pub fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .find(|v| **v == requested)
        .copied()
        .unwrap_or(MCP_PROTOCOL_VERSION)
}

/// Longest request `id` accepted, as serialized. With the id bounded, an
/// error response always fits the message cap, so every request is answered.
pub const MAX_ID_BYTES: usize = 256;

/// Longest JSON-RPC error message; a longer one is cut with a marker.
pub const MAX_ERROR_MESSAGE_BYTES: usize = 4096;

/// A request (with `id`) or a notification (without).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Incoming {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// What one line from the client turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
    },
    /// A response to a request this server sent. It sends none, so these are
    /// only logged.
    Response,
}

impl Message {
    /// Classify one line, or the error response it earns. Batches are not
    /// part of MCP since 2025-06-18 and are refused as invalid requests.
    pub fn parse(line: &[u8]) -> Result<Self, Box<Response>> {
        let value: Value = serde_json::from_slice(line).map_err(|e| {
            Box::new(Response::error(
                Value::Null,
                PARSE_ERROR,
                format!("parse error: {e}"),
            ))
        })?;
        let invalid = |id: Value, why: &str| {
            Box::new(Response::error(
                id,
                INVALID_REQUEST,
                format!("invalid request: {why}"),
            ))
        };
        let Some(object) = value.as_object() else {
            return Err(invalid(
                Value::Null,
                "expected one JSON-RPC object per line",
            ));
        };
        let id = object.get("id").cloned();
        let id_ok = id
            .as_ref()
            .is_none_or(|id| id.is_string() || id.is_number());
        if !id_ok {
            return Err(invalid(Value::Null, "`id` must be a string or a number"));
        }
        if !object.contains_key("method")
            && (object.contains_key("result") || object.contains_key("error"))
        {
            return Ok(Self::Response);
        }
        let id_bytes = id.as_ref().map_or(0, encoded_len);
        if id_bytes > MAX_ID_BYTES {
            let why = format!("`id` is {id_bytes} bytes; at most {MAX_ID_BYTES} are accepted");
            return Err(invalid(Value::Null, &why));
        }
        let incoming: Incoming = serde_json::from_value(value.clone())
            .map_err(|e| invalid(id.clone().unwrap_or(Value::Null), &e.to_string()))?;
        if incoming.jsonrpc != "2.0" {
            return Err(invalid(
                id.unwrap_or(Value::Null),
                "`jsonrpc` must be \"2.0\"",
            ));
        }
        Ok(match incoming.id {
            Some(id) => Self::Request {
                id,
                method: incoming.method,
                params: incoming.params,
            },
            None => Self::Notification {
                method: incoming.method,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    /// The message is cut to [`MAX_ERROR_MESSAGE_BYTES`], since it may
    /// quote the caller's input.
    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        let message: String = message.into();
        let message = truncate(&message, MAX_ERROR_MESSAGE_BYTES).into_owned();
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message }),
        }
    }

    /// This response as one newline-terminated line of at most
    /// [`MAX_MESSAGE_BYTES`]. One that would be longer is replaced by an
    /// error under the same id, so the caller still learns its call failed.
    pub fn to_line(&self) -> Result<Vec<u8>, serde_json::Error> {
        let line = encode_line(self)?;
        if line.len() <= MAX_MESSAGE_BYTES {
            return Ok(line);
        }
        tracing::warn!(
            bytes = line.len(),
            "replaced an MCP response over the message cap"
        );
        let refusal = Self::error(
            self.id.clone(),
            INTERNAL_ERROR,
            format!(
                "the response was {} bytes, over the {MAX_MESSAGE_BYTES}-byte MCP message \
                 limit, so it was not sent",
                line.len()
            ),
        );
        let line = encode_line(&refusal)?;
        assert!(
            line.len() <= MAX_MESSAGE_BYTES,
            "ids are bounded by MAX_ID_BYTES, so a refusal always fits"
        );
        Ok(line)
    }
}

fn encode_line(response: &Response) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(response)?;
    debug_assert!(
        !bytes.contains(&b'\n'),
        "serde_json never emits raw newlines"
    );
    bytes.push(b'\n');
    Ok(bytes)
}

/// Serialized size; a value that cannot be encoded counts as too large.
pub(crate) fn encoded_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

/// `text` cut to at most `max_bytes` on a character boundary, ending with a
/// marker that says how long the original was.
pub fn truncate(text: &str, max_bytes: usize) -> Cow<'_, str> {
    if text.len() <= max_bytes {
        return Cow::Borrowed(text);
    }
    let marker = format!("… [truncated from {} bytes]", text.len());
    assert!(
        max_bytes >= 2 * marker.len(),
        "a {max_bytes}-byte limit leaves no room for the text"
    );
    let kept = text.floor_char_boundary(max_bytes - marker.len());
    let cut = format!("{}{marker}", &text[..kept]);
    assert!(cut.len() <= max_bytes, "the cut text fits its limit");
    Cow::Owned(cut)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn error_code(line: &str) -> (i64, Value) {
        let response = Message::parse(line.as_bytes()).expect_err("an error response");
        (response.error.as_ref().expect("error").code, response.id)
    }

    #[test]
    fn notifications_have_no_id_and_responses_carry_one_side() {
        let note: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(note.id.is_none());
        let value = serde_json::to_value(Response::ok(1.into(), serde_json::json!({}))).unwrap();
        assert!(value.get("error").is_none());
        assert_eq!(value["jsonrpc"], "2.0");
    }

    #[test]
    fn lines_classify_into_requests_notifications_and_responses() {
        let request = Message::parse(br#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#).unwrap();
        assert!(matches!(request, Message::Request { id, .. } if id == "a"));
        let note =
            Message::parse(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#);
        assert!(matches!(note, Ok(Message::Notification { .. })));
        let response = Message::parse(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#);
        assert_eq!(response, Ok(Message::Response));
    }

    #[test]
    fn malformed_lines_earn_the_spec_error_codes() {
        assert_eq!(error_code("{nope"), (PARSE_ERROR, Value::Null));
        assert_eq!(error_code("[]").0, INVALID_REQUEST);
        assert_eq!(
            error_code(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#).0,
            INVALID_REQUEST
        );
        let (code, id) = error_code(r#"{"jsonrpc":"1.0","id":7,"method":"ping"}"#);
        assert_eq!((code, id), (INVALID_REQUEST, Value::from(7)));
        assert_eq!(error_code(r#"{"jsonrpc":"2.0","id":8}"#).0, INVALID_REQUEST);
    }

    #[test]
    fn ids_past_the_limit_are_refused_without_being_echoed() {
        let quoted = 2;
        let longest =
            json!({"jsonrpc": "2.0", "id": "a".repeat(MAX_ID_BYTES - quoted), "method": "ping"});
        let parsed = Message::parse(longest.to_string().as_bytes());
        assert!(matches!(parsed, Ok(Message::Request { .. })), "{parsed:?}");
        let over = json!({"jsonrpc": "2.0", "id": "a".repeat(MAX_ID_BYTES), "method": "ping"});
        assert_eq!(
            error_code(&over.to_string()),
            (INVALID_REQUEST, Value::Null)
        );
    }

    #[test]
    fn error_messages_are_cut_on_a_character_boundary_with_a_marker() {
        let long = "é".repeat(MAX_ERROR_MESSAGE_BYTES);
        let response = Response::error(Value::Null, INVALID_PARAMS, long.clone());
        let message = response.error.expect("an error").message;
        assert!(
            message.len() <= MAX_ERROR_MESSAGE_BYTES,
            "{}",
            message.len()
        );
        let marker = format!("… [truncated from {} bytes]", long.len());
        assert!(message.ends_with(&marker), "{message}");
        assert!(message.starts_with("éé"));
        assert_eq!(truncate("short", 64), "short");
    }

    #[test]
    fn a_response_over_the_cap_is_replaced_by_an_error_under_its_id() {
        let fits = Response::ok(1.into(), json!({})).to_line().unwrap();
        assert_eq!(fits, b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        let text = "x".repeat(MAX_MESSAGE_BYTES);
        let huge = Response::ok("call-7".into(), json!({"text": text}));
        let line = huge.to_line().unwrap();
        assert!(line.len() <= MAX_MESSAGE_BYTES && line.ends_with(b"\n"));
        let sent: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(sent["id"], "call-7");
        assert_eq!(sent["error"]["code"], INTERNAL_ERROR);
        let message = sent["error"]["message"].as_str().unwrap();
        assert!(message.contains("MCP message limit"), "{message}");
    }

    #[test]
    fn negotiation_echoes_known_revisions_and_offers_ours_otherwise() {
        assert_eq!(negotiate("2025-03-26"), "2025-03-26");
        assert_eq!(negotiate("2025-11-25"), "2025-11-25");
        assert_eq!(negotiate("1999-01-01"), MCP_PROTOCOL_VERSION);
    }
}
