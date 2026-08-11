use pi_agent::{AgentConfig, AgentEvent, AgentSession, PermissionPolicy, QueueMode, SessionPhase};
use pi_ai::Message;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::mpsc;
use tracing::error;

use crate::config::AppConfig;

const RPC_OUTPUT_CAPACITY: usize = 256;

macro_rules! send_rpc {
    ($sender:expr, $value:expr) => {
        $sender.send($value).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                format!("RPC output failed: {error}"),
            )
        })?;
    };
}

#[derive(Clone)]
struct OutputSender {
    tx: mpsc::Sender<serde_json::Value>,
    error_tx: mpsc::Sender<std::io::Error>,
}

impl OutputSender {
    fn send(&self, value: serde_json::Value) -> std::io::Result<()> {
        match self.tx.try_send(value) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                let _ = self.error_tx.try_send(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "RPC stdout backpressure limit exceeded",
                ));
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "RPC stdout backpressure limit exceeded",
                ))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "RPC stdout closed",
            )),
        }
    }
}

use crate::rpc::transport::{read_record, write_record, TransportError};
use crate::rpc::types::{
    RpcCommand, RpcRequest, RpcResponse, RpcState, RpcThinkingLevel, StreamingBehavior,
};
use crate::trust::{CanonicalProjectRoot, TrustDecision};

fn available_thinking_levels(model: &pi_ai::Model) -> Vec<pi_ai::ThinkingLevel> {
    if !model.reasoning {
        return vec![pi_ai::ThinkingLevel::Off];
    }
    vec![
        pi_ai::ThinkingLevel::Off,
        pi_ai::ThinkingLevel::Minimal,
        pi_ai::ThinkingLevel::Low,
        pi_ai::ThinkingLevel::Medium,
        pi_ai::ThinkingLevel::High,
        pi_ai::ThinkingLevel::Xhigh,
    ]
}

fn clamp_rpc_thinking_level(level: RpcThinkingLevel, model: &pi_ai::Model) -> pi_ai::ThinkingLevel {
    let available = available_thinking_levels(model);
    let target = match level {
        RpcThinkingLevel::Off => pi_ai::ThinkingLevel::Off,
        RpcThinkingLevel::Minimal => pi_ai::ThinkingLevel::Minimal,
        RpcThinkingLevel::Low => pi_ai::ThinkingLevel::Low,
        RpcThinkingLevel::Medium => pi_ai::ThinkingLevel::Medium,
        RpcThinkingLevel::High => pi_ai::ThinkingLevel::High,
        RpcThinkingLevel::Xhigh | RpcThinkingLevel::Max => pi_ai::ThinkingLevel::Xhigh,
    };
    if available.contains(&target) {
        target
    } else {
        *available.last().unwrap_or(&pi_ai::ThinkingLevel::Off)
    }
}

fn thinking_level_to_wire(level: pi_ai::ThinkingLevel) -> &'static str {
    match level {
        pi_ai::ThinkingLevel::Off => "off",
        pi_ai::ThinkingLevel::Minimal => "minimal",
        pi_ai::ThinkingLevel::Low => "low",
        pi_ai::ThinkingLevel::Medium => "medium",
        pi_ai::ThinkingLevel::High => "high",
        pi_ai::ThinkingLevel::Xhigh => "xhigh",
    }
}

/// Stateful RPC server processing JSONL requests from `reader` and writing JSONL responses/events to `writer`.
pub async fn run_server<R, W>(
    config: AppConfig,
    permission_policy: Arc<dyn PermissionPolicy>,
    trust_context: (TrustDecision, Option<CanonicalProjectRoot>),
    reader: R,
    writer: W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let system_prompt =
        crate::system_prompt::build_system_prompt(&config.config_dir, &trust_context);
    let tools = pi_agent::tools::default_tools();

    let mut agent_config = AgentConfig::new(config.model.clone(), system_prompt);
    agent_config.tools = tools;
    agent_config.permission = permission_policy;
    agent_config.runtime_limits.max_turns = config.max_turns;
    agent_config.thinking_level = config.thinking_level;

    run_server_with_agent_config(config, agent_config, reader, writer).await
}

