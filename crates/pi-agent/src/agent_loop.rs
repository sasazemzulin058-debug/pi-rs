//! Agent loop — Rust port of `packages/agent/src/agent-loop.ts`.
//!
//! Streams assistant deltas, executes tool calls, and surfaces permission
//! decisions. Cancellation is honored via `StreamOptions::cancel`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::StreamExt;
use pi_ai::{AssistantMessageEvent, Content, Context, Message, StopReason, ToolResultMessage};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::instrument;

use crate::error::{AgentError, Result};
use crate::types::{AgentConfig, AgentEvent, AgentTool, AgentToolResult, PermissionDecision};

pub struct AgentRun {
    pub messages: Vec<Message>,
    pub stopped_at_turn_limit: bool,
}

#[instrument(skip(config, initial_prompt, events), fields(model = %config.model.id))]
pub async fn run_agent(
    config: &AgentConfig,
    initial_prompt: Message,
    events: Option<mpsc::UnboundedSender<AgentEvent>>,
) -> Result<AgentRun> {
    run_agent_with_history(config, vec![initial_prompt], events).await
}

/// Continue a run with an existing transcript. Use this for `pi-rs --resume`.
pub async fn run_agent_with_history(
    config: &AgentConfig,
    mut messages: Vec<Message>,
    events: Option<mpsc::UnboundedSender<AgentEvent>>,
) -> Result<AgentRun> {
    if let Some(last) = messages.last().cloned() {
        emit(&events, AgentEvent::UserMessage { message: last });
    }
    emit(&events, AgentEvent::AgentStart);

    let tool_index: HashMap<String, Arc<dyn AgentTool>> = config
        .tools
        .iter()
        .map(|t| (t.name().to_string(), t.clone()))
        .collect();
    let tool_defs: Vec<pi_ai::Tool> = config
        .tools
        .iter()
        .map(|t| crate::types::tool_def(t.as_ref()))
        .collect();

    let mut session_allowed: HashSet<String> = HashSet::new();
    let mut turn: u32 = 0;
    let mut stopped_at_turn_limit = false;

    'outer: while turn < config.runtime_limits.max_turns {
        turn += 1;
        emit(&events, AgentEvent::TurnStart);

        let mut compaction_retried = false;
        let msg = 'attempt: loop {
            let ctx = Context {
                system_prompt: Some(config.system_prompt.clone()),
                messages: messages.clone(),
                tools: tool_defs.clone(),
            };
            let mut options = config.stream_options.clone();
            if options.reasoning.is_none() && config.thinking_level != pi_ai::ThinkingLevel::Off {
                options.reasoning = Some(config.thinking_level);
            }
            let mut stream = match config
                .provider_factory
                .stream(&config.model, &ctx, &options)
                .await
            {
                Ok(stream) => stream,
                Err(error) if !compaction_retried && error.is_context_overflow() => {
                    if compact_messages(&mut messages) {
                        compaction_retried = true;
                        emit(&events, AgentEvent::AutoCompacted);
                        continue;
                    } else {
                        return Err(error.into());
                    }
                }
                Err(error) => return Err(error.into()),
            };
            let mut final_message = None;
            let mut stop = StopReason::Stop;
            let mut has_emitted_deltas = false;
            while let Some(ev) = stream.next().await {
                let ev = match ev {
                    Ok(ev) => ev,
                    Err(error) if !compaction_retried && error.is_context_overflow() => {
                        if compact_messages(&mut messages) {
                            compaction_retried = true;
                            if has_emitted_deltas {
                                emit(&events, AgentEvent::RetryReset);
                            }
                            emit(&events, AgentEvent::AutoCompacted);
                            continue 'attempt;
                        } else {
                            return Err(error.into());
                        }
                    }
                    Err(error) => return Err(error.into()),
                };
                match ev {
                    AssistantMessageEvent::Done { reason, message } => {
                        stop = reason;
                        final_message = Some(message);
                        break;
                    }
                    AssistantMessageEvent::Error { error, .. } => {
                        let overflow = error
                            .error_message
                            .as_deref()
                            .is_some_and(pi_ai::error::is_context_overflow_message);
                        if !compaction_retried && overflow {
                            if compact_messages(&mut messages) {
                                compaction_retried = true;
                                if has_emitted_deltas {
                                    emit(&events, AgentEvent::RetryReset);
                                }
                                emit(&events, AgentEvent::AutoCompacted);
                                continue 'attempt;
                            } else {
                                return Err(AgentError::Other(
                                    error
                                        .error_message
                                        .unwrap_or_else(|| "provider error".into()),
                                ));
                            }
                        }
                        return Err(AgentError::Other(
                            error
                                .error_message
                                .unwrap_or_else(|| "provider error".into()),
                        ));
                    }
                    AssistantMessageEvent::TextDelta {
                        content_index: _,
                        delta,
                    } => {
                        has_emitted_deltas = true;
                        emit(&events, AgentEvent::TextDelta { delta });
                    }
                    AssistantMessageEvent::ThinkingDelta {
                        content_index: _,
                        delta,
                    } => {
                        has_emitted_deltas = true;
                        emit(&events, AgentEvent::ThinkingDelta { delta });
                    }
                    _ => {}
                }
            }
            if let Some(message) = final_message {
                break (message, stop);
            }
            return Err(AgentError::Other(
                "provider stream produced no terminal event".into(),
            ));
        };
        let (msg, stop) = msg;

        let assistant_message = Message::Assistant(msg.clone());
        messages.push(assistant_message.clone());
        emit(
            &events,
            AgentEvent::AssistantMessage {
                message: assistant_message,
            },
        );

        let tool_calls: Vec<(String, String, Value)> = msg
            .content
            .iter()
            .filter_map(|c| match c {
                Content::ToolCall {
                    id,
                    name,
                    arguments,
                } => Some((id.clone(), name.clone(), arguments.clone())),
                _ => None,
            })
            .collect();

        if tool_calls.is_empty() || stop != StopReason::ToolUse {
            emit(&events, AgentEvent::TurnEnd);
            break 'outer;
        }

        let mut any_terminate = !tool_calls.is_empty();
        for (id, name, args) in tool_calls {
            // Permission gate (only for tools that require it, and only once
            // per name per run if the user said "allow session").
            let tool_obj = tool_index.get(&name);
            let needs_perm = tool_obj.map(|t| t.requires_permission()).unwrap_or(false)
                && !session_allowed.contains(&name);
            if needs_perm {
                match config.permission.check(&name, &args).await {
                    PermissionDecision::Allow => {}
                    PermissionDecision::AllowSession => {
                        session_allowed.insert(name.clone());
                    }
                    PermissionDecision::Deny { reason } => {
                        emit(
                            &events,
                            AgentEvent::PermissionDenied {
                                tool_name: name.clone(),
                                reason: reason.clone(),
                            },
                        );
                        let tr = ToolResultMessage {
                            tool_call_id: id,
                            tool_name: name,
                            content: vec![Content::text(format!("permission denied: {reason}"))],
                            is_error: true,
                            timestamp: pi_ai::now_ms(),
                        };
                        messages.push(Message::ToolResult(tr));
                        any_terminate = false;
                        continue;
                    }
                }
            }

            emit(
                &events,
                AgentEvent::ToolExecutionStart {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    args: args.clone(),
                },
            );
            let (content, is_error, terminate) = match tool_obj {
                Some(tool) => match tool.execute(&id, args).await {
                    Ok(AgentToolResult {
                        content,
                        details: _,
                        terminate,
                    }) => (content, false, terminate),
                    Err(e) => (vec![Content::text(format!("tool error: {e}"))], true, false),
                },
                None => (
                    vec![Content::text(format!("unknown tool: {name}"))],
                    true,
                    false,
                ),
            };
            if !terminate {
                any_terminate = false;
            }
            emit(
                &events,
                AgentEvent::ToolExecutionEnd {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    is_error,
                    content: content.clone(),
                },
            );
            let tr = ToolResultMessage {
                tool_call_id: id,
                tool_name: name,
                content,
                is_error,
                timestamp: pi_ai::now_ms(),
            };
            messages.push(Message::ToolResult(tr));
        }
        emit(&events, AgentEvent::TurnEnd);
        if any_terminate {
            break;
        }
    }

    if turn >= config.runtime_limits.max_turns {
        stopped_at_turn_limit = true;
    }

    emit(
        &events,
        AgentEvent::AgentEnd {
            messages: messages.clone(),
        },
    );
    Ok(AgentRun {
        messages,
        stopped_at_turn_limit,
    })
}

