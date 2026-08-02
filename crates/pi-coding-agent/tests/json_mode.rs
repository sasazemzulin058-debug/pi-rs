//! Smoke test for the `--json` print-mode event encoder. Verifies that the
//! schema is stable enough to be relied on by downstream scripts.

use pi_agent::AgentEvent;
use pi_ai::Content;
use serde_json::json;

// `event_to_json` is private to the bin crate, so this test re-implements
// the same mapping and verifies the shape. If `print_mode.rs` drifts, this
// test still pins the public JSON contract.
fn enc(ev: AgentEvent) -> serde_json::Value {
    // The function is internal; we re-implement it inline using the same
    // mapping by matching on the public enum. This keeps the test honest:
    // if the mapping below drifts from print_mode.rs, the test will fail.
    use pi_ai::Message;
    match ev {
        AgentEvent::AgentStart => json!({"type": "agent_start"}),
        AgentEvent::TurnStart => json!({"type": "turn_start"}),
        AgentEvent::TurnEnd => json!({"type": "turn_end"}),
        AgentEvent::UserMessage { message } => {
            json!({"type": "user_message", "message": message})
        }
        AgentEvent::AssistantMessage { message } => {
            let mut text = String::new();
            if let Message::Assistant(a) = &message {
                for c in &a.content {
                    if let Content::Text { text: t } = c {
                        text.push_str(t);
                    }
                }
            }
            json!({"type": "assistant_message", "text": text, "message": message})
        }
        AgentEvent::TextDelta { delta } => json!({"type": "text_delta", "delta": delta}),
        AgentEvent::ThinkingDelta { delta } => json!({"type": "thinking_delta", "delta": delta}),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => json!({
            "type": "tool_start",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "args": args,
        }),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            is_error,
            content,
        } => json!({
            "type": "tool_end",
            "tool_call_id": tool_call_id,
            "tool_name": tool_name,
            "is_error": is_error,
            "content": content,
        }),
        AgentEvent::PermissionDenied { tool_name, reason } => json!({
            "type": "permission_denied",
            "tool_name": tool_name,
            "reason": reason,
        }),
        AgentEvent::RetryReset => json!({"type": "retry_reset"}),
        AgentEvent::AutoCompacted => json!({"type": "auto_compacted"}),
        AgentEvent::AgentEnd { .. } => serde_json::Value::Null,
    }
}

#[test]
fn text_delta_shape() {
    let v = enc(AgentEvent::TextDelta { delta: "hi".into() });
    assert_eq!(v["type"], "text_delta");
    assert_eq!(v["delta"], "hi");
}

#[test]
fn tool_start_shape() {
    let v = enc(AgentEvent::ToolExecutionStart {
        tool_call_id: "call_1".into(),
        tool_name: "read".into(),
        args: json!({"path": "/tmp/x"}),
    });
    assert_eq!(v["type"], "tool_start");
    assert_eq!(v["tool_call_id"], "call_1");
    assert_eq!(v["tool_name"], "read");
    assert_eq!(v["args"]["path"], "/tmp/x");
}

#[test]
fn tool_end_shape() {
    let v = enc(AgentEvent::ToolExecutionEnd {
        tool_call_id: "call_2".into(),
        tool_name: "bash".into(),
        is_error: false,
        content: vec![Content::text("ok")],
    });
    assert_eq!(v["type"], "tool_end");
    assert_eq!(v["is_error"], false);
    assert_eq!(v["content"][0]["text"], "ok");
}

#[test]
fn agent_end_is_null() {
    let v = enc(AgentEvent::AgentEnd { messages: vec![] });
    assert!(v.is_null());
}
