use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{timeout, Duration};

use crate::types::{AgentTool, AgentToolResult};

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
        } else {
            false
        }
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

pub struct BashTool {
    cwd: Mutex<PathBuf>,
}

impl BashTool {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            cwd: Mutex::new(cwd),
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    fn requires_permission(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "Run a shell command via shell `-lc <cmd>`. Returns combined stdout/stderr and exit code, and `cd <path>` to change persistent cwd."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_ms": {"type": "integer", "default": 120000}
            },
            "required": ["command"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("missing 'command'")?;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let trimmed = cmd.trim();
        if let Some(rest) = trimmed.strip_prefix("cd ") {
            let target = rest.trim();
            if !target.is_empty() {
                let mut guard = self.cwd.lock().await;
                let candidate = PathBuf::from(target);
                let joined = if candidate.is_absolute() {
                    candidate
                } else {
                    guard.join(&candidate)
                };
                let resolved = joined.canonicalize().unwrap_or(joined);
                *guard = resolved.clone();
                return Ok(AgentToolResult::text(format!(
                    "(cwd → {})",
                    resolved.display()
                )));
            }
        }

        let cwd_snapshot = { self.cwd.lock().await.clone() };

        let shell_candidate = std::env::var("SHELL")
            .ok()
            .filter(|s| {
                let trimmed = s.trim();
                !trimmed.is_empty()
                    && (trimmed == "sh"
                        || trimmed == "bash"
                        || is_executable(std::path::Path::new(trimmed)))
            })
            .unwrap_or_else(|| "sh".to_string());

        let mut cmd_builder = Command::new(&shell_candidate);
        cmd_builder
            .arg("-lc")
            .arg(cmd)
            .current_dir(&cwd_snapshot)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        cmd_builder.process_group(0);

        let spawn_res = cmd_builder.spawn();

        let mut child = match spawn_res {
            Ok(c) => c,
            Err(_) if shell_candidate != "sh" => {
                let mut fallback = Command::new("sh");
                fallback
                    .arg("-lc")
                    .arg(cmd)
                    .current_dir(&cwd_snapshot)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                #[cfg(unix)]
                fallback.process_group(0);
                fallback
                    .spawn()
                    .map_err(|e| format!("spawn fallback sh: {e}"))?
            }
            Err(e) => return Err(format!("spawn: {e}")),
        };

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture stdout".to_string())?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| "failed to capture stderr".to_string())?;

        let (tx, mut rx) = mpsc::channel::<(bool, Vec<u8>)>(64);
        let tx_out = tx.clone();
        let tx_err = tx;

        let stdout_task = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx_out.send((false, buf[..n].to_vec())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx_err.send((true, buf[..n].to_vec())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let max_output_bytes = 100 * 1024;

        let combined = Arc::new(Mutex::new(Vec::new()));
        let combined_collector = combined.clone();
        let collector = tokio::spawn(async move {
            let mut truncated = false;

            while let Some((is_err, mut chunk)) = rx.recv().await {
                if truncated {
                    continue;
                }
                let mut buf = combined_collector.lock().await;
                if is_err {
                    buf.extend_from_slice(b"[stderr] ");
                }
                buf.append(&mut chunk);
                let current_len = buf.len();

                if current_len > max_output_bytes {
                    let mut truncated_len = max_output_bytes;
                    while truncated_len > 0 && (buf[truncated_len] & 0xC0) == 0x80 {
                        truncated_len -= 1;
                    }
                    buf.truncate(truncated_len);
                    buf.extend_from_slice(b"\n[output truncated]");
                    truncated = true;
                }
            }
        });

        let status = match timeout(Duration::from_millis(timeout_ms), child.wait()).await {
            Ok(Ok(s)) => {
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                let _ = collector.await;
                s
            }
            Ok(Err(e)) => {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                let _ = collector.await;
                return Err(format!("wait: {e}"));
            }
            Err(_) => {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    }
                }
                let _ = child.kill().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                let _ = collector.await;
                return Err(format!("command timed out after {timeout_ms}ms"));
            }
        };

        let code = status.code().unwrap_or(-1);
        let raw = combined.lock().await;
        let mut out = String::from_utf8_lossy(&raw).into_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("[exit {code}]"));
        Ok(AgentToolResult::text(out))
    }
}
