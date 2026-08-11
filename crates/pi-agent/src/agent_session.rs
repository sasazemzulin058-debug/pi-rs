use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::agent_loop::run_agent_with_history_and_steering;
use crate::error::{AgentError, Result};
pub use crate::types::{AgentConfig, AgentEvent, AgentSessionState, QueueMode, SessionPhase};

/// Stateful agent session wrapper managing follow-up/steering queues,
/// turn lifecycle, phase changes, provider cancellation, and settlement.
///
/// Session cancellation token interrupts active provider streams. `AgentTool::execute`
/// has no cancellation parameter, so cancellation during a tool call takes effect only
/// after that call returns.
#[derive(Clone)]
pub struct AgentSession {
    config: Arc<Mutex<AgentConfig>>,
    state: Arc<Mutex<AgentSessionState>>,
    active_run: Arc<AsyncMutex<()>>,
    active_cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl AgentSession {
    pub fn new(config: AgentConfig, initial_messages: Vec<pi_ai::Message>) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            state: Arc::new(Mutex::new(AgentSessionState::new(initial_messages))),
            active_run: Arc::new(AsyncMutex::new(())),
            active_cancel: Arc::new(Mutex::new(None)),
        }
    }

    pub fn thinking_level(&self) -> pi_ai::ThinkingLevel {
        self.config.lock().unwrap().thinking_level
    }

    pub fn set_thinking_level(&self, level: pi_ai::ThinkingLevel) {
        self.config.lock().unwrap().thinking_level = level;
    }

    pub fn state(&self) -> AgentSessionState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_queue_mode(&self, mode: QueueMode) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.steering_mode = mode;
        state.followup_mode = mode;
        Ok(())
    }

    pub fn set_steering_mode(&self, mode: QueueMode) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.steering_mode = mode;
        Ok(())
    }

    pub fn set_followup_mode(&self, mode: QueueMode) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.followup_mode = mode;
        Ok(())
    }

    pub fn queue_followup(&self, msg: pi_ai::Message) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.queue_followup(msg)
    }

    pub fn queue_steering(&self, msg: pi_ai::Message) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.queue_steering(msg)
    }

    pub fn cancel(&self) {
        if let Some(cancel) = self.active_cancel.lock().unwrap().as_ref() {
            cancel.cancel();
        }
    }

    pub fn is_settled(&self) -> bool {
        self.state.lock().unwrap().settled
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
            let (inputs, active_phase) = if !state.steering_queue.is_empty() {
                (state.take_steering(), SessionPhase::Steering)
            } else if !state.input_queue.is_empty() {
                (state.take_followups(), SessionPhase::Executing)
            } else {
                if matches!(
                    state.phase,
                    SessionPhase::Executing | SessionPhase::Steering
                ) {
                    state.phase = SessionPhase::Idle;
                    drop(state);
                    Self::publish_phase(SessionPhase::Idle, &events);
                }
                return Ok(false);
            };
            state.phase = active_phase.clone();
            state.cancelled = false;
            state.messages.extend(inputs.iter().cloned());
            (inputs, active_phase)
        };
        debug_assert!(!inputs.is_empty());
        Self::publish_phase(active_phase, &events);

        let cancel_token = CancellationToken::new();
        *self.active_cancel.lock().unwrap() = Some(cancel_token.clone());

        let mut config = self.config.lock().unwrap().clone();
        config.stream_options.cancel = Some(cancel_token);

        let state_arc = self.state.clone();
        let get_steering = Arc::new(move || {
            let mut st = state_arc.lock().unwrap();
            st.take_steering()
        });

        let current_messages = self.state.lock().unwrap().messages.clone();
        let run_res = run_agent_with_history_and_steering(
            &config,
            current_messages,
            events.clone(),
            get_steering,
        )
        .await;

        *self.active_cancel.lock().unwrap() = None;

        match run_res {
            Ok(run) => {
                let settlement = {
                    let mut state = self.state.lock().unwrap();
                    state.messages = run.messages;
                    state.phase = SessionPhase::Idle;
                    state.cancelled = false;
                    state.settled = false;
                    None
                };
                if let Some((messages, cancelled)) = settlement {
                    Self::publish_settlement(messages, cancelled, &events);
                } else {
                    Self::publish_phase(SessionPhase::Idle, &events);
                }
                Ok(true)
            }
            Err(error) => {
                {
                    let mut state = self.state.lock().unwrap();
                    state.phase = SessionPhase::Idle;
                    state.cancelled = false;
                    state.settled = false;
                }
                Self::publish_phase(SessionPhase::Idle, &events);
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
            let is_empty = {
                let mut state = self.state.lock().unwrap();
                if state.steering_queue.is_empty() && state.input_queue.is_empty() {
                    state.phase = SessionPhase::Idle;
                    state.cancelled = false;
                    state.settled = false;
                    true
                } else {
                    false
                }
            };
            if is_empty {
                Self::publish_phase(SessionPhase::Idle, &events);
                return Ok(());
            }

            self.tick_locked(events.clone()).await?;
        }
    }
}
