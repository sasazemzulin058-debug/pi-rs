use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;
use tokio::io::AsyncWriteExt;

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

use crate::types::{AgentTool, AgentToolResult};

pub struct WriteTool;

fn temporary_path(destination: &Path, n: u64) -> PathBuf {
    let mut name: OsString = destination.as_os_str().to_os_string();
    name.push(format!(".tmp.{}.{n}", std::process::id()));
    PathBuf::from(name)
}

async fn create_exclusive_temp_file(path: PathBuf) -> std::io::Result<fs::File> {
    // The synchronous create_new call is isolated from the async executor. Cancellation can
    // abandon the blocking task after the OS operation starts, so cleanup is bounded but not
    // guaranteed until the next residue cleanup; the exclusive create itself remains atomic.
    tokio::task::spawn_blocking(move || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    })
    .await
    .map_err(std::io::Error::other)?
    .map(fs::File::from_std)
}

#[async_trait]
impl AgentTool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }
    fn requires_permission(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "Write the given content to the path, replacing any existing file. Creates parent directories as needed."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("missing 'path'")?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or("missing 'content'")?;
        let destination = std::path::Path::new(path);
        if let Some(parent) = destination.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
        }

        let existing_permissions = match fs::symlink_metadata(destination).await {
            Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
            Ok(_) => None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("metadata {path}: {e}")),
        };

        let mut tmp_path = None;
        let mut tmp_file = None;
        for _ in 0..100 {
            let n = WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate = temporary_path(destination, n);
            match create_exclusive_temp_file(candidate.clone()).await {
                Ok(file) => {
                    tmp_path = Some(candidate);
                    tmp_file = Some(file);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("write {}: {e}", candidate.display())),
            }
        }
        let (tmp_path, mut tmp_file) = match (tmp_path, tmp_file) {
            (Some(path), Some(file)) => (path, file),
            _ => {
                return Err(format!(
                    "write {path}: could not allocate a unique temporary file"
                ))
            }
        };
        if let Err(e) = tmp_file.write_all(content.as_bytes()).await {
            drop(tmp_file);
            let _ = fs::remove_file(&tmp_path).await;
            return Err(format!("write {}: {e}", tmp_path.display()));
        }
        drop(tmp_file);
        if let Some(permissions) = existing_permissions {
            if let Err(e) = fs::set_permissions(&tmp_path, permissions).await {
                let _ = fs::remove_file(&tmp_path).await;
                return Err(format!("set permissions {}: {e}", tmp_path.display()));
            }
        }
        if let Err(e) = fs::rename(&tmp_path, destination).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(format!("rename {} to {path}: {e}", tmp_path.display()));
        }
        Ok(AgentToolResult::text(format!(
            "wrote {} bytes to {}",
            content.len(),
            path
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    #[tokio::test]
    async fn preserves_restrictive_permissions_on_existing_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "pi-rs-write-permissions-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("secret.txt");
        fs::write(&path, "original").await.unwrap();
        fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .unwrap();

        WriteTool
            .execute(
                "permissions",
                json!({"path": path, "content": "replacement"}),
            )
            .await
            .unwrap();

        let mode = fs::metadata(&path).await.unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "replacement");
        let _ = fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn retries_existing_temporary_path_without_truncating_it() {
        WRITE_COUNTER.store(0, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-write-collision-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("target.txt");
        let occupied = temporary_path(&path, 0);
        fs::write(&occupied, "sentinel").await.unwrap();

        WriteTool
            .execute("collision", json!({"path": path, "content": "replacement"}))
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(&occupied).await.unwrap(), "sentinel");
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "replacement");
        let _ = fs::remove_dir_all(dir).await;
    }
}
