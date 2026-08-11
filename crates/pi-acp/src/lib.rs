//! Minimal ACP v1 JSON-RPC 2.0 over newline-delimited JSON transport.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use thiserror::Error;

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const ACP_PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u16);

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemCapabilities {
    #[serde(default)]
    pub read_text_file: bool,
    #[serde(default)]
    pub write_text_file: bool,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default)]
    pub fs: FileSystemCapabilities,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<Value>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Implementation {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    #[serde(default)]
    pub image: bool,
    #[serde(default)]
    pub audio: bool,
    #[serde(default)]
    pub embedded_context: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct McpCapabilities {
    #[serde(default)]
    pub http: bool,
    #[serde(default)]
    pub sse: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub load_session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_capabilities: Option<PromptCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_capabilities: Option<McpCapabilities>,
    #[serde(default)]
    pub session_capabilities: Value,
    #[serde(default)]
    pub auth: Value,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub protocol_version: ProtocolVersion,
    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<Implementation>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: ProtocolVersion,
    pub agent_capabilities: AgentCapabilities,
    #[serde(default)]
    pub auth_methods: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<Implementation>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl InitializeResponse {
    pub fn for_request(_request: &InitializeRequest, version: &str) -> Self {
        let negotiated = ACP_PROTOCOL_VERSION;
        Self {
            protocol_version: ProtocolVersion(negotiated),
            agent_capabilities: AgentCapabilities {
                load_session: false,
                prompt_capabilities: Some(PromptCapabilities::default()),
                mcp_capabilities: Some(McpCapabilities::default()),
                session_capabilities: serde_json::json!({}),
                auth: serde_json::json!({}),
                meta: None,
            },
            auth_methods: Vec::new(),
            agent_info: Some(Implementation {
                name: "pi-rs".into(),
                version: version.into(),
                title: None,
                meta: None,
            }),
            meta: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("message exceeds {MAX_MESSAGE_BYTES} bytes")]
    TooLarge,
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JSON-RPC message must be object")]
    NotObject,
    #[error("JSON-RPC version must be 2.0")]
    WrongVersion,
    #[error("invalid JSON-RPC envelope: {0}")]
    InvalidEnvelope(&'static str),
}

/// Validate JSON-RPC envelope shape without imposing ACP method semantics.
fn valid_id(value: &Value) -> bool {
    value.is_string() || value.as_i64().is_some() || value.as_u64().is_some()
}

pub fn validate_message(message: &Value) -> Result<(), Error> {
    let object = message.as_object().ok_or(Error::NotObject)?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Error::WrongVersion);
    }
    if let Some(method) = object.get("method") {
        if object.contains_key("result") || object.contains_key("error") {
            return Err(Error::InvalidEnvelope(
                "request cannot contain result or error",
            ));
        }
        if !method.is_string() {
            return Err(Error::InvalidEnvelope("method must be string"));
        }
        if let Some(params) = object.get("params") {
            if !(params.is_object() || params.is_array()) {
                return Err(Error::InvalidEnvelope("params must be object or array"));
            }
        }
        if let Some(id) = object.get("id") {
            if !valid_id(id) {
                return Err(Error::InvalidEnvelope("id must be string or integer"));
            }
        }
        return Ok(());
    }

    if !object.contains_key("id") {
        return Err(Error::InvalidEnvelope("response missing id"));
    }
    let id = &object["id"];
    if !id.is_null() && !valid_id(id) {
        return Err(Error::InvalidEnvelope("id must be string or integer"));
    }
    match (object.contains_key("result"), object.contains_key("error")) {
        (true, false) => Ok(()),
        (false, true) => {
            let error = object["error"]
                .as_object()
                .ok_or(Error::InvalidEnvelope("error must be object"))?;
            if error.get("code").and_then(Value::as_i64).is_none() {
                return Err(Error::InvalidEnvelope("error code must be integer"));
            }
            if error.get("message").and_then(Value::as_str).is_none() {
                return Err(Error::InvalidEnvelope("error message must be string"));
            }
            Ok(())
        }
        _ => Err(Error::InvalidEnvelope(
            "response needs exactly one of result or error",
        )),
    }
}

/// Read one ACP JSON-RPC message. Blank lines are ignored.
pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, Error> {
    let mut frame = Vec::new();
    loop {
        let (used, end) = {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                (0, true)
            } else if let Some(pos) = chunk.iter().position(|&byte| byte == b'\n') {
                frame.extend_from_slice(&chunk[..=pos]);
                (pos + 1, true)
            } else {
                frame.extend_from_slice(chunk);
                (chunk.len(), false)
            }
        };
        if used != 0 {
            reader.consume(used);
        }
        if frame.len() > MAX_MESSAGE_BYTES + 1 {
            return Err(Error::TooLarge);
        }
        if !end {
            continue;
        }
        if frame.is_empty() {
            return Ok(None);
        }
        let trimmed = frame.strip_suffix(b"\n").unwrap_or(&frame);
        let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
        if trimmed.len() > MAX_MESSAGE_BYTES {
            return Err(Error::TooLarge);
        }
        if trimmed.iter().all(u8::is_ascii_whitespace) {
            frame.clear();
            continue;
        }
        let value: Value = serde_json::from_slice(trimmed)?;
        validate_message(&value)?;
        return Ok(Some(value));
    }
}

/// Write one ACP JSON-RPC message followed by exactly one newline.
pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> Result<(), Error> {
    validate_message(message)?;
    let encoded = serde_json::to_vec(message)?;
    if encoded.len() > MAX_MESSAGE_BYTES {
        return Err(Error::TooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn initialize_schema_defaults_and_wire_names() {
        let request: InitializeRequest = serde_json::from_str(r#"{"protocolVersion":1}"#).unwrap();
        assert_eq!(request.protocol_version, ProtocolVersion(1));
        assert!(!request.client_capabilities.fs.read_text_file);
        let response = InitializeResponse::for_request(&request, "test");
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["protocolVersion"], 1);
        assert_eq!(value["agentCapabilities"]["loadSession"], false);
        assert_eq!(
            value["agentCapabilities"]["promptCapabilities"]["embeddedContext"],
            false
        );
        assert_eq!(value["authMethods"], serde_json::json!([]));
    }

    #[test]
    fn protocol_version_rejects_out_of_range_values() {
        for input in [
            r#"{"protocolVersion":-1}"#,
            r#"{"protocolVersion":65536}"#,
            r#"{"protocolVersion":1.5}"#,
        ] {
            assert!(serde_json::from_str::<InitializeRequest>(input).is_err());
        }
    }

    #[test]
    fn initialize_preserves_meta_and_unknown_fields() {
        let request: InitializeRequest =
            serde_json::from_str(r#"{"protocolVersion":1,"_meta":{"x":true},"future":42}"#)
                .unwrap();
        assert_eq!(request.meta.unwrap()["x"], true);
    }

    #[test]
    fn round_trip_and_blank_lines() {
        let message = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize"});
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).unwrap();
        let mut reader = Cursor::new(format!("\n{}", String::from_utf8(bytes).unwrap()));
        assert_eq!(read_message(&mut reader).unwrap(), Some(message));
    }

    #[test]
    fn reject_wrong_version_and_oversize() {
        let mut reader = Cursor::new(
            br#"{"jsonrpc":"1.0"}
"#
            .to_vec(),
        );
        assert!(matches!(
            read_message(&mut reader),
            Err(Error::WrongVersion)
        ));
        let mut reader = Cursor::new(vec![b'x'; MAX_MESSAGE_BYTES + 1]);
        assert!(matches!(read_message(&mut reader), Err(Error::TooLarge)));
    }

    #[test]
    fn accepts_crlf_multiple_and_eof_frames() {
        let mut reader = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"method\":\"a\"}\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}".to_vec());
        assert!(read_message(&mut reader).is_ok());
        assert!(read_message(&mut reader).is_ok());
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn rejects_malformed_envelopes_and_writer_keeps_boundary() {
        for input in [
            br#"[]
"#
            .as_slice(),
            br#"{"jsonrpc":"2.0","method":1}
"#
            .as_slice(),
            br#"{"jsonrpc":"2.0","id":1}
"#
            .as_slice(),
            br#"{"jsonrpc":"2.0","id":1,"result":null,"error":{}}
"#
            .as_slice(),
        ] {
            let mut reader = Cursor::new(input);
            assert!(read_message(&mut reader).is_err());
        }
        let mut out = Vec::new();
        write_message(&mut out, &serde_json::json!({"jsonrpc":"2.0","method":"x"})).unwrap();
        assert_eq!(out.last(), Some(&b'\n'));
    }
}