pub(crate) async fn run_server_with_agent_config<R, W>(
    config: AppConfig,
    agent_config: AgentConfig,
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    // One task owns stdout. I/O failure is fatal, not a diagnostic-only event.
    let (output_tx, mut rx) = mpsc::channel::<serde_json::Value>(RPC_OUTPUT_CAPACITY);
    let (writer_error_tx, mut writer_error_rx) = mpsc::channel::<std::io::Error>(1);
    let tx = OutputSender {
        tx: output_tx,
        error_tx: writer_error_tx.clone(),
    };
    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = write_record(&mut writer, &msg).await {
                let _ = writer_error_tx.try_send(std::io::Error::new(e.kind(), e.to_string()));
                return Err(e);
            }
        }
        Ok::<(), std::io::Error>(())
    });

    // Session initialization
    let mut persisted = crate::session::Session::new(&config.model);
    let mut current_session_id = persisted.id.clone();
    let mut session = AgentSession::new(agent_config.clone(), Vec::new());

    // Track background run task. Join result owns terminal persistence boundary.
    struct RunOutcome {
        result: Result<(), pi_agent::AgentError>,
    }
    let mut current_run_task: Option<tokio::task::JoinHandle<RunOutcome>> = None;
    // Abort must not race run startup: AgentSession::cancel() only sees token after run enters provider.
    let mut current_run_started: Option<tokio::sync::oneshot::Receiver<()>> = None;
    let mut shutdown_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;

    loop {
        // Reap completed runs before accepting new work. This prevents lost join errors
        // and makes persistence deterministic at next-command boundaries.
        if current_run_task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            if let Some(task) = current_run_task.take() {
                match task.await {
                    Ok(outcome) => {
                        if let Err(error) = outcome.result {
                            error!("Agent session run error: {error}");
                        }
                        save_session(&config, &mut persisted, &session);
                    }
                    Err(error) => shutdown_error = Some(Box::new(error)),
                }
            }
        }
        if shutdown_error.is_some() {
            break;
        }

        let record = tokio::select! {
            result = read_record(&mut reader) => match result {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(TransportError::Oversized) => {
                    let err_resp = RpcResponse::error(None, "parse", "record exceeds maximum size");
                    send_rpc!(tx, serde_json::to_value(err_resp)?);
                    continue;
                }
                Err(TransportError::Utf8) => {
                    let err_resp = RpcResponse::error(None, "parse", "invalid UTF-8");
                    send_rpc!(tx, serde_json::to_value(err_resp)?);
                    continue;
                }
                Err(TransportError::Io(e)) => return Err(e.into()),
            },
            Some(error) = writer_error_rx.recv() => {
                shutdown_error = Some(Box::new(error));
                break;
            },
        };
        if shutdown_error.is_some() {
            break;
        }
        // Try parsing JSON Value first to keep ID on malformed/unknown commands
        let val: serde_json::Value = match serde_json::from_str(&record) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = RpcResponse::error(None, "parse", format!("JSON parse error: {e}"));
                send_rpc!(tx, serde_json::to_value(err_resp)?);
                continue;
            }
        };

        let request_id = val
            .get("id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let req: RpcRequest = match serde_json::from_value(val) {
            Ok(r) => r,
            Err(e) => {
                let err_resp =
                    RpcResponse::error(request_id, "parse", format!("Invalid request shape: {e}"));
                send_rpc!(tx, serde_json::to_value(err_resp)?);
                continue;
            }
        };

        let cmd = match req.command_kind() {
            Ok(c) => c,
            Err(err_msg) => {
                let err_resp = RpcResponse::error(request_id, req.command, err_msg);
                send_rpc!(tx, serde_json::to_value(err_resp)?);
                continue;
            }
        };

        let is_running = current_run_task.as_ref().is_some_and(|h| !h.is_finished());

        match cmd {
            RpcCommand::GetState => {
                let st = session.state();
                let rpc_state = RpcState {
                    model: config.model.id.clone(),
                    thinking_level: thinking_level_to_wire(session.thinking_level()).into(),
                    is_streaming: is_running,
                    is_compacting: st.phase == SessionPhase::Compacting,
                    steering_mode: match st.steering_mode {
                        QueueMode::All => "all".into(),
                        QueueMode::OneAtATime => "one-at-a-time".into(),
                    },
                    follow_up_mode: match st.followup_mode {
                        QueueMode::All => "all".into(),
                        QueueMode::OneAtATime => "one-at-a-time".into(),
                    },
                    session_id: current_session_id.clone(),
                    message_count: st.messages.len(),
                    pending_message_count: st.input_queue.len() + st.steering_queue.len(),
                };
                let resp = RpcResponse::state(request_id, rpc_state);
                send_rpc!(tx, serde_json::to_value(resp)?);
            }
            RpcCommand::Prompt => {
                let text = match req.text() {
                    Ok(t) => t,
                    Err(e) => {
                        let resp = RpcResponse::error(request_id, "prompt", e);
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }
                };

                let msg = Message::user_text(text);

                if is_running {
                    match req.streaming_behavior {
                        Some(StreamingBehavior::Steer) => {
                            if let Err(e) = session.queue_steering(msg) {
                                let resp = RpcResponse::error(request_id, "prompt", e.to_string());
                                send_rpc!(tx, serde_json::to_value(resp)?);
                            } else {
                                let resp = RpcResponse::ok(request_id, "prompt");
                                send_rpc!(tx, serde_json::to_value(resp)?);
                            }
                        }
                        Some(StreamingBehavior::FollowUp) => {
                            if let Err(e) = session.queue_followup(msg) {
                                let resp = RpcResponse::error(request_id, "prompt", e.to_string());
                                send_rpc!(tx, serde_json::to_value(resp)?);
                            } else {
                                let resp = RpcResponse::ok(request_id, "prompt");
                                send_rpc!(tx, serde_json::to_value(resp)?);
                            }
                        }
                        None => {
                            let resp = RpcResponse::error(
                                request_id,
                                "prompt",
                                "agent active: streamingBehavior ('steer' or 'followUp') required",
                            );
                            send_rpc!(tx, serde_json::to_value(resp)?);
                        }
                    }
                } else {
                    if let Err(e) = session.queue_followup(msg) {
                        let resp = RpcResponse::error(request_id, "prompt", e.to_string());
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }

                    let resp = RpcResponse::ok(request_id, "prompt");
                    send_rpc!(tx, serde_json::to_value(resp)?);
                    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
                    let out_tx = tx.clone();
                    let (run_started_tx, run_started_rx) = tokio::sync::oneshot::channel();
                    current_run_started = Some(run_started_rx);

                    let sess = session.clone();
                    current_run_task = Some(tokio::spawn(async move {
                        let _ = run_started_tx.send(());
                        let forwarder_tx = out_tx.clone();
                        let forwarder = tokio::spawn(async move {
                            while let Some(evt) = event_rx.recv().await {
                                if let Ok(val) = map_agent_event_to_json(&evt) {
                                    if forwarder_tx.send(val).is_err() {
                                        return false;
                                    }
                                }
                            }
                            true
                        });

                        let run_res = sess.run(Some(event_tx.clone())).await;
                        // Close event stream before waiting forwarder; otherwise sender held here deadlocks.
                        drop(event_tx);
                        let output_ok = forwarder.await.unwrap_or(false);
                        if !output_ok {
                            return RunOutcome {
                                result: Err(pi_agent::AgentError::Other(
                                    "RPC output queue overflow or closed".into(),
                                )),
                            };
                        }

                        let (cancelled, outcome) = match run_res {
                            Ok(()) => {
                                if out_tx
                                    .send(serde_json::json!({
                                        "type": "event", "event": "run_complete"
                                    }))
                                    .is_err()
                                {
                                    return RunOutcome {
                                        result: Err(pi_agent::AgentError::Other(
                                            "RPC output queue overflow or closed".into(),
                                        )),
                                    };
                                }
                                (false, Ok(()))
                            }
                            Err(e) => {
                                let is_cancelled = is_cancellation_error(&e);
                                let err_msg = e.to_string();
                                let event = if is_cancelled {
                                    serde_json::json!({
                                        "type": "event", "event": "run_abort", "reason": err_msg,
                                    })
                                } else {
                                    serde_json::json!({
                                        "type": "event", "event": "run_error", "error": err_msg,
                                    })
                                };
                                if out_tx.send(event).is_err() {
                                    return RunOutcome {
                                        result: Err(pi_agent::AgentError::Other(
                                            "RPC output queue overflow or closed".into(),
                                        )),
                                    };
                                }
                                (is_cancelled, Err(e))
                            }
                        };
                        // Core cancellation path may omit Settlement. Synthesize one so
                        // every accepted run has protocol-visible terminal state.
                        let messages = sess.state().messages;
                        if out_tx
                            .send(serde_json::json!({
                                "type": "event", "event": "settlement",
                                "messages": messages, "cancelled": cancelled,
                            }))
                            .is_err()
                        {
                            return RunOutcome {
                                result: Err(pi_agent::AgentError::Other(
                                    "RPC output queue overflow or closed".into(),
                                )),
                            };
                        }
                        RunOutcome { result: outcome }
                    }));
                }
            }
            RpcCommand::Steer => {
                let text = match req.text() {
                    Ok(t) => t,
                    Err(e) => {
                        let resp = RpcResponse::error(request_id, "steer", e);
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }
                };
                let msg = Message::user_text(text);
                if let Err(e) = session.queue_steering(msg) {
                    let resp = RpcResponse::error(request_id, "steer", e.to_string());
                    send_rpc!(tx, serde_json::to_value(resp)?);
                } else {
                    let resp = RpcResponse::ok(request_id, "steer");
                    send_rpc!(tx, serde_json::to_value(resp)?);
                }
            }
            RpcCommand::FollowUp => {
                let text = match req.text() {
                    Ok(t) => t,
                    Err(e) => {
                        let resp = RpcResponse::error(request_id, "follow_up", e);
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }
                };
                let msg = Message::user_text(text);
                if let Err(e) = session.queue_followup(msg) {
                    let resp = RpcResponse::error(request_id, "follow_up", e.to_string());
                    send_rpc!(tx, serde_json::to_value(resp)?);
                } else {
                    let resp = RpcResponse::ok(request_id, "follow_up");
                    send_rpc!(tx, serde_json::to_value(resp)?);
                }
            }
            RpcCommand::Abort => {
                // Ack enters output queue before cancellation settlement.
                let resp = RpcResponse::ok(request_id, "abort");
                tx.send(serde_json::to_value(resp)?).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed")
                })?;
                if let Some(started) = current_run_started.take() {
                    let _ = started.await;
                }
                session.cancel();
            }
            RpcCommand::NewSession => {
                if let Some(started) = current_run_started.take() {
                    let _ = started.await;
                }
                session.cancel();
                if let Some(task) = current_run_task.take() {
                    let _ = task.await;
                }

                save_session(&config, &mut persisted, &session);
                persisted = crate::session::Session::new(&config.model);
                current_session_id = persisted.id.clone();
                let current_level = session.thinking_level();
                let mut new_agent_config = agent_config.clone();
                new_agent_config.thinking_level = current_level;
                session = AgentSession::new(new_agent_config, Vec::new());

                let resp = RpcResponse::ok(request_id, "new_session");
                send_rpc!(tx, serde_json::to_value(resp)?);
            }
            RpcCommand::SetSteeringMode => {
                let mode = match req.queue_mode() {
                    Ok(m) => m,
                    Err(e) => {
                        let resp = RpcResponse::error(request_id, "set_steering_mode", e);
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }
                };
                if let Err(e) = session.set_steering_mode(mode.into()) {
                    let resp = RpcResponse::error(request_id, "set_steering_mode", e.to_string());
                    send_rpc!(tx, serde_json::to_value(resp)?);
                } else {
                    let resp = RpcResponse::ok(request_id, "set_steering_mode");
                    send_rpc!(tx, serde_json::to_value(resp)?);
                }
            }
            RpcCommand::SetFollowUpMode => {
                let mode = match req.queue_mode() {
                    Ok(m) => m,
                    Err(e) => {
                        let resp = RpcResponse::error(request_id, "set_follow_up_mode", e);
                        send_rpc!(tx, serde_json::to_value(resp)?);
                        continue;
                    }
                };
                if let Err(e) = session.set_followup_mode(mode.into()) {
                    let resp = RpcResponse::error(request_id, "set_follow_up_mode", e.to_string());
                    send_rpc!(tx, serde_json::to_value(resp)?);
                } else {
                    let resp = RpcResponse::ok(request_id, "set_follow_up_mode");
                    send_rpc!(tx, serde_json::to_value(resp)?);
                }
            }
            RpcCommand::SetThinkingLevel => {
                let level = match req.thinking_level() {
                    Ok(l) => l,
                    Err(e) => {
                        send_rpc!(
                            tx,
                            serde_json::to_value(RpcResponse::error(
                                request_id,
                                "set_thinking_level",
                                e,
                            ))?
                        );
                        continue;
                    }
                };
                session.set_thinking_level(clamp_rpc_thinking_level(level, &config.model));
                send_rpc!(
                    tx,
                    serde_json::to_value(RpcResponse::ok(request_id, "set_thinking_level",))?
                );
            }
            RpcCommand::CycleThinkingLevel => {
                let available = available_thinking_levels(&config.model);
                if available.len() == 1 {
                    send_rpc!(
                        tx,
                        serde_json::to_value(RpcResponse::ok_with_data(
                            request_id,
                            "cycle_thinking_level",
                            serde_json::json!({"level": null}),
                        ))?
                    );
                } else {
                    let current = session.thinking_level();
                    let i = available.iter().position(|l| *l == current).unwrap_or(0);
                    let next = available[(i + 1) % available.len()];
                    session.set_thinking_level(next);
                    send_rpc!(
                        tx,
                        serde_json::to_value(RpcResponse::ok_with_data(
                            request_id,
                            "cycle_thinking_level",
                            serde_json::json!({"level": thinking_level_to_wire(next)}),
                        ))?
                    );
                }
            }
            RpcCommand::GetAvailableThinkingLevels => {
                let levels: Vec<_> = available_thinking_levels(&config.model)
                    .into_iter()
                    .map(thinking_level_to_wire)
                    .collect();
                send_rpc!(
                    tx,
                    serde_json::to_value(RpcResponse::ok_with_data(
                        request_id,
                        "get_available_thinking_levels",
                        serde_json::json!({"levels": levels}),
                    ))?
                );
            }
        }
    }

    // Clean shutdown: cancel active session, join run, persist settled state.
    if let Some(started) = current_run_started.take() {
        let _ = started.await;
    }
    session.cancel();
    if let Some(task) = current_run_task.take() {
        match task.await {
            Ok(outcome) => {
                if let Err(error) = outcome.result {
                    error!("Agent session run error: {error}");
                }
            }
            Err(error) => shutdown_error = Some(Box::new(error)),
        }
    }
    save_session(&config, &mut persisted, &session);

    drop(tx);
    let writer_result = writer_handle.await;
    if let Some(error) = shutdown_error {
        return Err(error);
    }
    match writer_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.into()),
        Err(e) => Err(Box::new(e)),
    }
}

