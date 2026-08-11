//! Trusted Node extension host. JSONL stdout reserved for ABI messages.

use crate::node_runtime::{resolve_node, NodeRuntimeError};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub const ABI_VERSION: u32 = 1;
const MAX_LINE: usize = 1024 * 1024;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize)]
struct Handshake<'a> {
    kind: &'a str,
    version: u32,
}
#[derive(Debug, Serialize)]
pub struct HookRequest {
    pub kind: &'static str,
    pub id: u64,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct HostMessage {
    pub kind: String,
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub block: Option<bool>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Debug, thiserror::Error)]
pub enum ExtensionHostError {
    #[error("Node runtime unavailable: {0}")]
    Runtime(#[from] NodeRuntimeError),
    #[error("extension path is not trusted")]
    UntrustedPath,
    #[error("host I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON encoding: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid host protocol: {0}")]
    Protocol(String),
}

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ChildGuard {
    fn into_inner(mut self) -> Child {
        self.0.take().expect("child guard consumed once")
    }
}

pub struct ExtensionHost {
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<Vec<u8>, String>>,
}

impl ExtensionHost {
    pub fn spawn(
        node: Option<&Path>,
        extension: &Path,
        trusted_root: &Path,
    ) -> Result<Self, ExtensionHostError> {
        let extension = extension
            .canonicalize()
            .map_err(|_| ExtensionHostError::UntrustedPath)?;
        let root = trusted_root
            .canonicalize()
            .map_err(|_| ExtensionHostError::UntrustedPath)?;
        if !extension.starts_with(&root) || !extension.is_file() {
            return Err(ExtensionHostError::UntrustedPath);
        }
        let runtime = resolve_node(node)?;
        let host_script = Path::new(env!("CARGO_MANIFEST_DIR")).join("node/extension-host.mjs");
        let child = Command::new(runtime.executable)
            .arg(host_script)
            .arg(&extension)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut guard = ChildGuard(Some(child));
        let mut input = guard
            .0
            .as_mut()
            .expect("child exists")
            .stdin
            .take()
            .ok_or_else(|| ExtensionHostError::Protocol("missing stdin".into()))?;
        let output = guard
            .0
            .as_mut()
            .expect("child exists")
            .stdout
            .take()
            .ok_or_else(|| ExtensionHostError::Protocol("missing stdout".into()))?;
        serde_json::to_writer(
            &mut input,
            &Handshake {
                kind: "pi-extension-host",
                version: ABI_VERSION,
            },
        )?;
        input.write_all(b"\n")?;
        input.flush()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || read_frames(output, tx));
        let mut host = Self {
            child: guard.into_inner(),
            input,
            output: rx,
        };
        let msg: HostMessage = host.read_json()?;
        if msg.kind != "pi-extension-host" || msg.version != Some(ABI_VERSION) {
            return Err(ExtensionHostError::Protocol("unsupported handshake".into()));
        }
        Ok(host)
    }

    pub fn send(&mut self, message: &impl Serialize) -> Result<HostMessage, ExtensionHostError> {
        if let Err(error) = serde_json::to_writer(&mut self.input, message) {
            self.abort();
            return Err(ExtensionHostError::Json(error));
        }
        if let Err(error) = self.input.write_all(b"\n").and_then(|_| self.input.flush()) {
            self.abort();
            return Err(ExtensionHostError::Io(error));
        }
        self.read_json()
    }

    fn abort(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn read_json<T: for<'de> Deserialize<'de>>(&mut self) -> Result<T, ExtensionHostError> {
        let line = match self.output.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                self.abort();
                return Err(ExtensionHostError::Protocol(error));
            }
            Err(error) => {
                self.abort();
                return Err(ExtensionHostError::Protocol(format!(
                    "host response timeout: {error}"
                )));
            }
        };
        match serde_json::from_slice(&line) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.abort();
                Err(ExtensionHostError::Protocol(error.to_string()))
            }
        }
    }
}