fn compact_messages(messages: &mut Vec<Message>) -> bool {
    // Preserve valid groups:
    // UserMessage must precede AssistantMessage, AssistantMessage with ToolCall must precede ToolResult.
    // If we have history, we must keep at least the initial user query and the last turn.
    // Let's implement a safe compaction that preserves protocol groups and reduces history.
    // If messages.len() <= 2, we cannot really reduce it further without losing the initial query/context.
    if messages.len() <= 2 {
        return false;
    }

    // We want to reduce. The simplest robust strategy is:
    // Keep the very first message (usually the initial user instruction).
    // Find the latest message sequence that is valid (assistant + tool result if tool execution was ongoing, or just assistant).
    // Specifically, let's keep the first message.
    // Let's keep the last N messages that form a complete protocol unit.
    // If the last message is a ToolResult, we MUST also keep the preceding Assistant message that has the ToolCall.
    // Let's scan backwards:
    let mut indices_to_keep = std::collections::BTreeSet::new();
    indices_to_keep.insert(0); // Always keep the initial message

    let len = messages.len();
    if len > 1 {
        let last_idx = len - 1;
        indices_to_keep.insert(last_idx);

        // If the last message is a ToolResult, find all other ToolResult messages or the Assistant message that started the tool calls.
        // Actually, we can look at the sequence backwards and find the last Assistant message and all ToolResults that follow it.
        if let Message::ToolResult(tr) = &messages[last_idx] {
            // Find the preceding Assistant message that contains this tool_call_id
            let mut found_assistant = false;
            for i in (0..last_idx).rev() {
                if let Message::Assistant(a) = &messages[i] {
                    if a.content.iter().any(|c| match c {
                        Content::ToolCall { id, .. } => id == &tr.tool_call_id,
                        _ => false,
                    }) {
                        indices_to_keep.insert(i);
                        // Also keep any other tool results for tool calls in this assistant message to keep it balanced, or just keep all messages from that assistant message onwards.
                        for j in i..len {
                            indices_to_keep.insert(j);
                        }
                        found_assistant = true;
                        break;
                    }
                }
            }
            if !found_assistant {
                // Fallback: just keep the last assistant message
                for i in (0..last_idx).rev() {
                    if let Message::Assistant(_) = &messages[i] {
                        indices_to_keep.insert(i);
                        break;
                    }
                }
            }
        }
    }

    // If indices_to_keep contains all messages, we didn't reduce anything!
    if indices_to_keep.len() >= messages.len() {
        return false;
    }

    let old_len = messages.len();
    let mut kept_messages = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if indices_to_keep.contains(&i) {
            kept_messages.push(msg.clone());
        }
    }

    // Combine adjacent User messages after dropping intermediate turns to preserve valid schema
    let mut new_messages = Vec::new();
    for msg in kept_messages {
        if let Some(Message::User {
            content: last_content,
            ..
        }) = new_messages.last_mut()
        {
            if let Message::User {
                content: new_content,
                ..
            } = &msg
            {
                last_content.extend(new_content.clone());
                continue;
            }
        }
        new_messages.push(msg);
    }

    *messages = new_messages;
    messages.len() < old_len
}

fn emit(sink: &Option<mpsc::UnboundedSender<AgentEvent>>, ev: AgentEvent) {
    if let Some(s) = sink {
        let _ = s.send(ev);
    }
}
