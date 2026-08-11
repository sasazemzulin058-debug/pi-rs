//! Optional Node.js resolution. Core runtime never installs Node.

use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_NODE_MAJOR: u64 = 18;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRuntime {
    pub executable: PathBuf,
    pub version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NodeRuntimeError {
    #[error("Node executable not found")]
    NotFound,
    #[error("failed to run Node at {path}: {source}")]
    Probe {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid Node version output: {0}")]
    InvalidVersion(String),
    #[error("Node {found} unsupported; minimum is {MIN_NODE_MAJOR}")]
    Unsupported { found: u64 },
}

pub fn resolve_node(explicit: Option<&Path>) -> Result<NodeRuntime, NodeRuntimeError> {
    let candidate = explicit
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|p| p.join("node"))
                    .find(|p| p.is_file())
            })
        })
        .or_else(|| std::env::var_os("PREFIX").map(|p| PathBuf::from(p).join("bin/node")))
        .ok_or(NodeRuntimeError::NotFound)?;
    probe_node(candidate)
}

pub fn probe_node(path: PathBuf) -> Result<NodeRuntime, NodeRuntimeError> {
    let output = Command::new(&path)
        .arg("--version")
        .output()
        .map_err(|source| NodeRuntimeError::Probe {
            path: path.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(NodeRuntimeError::NotFound);
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let major = version
        .strip_prefix('v')
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| NodeRuntimeError::InvalidVersion(version.clone()))?;
    if major < MIN_NODE_MAJOR {
        return Err(NodeRuntimeError::Unsupported { found: major });
    }
    Ok(NodeRuntime {
        executable: path,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    fn fake(version: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "pi node {} {}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&p, format!("#!/bin/sh\necho {version}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
    #[test]
    fn accepts_present_node() {
        let p = fake("v20.1.0");
        assert_eq!(probe_node(p.clone()).unwrap().version, "v20.1.0");
        let _ = fs::remove_file(p);
    }
    #[test]
    fn rejects_old_node() {
        let p = fake("v16.0.0");
        assert!(matches!(
            probe_node(p.clone()),
            Err(NodeRuntimeError::Unsupported { found: 16 })
        ));
        let _ = fs::remove_file(p);
    }
    #[test]
    fn absent_node_errors() {
        assert!(matches!(
            probe_node(PathBuf::from("/missing/node")),
            Err(NodeRuntimeError::Probe { .. })
        ));
    }
}
