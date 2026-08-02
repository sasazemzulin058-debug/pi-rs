//! One-shot "print" mode: run a single prompt to completion and print the
//! result. Streams text deltas to stdout as they arrive.
//!
//! Two output formats:
//! - human (default): assistant text streams to stdout; tool activity goes to stderr.
//! - JSON-lines (`--json`): one JSON object per line on stdout for every
//!   significant agent event. Stable contract for scripting.

use std::io::Write;
use std::sync::Arc;

use pi_agent::{run_agent, tools::default_tools, AgentConfig, AgentEvent, PermissionPolicy};
use pi_ai::{Content, Message};
use serde_json::json;
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::session::Session;
use crate::system_prompt::build_system_prompt;

pub async fn run_print(
    app: &AppConfig,
    prompt: String,
    permission: Arc<dyn PermissionPolicy>,
    json_mode: bool,
    trust_decision: crate::trust::TrustDecision,
    initial: Option<crate::session::Session>,
) -> anyhow::Result<()> {
    let cfg = AgentConfig::new(
        app.model.clone(),
        build_system_prompt(&app.config_dir, trust_decision),
    )
    .with_tools(default_tools())
    .with_max_turns(app.max_turns)
    .with_thinking(app.thinking_level)
    .with_permission(permission);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = Message::user_text(prompt);
    let mut session = initial.unwrap_or_else(|| Session::new(&app.model));
    let history = if session.messages.is_empty() {
        None
    } else {
        let mut messages = session.messages.clone();
        messages.push(user.clone());
        Some(messages)
    };

    let cfg_cloned = cfg.clone();
    let handle = tokio::spawn(async move {
        match history {
            Some(messages) => {
                pi_agent::run_agent_with_history(&cfg_cloned, messages, Some(tx)).await
            }
            None => run_agent(&cfg_cloned, user, Some(tx)).await,
        }
    });

    if json_mode {
        run_json(&mut rx).await;
    } else {
        run_human(&mut rx).await;
    }

    let res = handle.await??;
    session.replace_messages(res.messages.clone());
    crate::session::save_jsonl(
        &app.config_dir
            .join("sessions")
            .join(format!("{}.jsonl", session.id)),
        &session,
    )?;
    if json_mode {
        emit_agent_end(res.stopped_at_turn_limit, res.messages.len());
    } else if res.stopped_at_turn_limit {
        eprintln!("(stopped at max turns)");
    }
    Ok(())
}

async fn run_human(rx: &mut mpsc::UnboundedReceiver<AgentEvent>) {
    let mut stdout = std::io::stdout();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta { delta } => {
                let _ = write!(stdout, "{delta}");
                let _ = stdout.flush();
            }
            AgentEvent::AssistantMessage { .. } => {
                let _ = writeln!(stdout);
            }
            AgentEvent::ToolExecutionStart {
                tool_name, args, ..
            } => {
                eprintln!("→ {tool_name}({args})");
            }
            AgentEvent::ToolExecutionEnd {
                tool_name,
                is_error,
                ..
            } => {
                eprintln!("← {} {}", tool_name, if is_error { "error" } else { "ok" });
            }
            AgentEvent::PermissionDenied { tool_name, reason } => {
                eprintln!("✗ permission denied for {tool_name}: {reason}");
            }
            _ => {}
        }
    }
}

async fn run_json(rx: &mut mpsc::UnboundedReceiver<AgentEvent>) {
    let mut stdout = std::io::stdout();
    run_json_to(rx, &mut stdout).await;
}

async fn run_json_to<W: Write + ?Sized>(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
    writer: &mut W,
) {
    while let Some(ev) = rx.recv().await {
        let value = event_to_json(&ev);
        if !value.is_null() {
            emit_json_to(&value, writer);
        }
    }
}

fn event_to_json(ev: &AgentEvent) -> serde_json::Value {
    match ev {
        AgentEvent::AgentStart => json!({"type": "agent_start"}),
        AgentEvent::TurnStart => json!({"type": "turn_start"}),
        AgentEvent::TurnEnd => json!({"type": "turn_end"}),
        AgentEvent::UserMessage { message } => {
            json!({"type": "user_message", "message": message})
        }
        AgentEvent::AssistantMessage { message } => {
            let mut text = String::new();
            if let Message::Assistant(a) = message {
                for c in &a.content {
                    if let Content::Text { text: t } = c {
                        text.push_str(t);
                    }
                }
            }
            json!({"type": "assistant_message", "text": text, "message": message})
        }
        AgentEvent::TextDelta { delta } => {
            json!({"type": "text_delta", "delta": delta})
        }
        AgentEvent::ThinkingDelta { delta } => {
            json!({"type": "thinking_delta", "delta": delta})
        }
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
        // AgentEnd is emitted by run_print after the channel closes so we know
        // the final state (stopped_at_turn_limit, etc.).
        AgentEvent::AgentEnd { .. } => serde_json::Value::Null,
    }
}

fn emit_agent_end(stopped_at_turn_limit: bool, message_count: usize) {
    let mut stdout = std::io::stdout();
    emit_agent_end_to(stopped_at_turn_limit, message_count, &mut stdout);
}

fn emit_agent_end_to<W: Write + ?Sized>(
    stopped_at_turn_limit: bool,
    message_count: usize,
    writer: &mut W,
) {
    emit_json_to(
        &json!({
            "type": "agent_end",
            "stopped_at_turn_limit": stopped_at_turn_limit,
            "message_count": message_count,
        }),
        writer,
    );
}

fn emit_json_to<W: Write + ?Sized>(value: &serde_json::Value, writer: &mut W) {
    let line = match serde_json::to_string(value) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = writeln!(writer, "{line}");
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn json_events_are_lines_and_end_is_terminal() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(AgentEvent::AgentStart).unwrap();
        tx.send(AgentEvent::TextDelta {
            delta: "hello\nworld".into(),
        })
        .unwrap();
        tx.send(AgentEvent::AgentEnd { messages: vec![] }).unwrap();
        drop(tx);

        let mut output = Vec::new();
        run_json_to(&mut rx, &mut output).await;
        emit_agent_end_to(false, 2, &mut output);
        let lines: Vec<_> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0]).unwrap()["type"],
            "agent_start"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[1]).unwrap()["type"],
            "text_delta"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[2]).unwrap()["type"],
            "agent_end"
        );
        assert!(!std::str::from_utf8(&output)
            .unwrap()
            .contains("hello\nworld"));
        assert!(output.ends_with(b"\n"));
    }

    #[test]
    fn json_renderer_emits_final_agent_end_shape() {
        let mut output = Vec::new();
        emit_agent_end_to(false, 2, &mut output);
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["type"], "agent_end");
        assert_eq!(value["message_count"], 2);
    }

    #[test]
    fn json_renderer_does_not_write_human_output() {
        let mut output = Vec::new();
        emit_json_to(
            &json!({"type": "assistant_message", "text": "ok"}),
            &mut output,
        );
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value, json!({"type": "assistant_message", "text": "ok"}));
        assert!(!std::str::from_utf8(&output).unwrap().contains("→"));
    }
}
