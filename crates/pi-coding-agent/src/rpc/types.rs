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
    SetSteeringMode,
    SetFollowUpMode,
    SetThinkingLevel,
    CycleThinkingLevel,
    GetAvailableThinkingLevels,
}

impl RpcCommand {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Steer => "steer",
            Self::FollowUp => "follow_up",
            Self::Abort => "abort",
            Self::NewSession => "new_session",
            Self::GetState => "get_state",
            Self::SetSteeringMode => "set_steering_mode",
            Self::SetFollowUpMode => "set_follow_up_mode",
            Self::SetThinkingLevel => "set_thinking_level",
            Self::CycleThinkingLevel => "cycle_thinking_level",
            Self::GetAvailableThinkingLevels => "get_available_thinking_levels",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RpcQueueMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RpcThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl From<RpcQueueMode> for pi_agent::QueueMode {
    fn from(mode: RpcQueueMode) -> Self {
        match mode {
            RpcQueueMode::All => pi_agent::QueueMode::All,
            RpcQueueMode::OneAtATime => pi_agent::QueueMode::OneAtATime,
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
    #[serde(default)]
    pub mode: Option<RpcQueueMode>,
    #[serde(default)]
    pub level: Option<RpcThinkingLevel>,
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
            "set_steering_mode" => Ok(RpcCommand::SetSteeringMode),
            "set_follow_up_mode" => Ok(RpcCommand::SetFollowUpMode),
            "set_thinking_level" => Ok(RpcCommand::SetThinkingLevel),
            "cycle_thinking_level" => Ok(RpcCommand::CycleThinkingLevel),
            "get_available_thinking_levels" => Ok(RpcCommand::GetAvailableThinkingLevels),
            other => Err(format!("Unknown command: {other}")),
        }
    }

    pub fn text(&self) -> Result<&str, String> {
        self.message
            .as_deref()
            .filter(|message| !message.is_empty())
            .ok_or_else(|| "message must be non-empty text".into())
    }

    pub fn queue_mode(&self) -> Result<RpcQueueMode, String> {
        self.mode.ok_or_else(|| "mode is required".into())
    }

    pub fn thinking_level(&self) -> Result<RpcThinkingLevel, String> {
        self.level.ok_or_else(|| "level is required".into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
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

    pub fn ok_with_data(id: Option<String>, command: impl Into<String>, data: Value) -> Self {
        Self {
            id,
            kind: "response",
            command: command.into(),
            success: true,
            error: None,
            data: Some(data),
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
        assert!(value.get("id").is_none());
        assert!(value.get("error").is_none());

        let value_with_id =
            serde_json::to_value(RpcResponse::ok(Some("req-1".into()), "get_state")).unwrap();
        assert_eq!(value_with_id["id"], "req-1");
    }

    #[test]
    fn queue_mode_commands_deserialize() {
        let steer_req: RpcRequest =
            serde_json::from_str(r#"{"type":"set_steering_mode","id":"1","mode":"all"}"#).unwrap();
        assert_eq!(steer_req.command_kind(), Ok(RpcCommand::SetSteeringMode));
        assert_eq!(steer_req.queue_mode(), Ok(RpcQueueMode::All));

        let followup_req: RpcRequest =
            serde_json::from_str(r#"{"type":"set_follow_up_mode","mode":"one-at-a-time"}"#)
                .unwrap();
        assert_eq!(followup_req.command_kind(), Ok(RpcCommand::SetFollowUpMode));
        assert_eq!(followup_req.queue_mode(), Ok(RpcQueueMode::OneAtATime));
    }

    #[test]
    fn invalid_queue_mode_rejected() {
        assert!(serde_json::from_str::<RpcRequest>(
            r#"{"type":"set_steering_mode","mode":"invalid"}"#
        )
        .is_err());
    }

    #[test]
    fn get_state_response_has_data() {
        let st = RpcState {
            model: "test-model".into(),
            thinking_level: "off".into(),
            is_streaming: false,
            is_compacting: false,
            steering_mode: "one-at-a-time".into(),
            follow_up_mode: "one-at-a-time".into(),
            session_id: "s1".into(),
            message_count: 0,
            pending_message_count: 0,
        };
        let resp = RpcResponse::state(Some("req-1".into()), st);
        let val = serde_json::to_value(resp).unwrap();
        assert_eq!(val["success"], true);
        assert_eq!(val["data"]["model"], "test-model");
        assert_eq!(val["data"]["steeringMode"], "one-at-a-time");
        assert_eq!(val["data"]["followUpMode"], "one-at-a-time");
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
    fn empty_messages_rejected() {
        for json in [r#"{"type":"prompt"}"#, r#"{"type":"steer","message":""}"#] {
            let request: RpcRequest = serde_json::from_str(json).unwrap();
            assert!(request.text().is_err());
        }
    }

    #[test]
    fn invalid_message_type_rejected() {
        assert!(serde_json::from_str::<RpcRequest>(r#"{"type":"prompt","message":3}"#).is_err());
    }
}