fn is_cancellation_error(err: &pi_agent::AgentError) -> bool {
    matches!(
        err,
        pi_agent::AgentError::Cancelled | pi_agent::AgentError::Provider(pi_ai::Error::Cancelled)
    )
}

fn save_session(config: &AppConfig, persisted: &mut crate::session::Session, agent: &AgentSession) {
    persisted.replace_messages(agent.state().messages);
    if let Err(error) = crate::session::save(&config.config_dir, persisted) {
        eprintln!(
            "warning: failed to save RPC session {}: {error}",
            persisted.id
        );
    }
}

fn map_agent_event_to_json(evt: &AgentEvent) -> Result<serde_json::Value, serde_json::Error> {
    match evt {
        AgentEvent::PhaseChange { phase } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "phase_change",
            "phase": format!("{:?}", phase).to_lowercase(),
        })),
        AgentEvent::AssistantMessage { message } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "message",
            "role": "assistant",
            "message": message,
        })),
        AgentEvent::UserMessage { message } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "message",
            "role": "user",
            "message": message,
        })),
        AgentEvent::TextDelta { delta } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "text_delta",
            "delta": delta,
        })),
        AgentEvent::ThinkingDelta { delta } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "thinking_delta",
            "delta": delta,
        })),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "tool_start",
            "id": tool_call_id,
            "name": tool_name,
            "input": args,
        })),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            is_error,
            content,
        } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "tool_end",
            "id": tool_call_id,
            "name": tool_name,
            "is_error": is_error,
            "result": content,
        })),
        AgentEvent::Settlement {
            messages,
            cancelled,
        } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "settlement",
            "messages": messages,
            "cancelled": cancelled,
        })),
        AgentEvent::PermissionDenied { tool_name, reason } => {
            serde_json::to_value(serde_json::json!({
                "type": "event",
                "event": "permission_denied",
                "tool": tool_name,
                "reason": reason,
            }))
        }
        AgentEvent::AgentStart => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "agent_start",
        })),
        AgentEvent::AgentEnd { messages } => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "agent_end",
            "messages": messages,
        })),
        AgentEvent::TurnStart => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "turn_start",
        })),
        AgentEvent::TurnEnd => serde_json::to_value(serde_json::json!({
            "type": "event",
            "event": "turn_end",
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::{CliPermission, Mode};
    use pi_ai::{
        AssistantMessage, AssistantMessageEvent, Content, FakeProviderFactory, StopReason, Usage,
    };
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::BufReader;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-rpc-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    struct FailingWriter;

    impl AsyncWrite for FailingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                ErrorKind::BrokenPipe,
                "writer pipe failed",
            )))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                ErrorKind::BrokenPipe,
                "writer pipe failed",
            )))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    struct FailingProviderFactory;

    #[async_trait::async_trait]
    impl pi_ai::ProviderFactory for FailingProviderFactory {
        async fn stream(
            &self,
            _model: &pi_ai::Model,
            _context: &pi_ai::Context,
            _options: &pi_ai::StreamOptions,
        ) -> pi_ai::Result<pi_ai::AssistantMessageEventStream> {
            eprintln!("PROVIDER CALLED");
            Err(pi_ai::Error::ProviderError {
                status: 500,
                body: "Provider overloaded".into(),
            })
        }
    }

    struct CancelableHangingProviderFactory;

    #[async_trait::async_trait]
    impl pi_ai::ProviderFactory for CancelableHangingProviderFactory {
        async fn stream(
            &self,
            _model: &pi_ai::Model,
            _context: &pi_ai::Context,
            options: &pi_ai::StreamOptions,
        ) -> pi_ai::Result<pi_ai::AssistantMessageEventStream> {
            let cancel = options.cancel.clone();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                if let Some(token) = cancel {
                    token.cancelled().await;
                    let _ = tx.send(Err(pi_ai::Error::Cancelled));
                }
            });
            let stream = futures::stream::unfold(rx, |mut rx| async move {
                let item = rx.recv().await?;
                Some((item, rx))
            });
            Ok(Box::pin(stream))
        }
    }

    #[tokio::test]
    async fn output_sender_rejects_full_queue() {
        let (tx, mut rx) = mpsc::channel(1);
        let (error_tx, mut error_rx) = mpsc::channel(1);
        let sender = OutputSender { tx, error_tx };

        sender.send(serde_json::json!({"first": true})).unwrap();
        let error = sender
            .send(serde_json::json!({"second": true}))
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::WouldBlock);
        assert_eq!(error_rx.recv().await.unwrap().kind(), ErrorKind::WouldBlock);
        assert_eq!(rx.recv().await.unwrap()["first"], true);
    }

    #[tokio::test]
    async fn server_writer_failure_propagates_immediately() {
        let tmp_dir = temp_dir("fail-writer");
        let config = AppConfig {
            config_dir: tmp_dir,
            model: crate::config::resolve_model("gemini-2.0-flash").unwrap(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let permission: Arc<dyn PermissionPolicy> = Arc::new(CliPermission::new(Mode::DenyAll));
        let trust_context = (TrustDecision::Trusted, None);
        let input = "{\"type\":\"get_state\",\"id\":\"1\"}\n";
        let reader = BufReader::new(input.as_bytes());

        let res = run_server(config, permission, trust_context, reader, FailingWriter).await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("writer pipe failed"));
    }

    #[tokio::test]
    async fn server_lifecycle_and_save_on_completion() {
        use tokio::io::AsyncBufReadExt;

        let tmp_dir = temp_dir("lifecycle");
        let model = crate::config::resolve_model("gemini-2.0-flash").unwrap();
        let config = AppConfig {
            config_dir: tmp_dir.clone(),
            model: model.clone(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let permission: Arc<dyn PermissionPolicy> = Arc::new(CliPermission::new(Mode::DenyAll));
        let done_event = AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: AssistantMessage {
                content: vec![Content::Text {
                    text: "Fake completion answer".into(),
                    text_signature: None,
                }],
                api: "google".into(),
                provider: "google".into(),
                model: model.id.clone(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                raw_stop_reason: None,
                usage: Usage::default(),
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: 1000,
            },
        };

        let mut agent_config = AgentConfig::new(config.model.clone(), "sys prompt");
        agent_config.permission = permission;
        agent_config = agent_config
            .with_provider_factory(Arc::new(FakeProviderFactory::new(vec![done_event])));

        let input = concat!(
            "{\"type\":\"get_state\",\"id\":\"s1\"}\n",
            "{\"type\":\"prompt\",\"id\":\"p1\",\"message\":\"hello server\"}\n"
        );
        let reader = BufReader::new(input.as_bytes());
        let (client, writer) = tokio::io::duplex(4096);

        let config_clone = config.clone();
        let handle = tokio::spawn(async move {
            run_server_with_agent_config(config_clone, agent_config, reader, writer).await
        });

        let mut client_reader = BufReader::new(client);
        let mut initial_session_id = String::new();
        let mut got_settlement = false;

        let mut lines = Vec::new();
        let mut line = String::new();
        while client_reader.read_line(&mut line).await.unwrap() > 0 {
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            if val["id"] == "s1" {
                initial_session_id = val["data"]["sessionId"].as_str().unwrap().to_string();
            }
            if val["event"] == "settlement" {
                got_settlement = true;
                break;
            }
            lines.push(val);
            line.clear();
        }

        assert!(!initial_session_id.is_empty());
        assert!(got_settlement);

        let server_res = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(server_res.is_ok());

        // Load saved session from disk and assert exact contents
        let loaded =
            crate::session::load(&tmp_dir, &initial_session_id).expect("session must be loadable");
        assert_eq!(loaded.id, initial_session_id);
        assert_eq!(loaded.model, model.id);

        let expected_user = match loaded.messages.first() {
            Some(Message::User { timestamp, .. }) => {
                let mut message = Message::user_text("hello server");
                if let Message::User {
                    timestamp: user_timestamp,
                    ..
                } = &mut message
                {
                    *user_timestamp = *timestamp;
                }
                message
            }
            _ => panic!("saved transcript must start with user message"),
        };
        let expected_assistant = Message::Assistant(AssistantMessage {
            content: vec![Content::Text {
                text: "Fake completion answer".into(),
                text_signature: None,
            }],
            api: "google".into(),
            provider: "google".into(),
            model: model.id.clone(),
            response_model: None,
            response_id: None,
            diagnostics: None,
            raw_stop_reason: None,
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: 1000,
        });

        let loaded_messages_json = serde_json::to_value(&loaded.messages).unwrap();
        let expected_messages_json =
            serde_json::to_value(vec![expected_user, expected_assistant]).unwrap();
        assert_eq!(loaded_messages_json, expected_messages_json);
    }

    #[tokio::test]
    async fn server_thinking_controls_fixture_replay() {
        use tokio::io::AsyncBufReadExt;

        let fixture_str = include_str!("../../tests/fixtures/rpc-0.83-thinking-controls.json");
        let fixture_val: serde_json::Value = serde_json::from_str(fixture_str).unwrap();
        let exchanges = fixture_val["exchanges"].as_array().unwrap();

        let mut input = String::new();
        for ex in exchanges {
            input.push_str(&serde_json::to_string(&ex["request"]).unwrap());
            input.push('\n');
        }

        let tmp_dir = temp_dir("thinking-fixture");
        let config = AppConfig {
            config_dir: tmp_dir,
            model: crate::config::resolve_model("claude-sonnet-4-6").unwrap(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let agent_config = AgentConfig::new(config.model.clone(), "sys prompt");
        let reader = BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let (client, writer) = tokio::io::duplex(4096);

        let handle = tokio::spawn(async move {
            run_server_with_agent_config(config, agent_config, reader, writer).await
        });

        let mut client_reader = BufReader::new(client);
        let mut line = String::new();
        let mut responses = Vec::new();

        while client_reader.read_line(&mut line).await.unwrap() > 0 {
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            responses.push(val);
            line.clear();
        }

        let server_res = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(server_res.is_ok());

        assert_eq!(responses.len(), exchanges.len());
        for (i, ex) in exchanges.iter().enumerate() {
            assert_eq!(responses[i], ex["response"]);
        }
    }

    #[tokio::test]
    async fn server_thinking_controls_and_live_state() {
        use tokio::io::AsyncBufReadExt;

        let tmp_dir = temp_dir("thinking-controls");
        let model = crate::config::resolve_model("claude-sonnet-4-6").unwrap();
        let config = AppConfig {
            config_dir: tmp_dir.clone(),
            model: model.clone(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let agent_config = AgentConfig::new(config.model.clone(), "sys prompt");

        let input = concat!(
            "{\"type\":\"get_available_thinking_levels\",\"id\":\"t1\"}\n",
            "{\"type\":\"get_state\",\"id\":\"t2\"}\n",
            "{\"type\":\"set_thinking_level\",\"id\":\"t3\",\"level\":\"high\"}\n",
            "{\"type\":\"get_state\",\"id\":\"t4\"}\n",
            "{\"type\":\"cycle_thinking_level\",\"id\":\"t5\"}\n",
            "{\"type\":\"get_state\",\"id\":\"t6\"}\n",
            "{\"type\":\"set_thinking_level\",\"id\":\"t7\",\"level\":\"max\"}\n",
            "{\"type\":\"get_state\",\"id\":\"t8\"}\n",
            "{\"type\":\"new_session\",\"id\":\"t9\"}\n",
            "{\"type\":\"get_state\",\"id\":\"t10\"}\n"
        );
        let reader = BufReader::new(input.as_bytes());
        let (client, writer) = tokio::io::duplex(4096);

        let handle = tokio::spawn(async move {
            run_server_with_agent_config(config, agent_config, reader, writer).await
        });

        let mut client_reader = BufReader::new(client);
        let mut line = String::new();
        let mut responses = std::collections::HashMap::new();

        while client_reader.read_line(&mut line).await.unwrap() > 0 {
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
                responses.insert(id.to_string(), val.clone());
            }
            line.clear();
        }

        let server_res = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(server_res.is_ok());

        assert_eq!(
            responses["t1"]["data"]["levels"],
            serde_json::json!(["off", "minimal", "low", "medium", "high", "xhigh"])
        );
        assert_eq!(responses["t2"]["data"]["thinkingLevel"], "off");
        assert_eq!(responses["t3"]["success"], true);
        assert!(responses["t3"].get("data").is_none());
        assert_eq!(responses["t4"]["data"]["thinkingLevel"], "high");
        assert_eq!(responses["t5"]["data"]["level"], "xhigh");
        assert_eq!(responses["t6"]["data"]["thinkingLevel"], "xhigh");
        // max clamped to highest available (xhigh)
        assert_eq!(responses["t7"]["success"], true);
        assert_eq!(responses["t8"]["data"]["thinkingLevel"], "xhigh");
        // new_session preserves current RPC thinking level
        assert_eq!(responses["t9"]["success"], true);
        assert_eq!(responses["t10"]["data"]["thinkingLevel"], "xhigh");
    }

    #[tokio::test]
    async fn server_provider_error_classification_test() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let tmp_dir = temp_dir("provider-error");
        let model = crate::config::resolve_model("gemini-2.0-flash").unwrap();
        let config = AppConfig {
            config_dir: tmp_dir,
            model: model.clone(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let permission: Arc<dyn PermissionPolicy> = Arc::new(CliPermission::new(Mode::DenyAll));
        let mut agent_config = AgentConfig::new(config.model.clone(), "sys prompt");
        agent_config.permission = permission;
        agent_config = agent_config.with_provider_factory(Arc::new(FailingProviderFactory));

        let (mut client_write, server_read) = tokio::io::duplex(4096);
        let (server_write, client_read) = tokio::io::duplex(4096);

        let handle = tokio::spawn(async move {
            run_server_with_agent_config(
                config,
                agent_config,
                BufReader::new(server_read),
                server_write,
            )
            .await
        });

        client_write
            .write_all(b"{\"type\":\"prompt\",\"id\":\"p1\",\"message\":\"fail test\"}\n")
            .await
            .unwrap();

        let mut client_reader = BufReader::new(client_read);
        let mut prompt_ack = false;
        let mut run_error_found = false;
        let mut settlement_cancelled: Option<bool> = None;
        let mut run_abort_found = false;

        let mut line = String::new();
        while tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_reader.read_line(&mut line),
        )
        .await
        .expect("timed out waiting for provider-error events")
        .unwrap()
            > 0
        {
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            if val["id"] == "p1" && val["success"] == true {
                prompt_ack = true;
            }
            if val["event"] == "run_error" {
                run_error_found = true;
            }
            if val["event"] == "run_abort" {
                run_abort_found = true;
            }
            if val["event"] == "settlement" {
                settlement_cancelled = val["cancelled"].as_bool();
                break;
            }
            line.clear();
        }

        drop(client_write);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;

        assert!(prompt_ack, "Prompt must be acknowledged");
        assert!(run_error_found, "run_error event must be emitted");
        assert_eq!(
            settlement_cancelled,
            Some(false),
            "Settlement cancelled must be false"
        );
        assert!(
            !run_abort_found,
            "run_abort must not be emitted for provider error"
        );
    }

    #[tokio::test]
    async fn server_abort_cancellation_test() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let tmp_dir = temp_dir("abort-cancel");
        let model = crate::config::resolve_model("gemini-2.0-flash").unwrap();
        let config = AppConfig {
            config_dir: tmp_dir,
            model: model.clone(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let permission: Arc<dyn PermissionPolicy> = Arc::new(CliPermission::new(Mode::DenyAll));
        let mut agent_config = AgentConfig::new(config.model.clone(), "sys prompt");
        agent_config.permission = permission;
        agent_config =
            agent_config.with_provider_factory(Arc::new(CancelableHangingProviderFactory));

        let (mut client_write, server_read) = tokio::io::duplex(4096);
        let (server_write, client_read) = tokio::io::duplex(4096);

        let handle = tokio::spawn(async move {
            run_server_with_agent_config(
                config,
                agent_config,
                BufReader::new(server_read),
                server_write,
            )
            .await
        });

        // 1. Send prompt
        client_write
            .write_all(b"{\"type\":\"prompt\",\"id\":\"p1\",\"message\":\"abort test\"}\n")
            .await
            .unwrap();

        let mut client_reader = BufReader::new(client_read);
        let mut line = String::new();

        // Wait for prompt ack
        loop {
            line.clear();
            if tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client_reader.read_line(&mut line),
            )
            .await
            .expect("timed out waiting for prompt ack")
            .unwrap()
                == 0
            {
                panic!("unexpected EOF waiting for prompt ack");
            }
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            if val["id"] == "p1" && val["success"] == true {
                break;
            }
        }

        // 2. Send abort
        client_write
            .write_all(b"{\"type\":\"abort\",\"id\":\"a1\"}\n")
            .await
            .unwrap();

        let mut abort_ack = false;
        let mut run_abort_found = false;
        let mut settlement_cancelled: Option<bool> = None;
        let mut run_error_found = false;

        loop {
            line.clear();
            if tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client_reader.read_line(&mut line),
            )
            .await
            .expect("timed out waiting for abort events")
            .unwrap()
                == 0
            {
                break;
            }
            let val: serde_json::Value = serde_json::from_str(&line).unwrap();
            if val["id"] == "a1" && val["success"] == true {
                abort_ack = true;
                assert!(
                    !run_abort_found,
                    "abort ack must enter output queue before run_abort"
                );
                assert!(
                    settlement_cancelled.is_none(),
                    "abort ack must enter output queue before settlement"
                );
            }
            if val["event"] == "run_abort" {
                run_abort_found = true;
            }
            if val["event"] == "run_error" {
                run_error_found = true;
            }
            if val["event"] == "settlement" {
                settlement_cancelled = val["cancelled"].as_bool();
                break;
            }
        }

        drop(client_write);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;

        assert!(abort_ack, "Abort command must be acknowledged");
        assert!(run_abort_found, "run_abort event must be emitted");
        assert_eq!(
            settlement_cancelled,
            Some(true),
            "Settlement cancelled must be true"
        );
        assert!(
            !run_error_found,
            "run_error must not be emitted for cancellation"
        );
    }

    #[tokio::test]
    async fn server_queue_controls_fixture_replay() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let fixture_str = include_str!("../../tests/fixtures/rpc-0.83-queue-controls.json");
        let fixture: serde_json::Value = serde_json::from_str(fixture_str).unwrap();
        let exchanges = fixture["exchanges"].as_array().unwrap();

        let tmp_dir = temp_dir("queue-controls-fixture");
        let model = crate::config::resolve_model("gemini-2.0-flash").unwrap();
        let config = AppConfig {
            config_dir: tmp_dir,
            model: model.clone(),
            max_turns: 100,
            thinking_level: pi_ai::ThinkingLevel::Off,
        };

        let permission: Arc<dyn PermissionPolicy> = Arc::new(CliPermission::new(Mode::DenyAll));
        let mut agent_config = AgentConfig::new(config.model.clone(), "sys prompt");
        agent_config.permission = permission;
        agent_config = agent_config.with_provider_factory(Arc::new(FailingProviderFactory));

        let (mut client_write, server_read) = tokio::io::duplex(8192);
        let (server_write, client_read) = tokio::io::duplex(8192);

        let handle = tokio::spawn(async move {
            run_server_with_agent_config(
                config,
                agent_config,
                BufReader::new(server_read),
                server_write,
            )
            .await
        });

        let mut client_reader = BufReader::new(client_read);

        for exchange in exchanges {
            let req_json = serde_json::to_string(&exchange["request"]).unwrap();
            client_write
                .write_all(format!("{req_json}\n").as_bytes())
                .await
                .unwrap();

            let mut line = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client_reader.read_line(&mut line),
            )
            .await
            .expect("timed out reading fixture response")
            .unwrap();

            let actual: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(actual, exchange["response"]);
        }

        // Check error recovery on missing mode
        client_write
            .write_all(b"{\"type\":\"set_steering_mode\",\"id\":\"err-1\"}\n")
            .await
            .unwrap();
        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let err_resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(err_resp["id"], "err-1");
        assert_eq!(err_resp["success"], false);
        assert_eq!(err_resp["command"], "set_steering_mode");

        // Assert state via get_state after fixture replay (modes reset to one-at-a-time at end of fixture)
        client_write
            .write_all(b"{\"type\":\"get_state\",\"id\":\"state-check\"}\n")
            .await
            .unwrap();
        line.clear();
        client_reader.read_line(&mut line).await.unwrap();
        let state_resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(state_resp["id"], "state-check");
        assert_eq!(state_resp["data"]["steeringMode"], "one-at-a-time");
        assert_eq!(state_resp["data"]["followUpMode"], "one-at-a-time");

        drop(client_write);
        let res = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(res.is_ok());
    }
}
