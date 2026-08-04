//! Interactive permission prompt for the CLI.

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use pi_agent::{PermissionDecision, PermissionPolicy};
use serde_json::Value;

#[allow(dead_code)] // DenyAll is a public mode, kept for sandboxed runs.
pub enum Mode {
    /// Always allow without asking.
    Yolo,
    /// Prompt the user interactively (stdin / stderr).
    Interactive,
    /// Always deny — useful for sandboxed runs.
    DenyAll,
}

pub struct CliPermission {
    mode: Mode,
    allowed_session: Mutex<std::collections::HashSet<String>>,
    reader: Option<Arc<Mutex<Box<dyn BufRead + Send>>>>,
}

impl CliPermission {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            allowed_session: Mutex::new(Default::default()),
            reader: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_reader(mode: Mode, reader: Box<dyn BufRead + Send>) -> Self {
        Self {
            mode,
            allowed_session: Mutex::new(Default::default()),
            reader: Some(Arc::new(Mutex::new(reader))),
        }
    }
}

#[async_trait]
impl PermissionPolicy for CliPermission {
    async fn check(&self, tool_name: &str, args: &Value) -> PermissionDecision {
        if let Ok(set) = self.allowed_session.lock() {
            if set.contains(tool_name) {
                return PermissionDecision::Allow;
            }
        }
        match self.mode {
            Mode::Yolo => PermissionDecision::Allow,
            Mode::DenyAll => PermissionDecision::Deny {
                reason: "permissions disabled".into(),
            },
            Mode::Interactive => {
                prompt(tool_name, args, &self.allowed_session, self.reader.clone()).await
            }
        }
    }
}

fn parse_permission_answer(answer: &str) -> PermissionDecision {
    match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => PermissionDecision::Allow,
        "a" | "all" | "allow" | "session" => PermissionDecision::AllowSession,
        _ => PermissionDecision::Deny {
            reason: "user denied".into(),
        },
    }
}

async fn prompt(
    tool_name: &str,
    args: &Value,
    allowed_session: &Mutex<std::collections::HashSet<String>>,
    reader: Option<Arc<Mutex<Box<dyn BufRead + Send>>>>,
) -> PermissionDecision {
    let tool_name = tool_name.to_string();
    let session_tool_name = tool_name.clone();
    let args_pretty = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
    // run the blocking prompt off the runtime thread
    let decision = tokio::task::spawn_blocking(move || {
        let mut err = std::io::stderr();
        let _ = writeln!(err, "\n⚠ tool call requires permission: {tool_name}");
        let _ = writeln!(err, "{args_pretty}");
        let _ = write!(err, "Allow? [y]es / [a]llow-session / [n]o: ");
        let _ = err.flush();
        let mut line = String::new();
        let read_res = if let Some(custom_reader) = reader {
            if let Ok(mut guard) = custom_reader.lock() {
                guard.read_line(&mut line)
            } else {
                return PermissionDecision::Deny {
                    reason: "permission prompt lock failed".into(),
                };
            }
        } else {
            std::io::stdin().lock().read_line(&mut line)
        };
        match read_res {
            Ok(0) => PermissionDecision::Deny {
                reason: "permission prompt reached EOF".into(),
            },
            Ok(_) => parse_permission_answer(&line),
            Err(_) => PermissionDecision::Deny {
                reason: "permission prompt failed".into(),
            },
        }
    })
    .await
    .unwrap_or(PermissionDecision::Deny {
        reason: "permission prompt failed".into(),
    });

    if decision == PermissionDecision::AllowSession {
        if let Ok(mut s) = allowed_session.lock() {
            s.insert(session_tool_name);
        }
    }
    decision
}

impl CliPermission {
    pub fn reset_session(&self) {
        if let Ok(mut allowed) = self.allowed_session.lock() {
            allowed.clear();
        }
        pi_agent::agent_loop::reset_session_permissions(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_agent::PermissionDecision;
    use std::io::Cursor;

    #[tokio::test]
    async fn prompt_with_empty_reader_returns_eof_deny() {
        let empty_reader = Box::new(Cursor::new(Vec::<u8>::new()));
        let policy = CliPermission::with_reader(Mode::Interactive, empty_reader);
        let decision = policy.check("bash", &serde_json::json!({})).await;
        assert_eq!(
            decision,
            PermissionDecision::Deny {
                reason: "permission prompt reached EOF".into()
            }
        );
    }

    #[test]
    fn permission_answers_are_fail_closed() {
        let cases = [
            ("y", PermissionDecision::Allow),
            (" yes ", PermissionDecision::Allow),
            ("a", PermissionDecision::AllowSession),
            (
                "allow-session",
                PermissionDecision::Deny {
                    reason: "user denied".into(),
                },
            ),
            (
                "",
                PermissionDecision::Deny {
                    reason: "user denied".into(),
                },
            ),
            (
                "   ",
                PermissionDecision::Deny {
                    reason: "user denied".into(),
                },
            ),
            (
                "maybe",
                PermissionDecision::Deny {
                    reason: "user denied".into(),
                },
            ),
        ];

        for (answer, expected) in cases {
            assert_eq!(
                parse_permission_answer(answer),
                expected,
                "answer: {answer:?}"
            );
        }
    }
}