fn read_frames(mut output: ChildStdout, tx: mpsc::Sender<Result<Vec<u8>, String>>) {
    let mut reader = BufReader::new(&mut output);
    loop {
        let mut frame = Vec::new();
        loop {
            let chunk = match reader.fill_buf() {
                Ok(chunk) => chunk,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            if chunk.is_empty() {
                let _ = tx.send(Err("host exited before response".into()));
                return;
            }
            let take = chunk
                .iter()
                .position(|b| *b == b'\n')
                .map_or(chunk.len(), |p| p + 1);
            frame.extend_from_slice(&chunk[..take]);
            reader.consume(take);
            if frame.len() > MAX_LINE {
                let _ = tx.send(Err("message too large".into()));
                return;
            }
            if frame.last() == Some(&b'\n') {
                break;
            }
        }
        while frame.last() == Some(&b'\n') || frame.last() == Some(&b'\r') {
            frame.pop();
        }
        if tx.send(Ok(frame)).is_err() {
            return;
        }
    }
}
impl Drop for ExtensionHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_runtime::NodeRuntime;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-extension-host-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn runtime_or_skip() -> Option<NodeRuntime> {
        resolve_node(None).ok()
    }

    #[test]
    fn node_extension_handshake_and_hook_roundtrip() {
        let Some(runtime) = runtime_or_skip() else {
            return;
        };
        let dir = test_dir("roundtrip");
        let extension = dir.join("extension.js");
        fs::write(
            &extension,
            "export default pi => pi.on('tool_call', e => { e.input.x = 1; });",
        )
        .unwrap();
        let mut host = ExtensionHost::spawn(Some(&runtime.executable), &extension, &dir).unwrap();
        let response = host
            .send(&HookRequest {
                kind: "hook",
                id: 1,
                tool_name: "x".into(),
                tool_call_id: "c".into(),
                input: serde_json::json!({"value": 1}),
            })
            .unwrap();
        assert_eq!(response.kind, "response");
        assert_eq!(response.id, Some(1));
        assert_eq!(response.block, Some(false));
        assert_eq!(
            response.input,
            Some(serde_json::json!({"value": 1, "x": 1}))
        );
        drop(host);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn node_extension_spaced_path() {
        let Some(runtime) = runtime_or_skip() else {
            return;
        };
        let dir = test_dir("space dir");
        let extension = dir.join("extension with spaces.js");
        fs::write(&extension, "export default () => {};").unwrap();
        let host = ExtensionHost::spawn(Some(&runtime.executable), &extension, &dir).unwrap();
        drop(host);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn node_extension_timeout_kills_host() {
        let Some(runtime) = runtime_or_skip() else {
            return;
        };
        let dir = test_dir("timeout");
        let extension = dir.join("extension.js");
        fs::write(
            &extension,
            "export default pi => pi.on('tool_call', async () => new Promise(() => {}));",
        )
        .unwrap();
        let mut host = ExtensionHost::spawn(Some(&runtime.executable), &extension, &dir).unwrap();
        let result = host.send(&HookRequest {
            kind: "hook",
            id: 2,
            tool_name: "x".into(),
            tool_call_id: "c".into(),
            input: serde_json::json!({}),
        });
        assert!(
            matches!(result, Err(ExtensionHostError::Protocol(message)) if message.contains("timeout"))
        );
        drop(host);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn node_extension_oversize_frame_fails_startup() {
        let Some(runtime) = runtime_or_skip() else {
            return;
        };
        let dir = test_dir("oversize");
        let extension = dir.join("extension.js");
        fs::write(
            &extension,
            "process.stdout.write('x'.repeat(1024 * 1024 + 1) + '\\n'); export default () => {};",
        )
        .unwrap();
        let result = ExtensionHost::spawn(Some(&runtime.executable), &extension, &dir);
        assert!(
            matches!(result, Err(ExtensionHostError::Protocol(message)) if message.contains("message too large"))
        );
        let _ = fs::remove_dir_all(dir);
    }
}
