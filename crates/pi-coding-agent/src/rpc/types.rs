//! Wire types for bounded U5 RPC subset.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcCommand {
    Prompt,
    Steer,
    FollowUp,
    Abort,
    NewSession,
    GetState,
}

impl RpcCommand {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
            Self::Abort => "abort",
            Self::NewSession => "new_session",
            Self::GetState => "get_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcRequest {
    #[serde(rename = "type")]
    pub command: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub streaming_behavior: Option<StreamingBehavior>,
}

impl RpcRequest {
    pub fn command_kind(&self) -> Result<RpcCommand, String> {
        match self.command.as_str() {
            "prompt" => Ok(RpcCommand::Prompt),
            "steer" => Ok(RpcCommand::Steer),
            "follow_up" => Ok(RpcCommand::FollowUp),
            "abort" => Ok(RpcCommand::Abort),
            "new_session" => Ok(RpcCommand::NewSession),
            "get_state" => Ok(RpcCommand::GetState),
            other => Err(format!("Unknown command: {other}")),
        }
    }

    pub fn text(&self) -> Result<&str, String> {
        self.message
            .as_deref()
            .filter(|message| !message.is_empty())
            .ok_or_else(|| "message must be non-empty text".into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub command: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcResponse {
    pub fn ok(id: Option<String>, command: impl Into<String>) -> Self {
        Self {
            id,
            kind: "response",
            command: command.into(),
            success: true,
            error: None,
            data: None,
        }
    }

    pub fn state(id: Option<String>, state: RpcState) -> Self {
        let state_val = serde_json::to_value(&state).ok();
        Self {
            id,
            kind: "response",
            command: "get_state".into(),
            success: true,
            error: None,
            data: state_val,
        }
    }

    pub fn error(id: Option<String>, command: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            id,
            kind: "response",
            command: command.into(),
            success: false,
            error: Some(error.into()),
            data: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcState {
    pub model: String,
    pub thinking_level: String,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: String,
    pub follow_up_mode: String,
    pub session_id: String,
    pub message_count: usize,
    pub pending_message_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_wire_shape() {
        let request: RpcRequest = serde_json::from_str(r#"{"type":"get_state","id":"x"}"#).unwrap();
        assert_eq!(request.command_kind(), Ok(RpcCommand::GetState));
        let value = serde_json::to_value(RpcResponse::ok(None, "get_state")).unwrap();
        assert_eq!(value["type"], "response");
        assert!(value.get("id").is_some());
        assert!(value.get("error").is_none());
    }

    #[test]
    fn get_state_response_has_data() {
        let st = RpcState {
            model: "test-model".into(),
            thinking_level: "off".into(),
            is_streaming: false,
            is_compacting: false,
            steering_mode: "default".into(),
            follow_up_mode: "default".into(),
            session_id: "s1".into(),
            message_count: 0,
            pending_message_count: 0,
        };
        let resp = RpcResponse::state(Some("req-1".into()), st);
        let val = serde_json::to_value(resp).unwrap();
        assert_eq!(val["success"], true);
        assert_eq!(val["data"]["model"], "test-model");
        assert!(val.get("state").is_none());
    }

    #[test]
    fn invalid_streaming_behavior_rejected() {
        assert!(serde_json::from_str::<RpcRequest>(
            r#"{"type":"prompt","streamingBehavior":"invalid"}"#
        )
        .is_err());
        let parsed: RpcRequest =
            serde_json::from_str(r#"{"type":"prompt","streamingBehavior":"steer"}"#).unwrap();
        assert_eq!(parsed.streaming_behavior, Some(StreamingBehavior::Steer));
    }

    #[test]
    fn invalid_message_type_rejected() {
        assert!(serde_json::from_str::<RpcRequest>(r#"{"type":"prompt","message":3}"#).is_err());
    }
}
