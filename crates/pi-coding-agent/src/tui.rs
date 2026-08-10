use std::os::fd::AsRawFd;

use pi_agent::AgentEvent;

pub struct TerminalGuard {
    saved_termios: Option<libc::termios>,
    fd: i32,
}

impl TerminalGuard {
    pub fn enter_raw_mode() -> std::io::Result<Self> {
        let fd = std::io::stdin().as_raw_fd();
        if unsafe { libc::isatty(fd) } == 0 {
            return Ok(Self {
                saved_termios: None,
                fd,
            });
        }

        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut termios) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            let saved_termios = termios;
            let mut raw = termios;
            libc::cfmakeraw(&mut raw);

            if libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            Ok(Self {
                saved_termios: Some(saved_termios),
                fd,
            })
        }
    }

    pub fn is_raw(&self) -> bool {
        self.saved_termios.is_some()
    }

    /// Restore terminal settings. Safe to call repeatedly.
    pub fn restore(&mut self) -> std::io::Result<()> {
        let Some(saved) = self.saved_termios.take() else {
            return Ok(());
        };
        if unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &saved) } != 0 {
            self.saved_termios = Some(saved);
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentStatus {
    #[default]
    Idle,
    Running,
    Thinking,
    ToolExecuting(String),
    Error(String),
}

pub fn reduce(state: &mut TuiState, event: &AgentEvent) {
    match event {
        AgentEvent::AgentStart => {
            state.status = AgentStatus::Running;
            state.text_buffer.clear();
            state.thinking_buffer.clear();
        }
        AgentEvent::TextDelta { delta } => {
            state.status = AgentStatus::Running;
            state.text_buffer.push_str(delta);
        }
        AgentEvent::ThinkingDelta { delta } => {
            state.status = AgentStatus::Thinking;
            state.thinking_buffer.push_str(delta);
        }
        AgentEvent::ToolExecutionStart { tool_name, .. } => {
            state.status = AgentStatus::ToolExecuting(tool_name.clone());
        }
        AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        } => {
            state.status = if *is_error {
                AgentStatus::Error(format!("tool failed: {tool_name}"))
            } else {
                AgentStatus::Running
            };
        }
        AgentEvent::PermissionDenied { tool_name, reason } => {
            state.status = AgentStatus::Error(format!("permission denied: {tool_name}: {reason}"));
        }
        AgentEvent::AgentEnd { .. } | AgentEvent::Settlement { .. } => {
            state.status = AgentStatus::Idle;
        }
        AgentEvent::PhaseChange { .. }
        | AgentEvent::TurnStart
        | AgentEvent::TurnEnd
        | AgentEvent::AssistantMessage { .. }
        | AgentEvent::UserMessage { .. } => {}
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiState {
    pub status: AgentStatus,
    pub text_buffer: String,
    pub thinking_buffer: String,
}

impl TuiState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, event: &AgentEvent) {
        reduce(self, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_guard_does_not_touch_non_tty_stdin() {
        let stdin_is_tty = unsafe { libc::isatty(std::io::stdin().as_raw_fd()) != 0 };
        if stdin_is_tty {
            // A real terminal may be shared with other tests; never change its mode here.
            return;
        }

        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        assert!(!guard.is_raw());
        guard.restore().unwrap();
        guard.restore().unwrap();
        assert!(!guard.is_raw());
    }

    #[test]
    fn test_tui_state_reducer_flow() {
        let mut state = TuiState::new();
        assert_eq!(state.status, AgentStatus::Idle);

        state.apply(&AgentEvent::AgentStart);
        assert_eq!(state.status, AgentStatus::Running);

        state.apply(&AgentEvent::ThinkingDelta {
            delta: "thinking...".into(),
        });
        assert_eq!(state.status, AgentStatus::Thinking);
        assert_eq!(state.thinking_buffer, "thinking...");

        state.apply(&AgentEvent::TextDelta {
            delta: "hello ".into(),
        });
        assert_eq!(state.status, AgentStatus::Running);
        assert_eq!(state.text_buffer, "hello ");

        state.apply(&AgentEvent::ToolExecutionStart {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            args: serde_json::json!({}),
        });
        assert_eq!(state.status, AgentStatus::ToolExecuting("bash".into()));

        state.apply(&AgentEvent::ToolExecutionEnd {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            is_error: false,
            content: vec![],
        });
        assert_eq!(state.status, AgentStatus::Running);

        state.apply(&AgentEvent::AgentEnd { messages: vec![] });
        assert_eq!(state.status, AgentStatus::Idle);

        state.apply(&AgentEvent::PermissionDenied {
            tool_name: "bash".into(),
            reason: "fail".into(),
        });
        assert_eq!(
            state.status,
            AgentStatus::Error("permission denied: bash: fail".into())
        );
    }
}
