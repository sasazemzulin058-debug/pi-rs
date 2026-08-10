use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::agent_loop::run_agent_with_history;
use crate::error::{AgentError, Result};
pub use crate::types::{AgentConfig, AgentEvent, AgentSessionState, QueueMode, SessionPhase};

/// Stateful agent session wrapper managing follow-up/steering queues,
/// turn lifecycle, phase changes, provider cancellation, and settlement.
///
/// Session cancellation token interrupts active provider streams. `AgentTool::execute`
/// has no cancellation parameter, so cancellation during a tool call takes effect only
/// after that call returns.
///
/// ponytail: steering queued during an active `agent_loop` invocation cannot be injected
/// at its next internal turn. Upgrade path: expose a single-turn agent-loop API.
#[derive(Clone)]
pub struct AgentSession {
    config: AgentConfig,
    state: Arc<Mutex<AgentSessionState>>,
    active_run: Arc<AsyncMutex<()>>,
    cancel: CancellationToken,
}

impl AgentSession {
    pub fn new(mut config: AgentConfig, initial_messages: Vec<pi_ai::Message>) -> Self {
        let cancel = CancellationToken::new();
        config.stream_options.cancel = Some(cancel.clone());
        Self {
            config,
            state: Arc::new(Mutex::new(AgentSessionState::new(initial_messages))),
            active_run: Arc::new(AsyncMutex::new(())),
            cancel,
        }
    }

    pub fn state(&self) -> AgentSessionState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_queue_mode(&self, mode: QueueMode) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::ensure_open(&state)?;
        state.queue_mode = mode;
        Ok(())
    }

    pub fn queue_followup(&self, msg: pi_ai::Message) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::ensure_open(&state)?;
        state.queue_followup(msg)
    }

    pub fn queue_steering(&self, msg: pi_ai::Message) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        Self::ensure_open(&state)?;
        state.queue_steering(msg)
    }

    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        if !state.settled {
            state.cancel();
            self.cancel.cancel();
        }
    }

    pub fn is_settled(&self) -> bool {
        self.state.lock().unwrap().settled
    }

    fn ensure_open(state: &AgentSessionState) -> Result<()> {
        if state.settled {
            Err(AgentError::Other("session already settled".into()))
        } else if state.cancelled {
            Err(AgentError::Other("session cancelled".into()))
        } else {
            Ok(())
        }
    }

    fn publish_phase(phase: SessionPhase, events: &Option<mpsc::UnboundedSender<AgentEvent>>) {
        if let Some(tx) = events {
            let _ = tx.send(AgentEvent::PhaseChange { phase });
        }
    }

    fn publish_settlement(
        messages: Vec<pi_ai::Message>,
        cancelled: bool,
        events: &Option<mpsc::UnboundedSender<AgentEvent>>,
    ) {
        if let Some(tx) = events {
            let _ = tx.send(AgentEvent::PhaseChange {
                phase: SessionPhase::Settled,
            });
            let _ = tx.send(AgentEvent::Settlement {
                messages,
                cancelled,
            });
        }
    }

    fn settle_locked(state: &mut AgentSessionState) -> Option<(Vec<pi_ai::Message>, bool)> {
        if state.settled {
            return None;
        }
        state.settled = true;
        state.phase = SessionPhase::Settled;
        Some((state.messages.clone(), state.cancelled))
    }

    /// Process next available queued inputs for one agent-loop batch.
    /// Returns `Ok(true)` if work was executed, or `Ok(false)` if queue was empty or cancelled.
    /// Rejects overlap with another `run` or `tick` invocation.
    pub async fn tick(&self, events: Option<mpsc::UnboundedSender<AgentEvent>>) -> Result<bool> {
        let _active = self
            .active_run
            .try_lock()
            .map_err(|_| AgentError::Other("session run already active".into()))?;
        self.tick_locked(events).await
    }

    async fn tick_locked(&self, events: Option<mpsc::UnboundedSender<AgentEvent>>) -> Result<bool> {
        let (inputs, active_phase) = {
            let mut state = self.state.lock().unwrap();
            if state.settled {
                return Err(AgentError::Other("session already settled".into()));
            }
            if state.cancelled {
                let (messages, cancelled) = Self::settle_locked(&mut state).unwrap();
                drop(state);
                Self::publish_settlement(messages, cancelled, &events);
                return Ok(false);
            }

            let active_phase = if !state.steering_queue.is_empty() {
                SessionPhase::Steering
            } else {
                SessionPhase::Executing
            };
            let inputs = state.take_inputs();
            if inputs.is_empty() {
                if matches!(
                    state.phase,
                    SessionPhase::Executing | SessionPhase::Steering
                ) {
                    state.phase = SessionPhase::Idle;
                    drop(state);
                    Self::publish_phase(SessionPhase::Idle, &events);
                }
                return Ok(false);
            }
            state.phase = active_phase.clone();
            state.messages.extend(inputs.iter().cloned());
            (inputs, active_phase)
        };
        debug_assert!(!inputs.is_empty());
        Self::publish_phase(active_phase, &events);

        let current_messages = self.state.lock().unwrap().messages.clone();
        let run_res = run_agent_with_history(&self.config, current_messages, events.clone()).await;

        match run_res {
            Ok(run) => {
                let settlement = {
                    let mut state = self.state.lock().unwrap();
                    state.messages = run.messages;
                    if state.cancelled {
                        Self::settle_locked(&mut state)
                    } else {
                        state.phase = SessionPhase::Idle;
                        None
                    }
                };
                if let Some((messages, cancelled)) = settlement {
                    Self::publish_settlement(messages, cancelled, &events);
                } else {
                    Self::publish_phase(SessionPhase::Idle, &events);
                }
                Ok(true)
            }
            Err(error) => {
                let settlement = {
                    let mut state = self.state.lock().unwrap();
                    Self::settle_locked(&mut state)
                };
                if let Some((messages, cancelled)) = settlement {
                    Self::publish_settlement(messages, cancelled, &events);
                }
                Err(error)
            }
        }
    }

    /// Run queued inputs until queues drain or session cancels/settles.
    /// Queue-drain detection and settlement share one state lock, so accepted input
    /// cannot be lost behind settlement.
    pub async fn run(&self, events: Option<mpsc::UnboundedSender<AgentEvent>>) -> Result<()> {
        let _active = self
            .active_run
            .try_lock()
            .map_err(|_| AgentError::Other("session run already active".into()))?;

        loop {
            let settlement = {
                let mut state = self.state.lock().unwrap();
                if state.settled {
                    return Err(AgentError::Other("session already settled".into()));
                }
                if state.cancelled
                    || (state.steering_queue.is_empty() && state.input_queue.is_empty())
                {
                    Self::settle_locked(&mut state)
                } else {
                    None
                }
            };
            if let Some((messages, cancelled)) = settlement {
                Self::publish_settlement(messages, cancelled, &events);
                return Ok(());
            }

            self.tick_locked(events.clone()).await?;
        }
    }
}
