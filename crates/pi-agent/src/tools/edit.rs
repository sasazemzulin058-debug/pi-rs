use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use pi_ai::Content;
use serde_json::{json, Value};
use similar::TextDiff;
use tokio::fs;
use tokio::io::AsyncWriteExt;

static EDIT_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempFileGuard {
    path: Option<std::path::PathBuf>,
}

impl TempFileGuard {
    fn disarm(mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}

use crate::types::{AgentTool, AgentToolResult};

pub struct EditTool;

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
impl AgentTool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }
    fn requires_permission(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "Replace one occurrence (or all, with replace_all) of old_string with new_string in the file at path. Returns a unified diff of the change."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean", "default": false}
            },
            "required": ["path", "old_string", "new_string"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("missing 'path'")?;
        let old_s = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or("missing 'old_string'")?;
        let new_s = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or("missing 'new_string'")?;
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if old_s.is_empty() {
            return Err("old_string must not be empty".to_string());
        }

        let original = fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {path}: {e}"))?;
        let count = original.matches(old_s).count();
        if count == 0 {
            return Err(format!("old_string not found in {path}"));
        }
        if count > 1 && !replace_all {
            return Err(format!(
                "old_string occurs {count} times in {path}; pass replace_all=true or expand the match"
            ));
        }
        let replaced = if replace_all {
            original.replace(old_s, new_s)
        } else {
            original.replacen(old_s, new_s, 1)
        };
        // Preserve CRLF line endings if the original used them.
        let updated = if original.contains("\r\n") {
            replaced.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            replaced
        };
        // Resolve first so rename updates a symlink target instead of replacing the link.
        let destination = fs::canonicalize(path)
            .await
            .map_err(|e| format!("metadata {path}: {e}"))?;
        let existing_permissions = match fs::metadata(&destination).await {
            Ok(metadata) if metadata.file_type().is_file() => Some(metadata.permissions()),
            Ok(_) => None,
            Err(e) => return Err(format!("metadata {path}: {e}")),
        };

        let mut tmp_path = None;
        let mut tmp_file = None;
        for _ in 0..100 {
            let n = EDIT_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate = temporary_path(&destination, n);
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
        let tmp_guard = TempFileGuard {
            path: Some(tmp_path.clone()),
        };
        if let Err(e) = tmp_file.write_all(updated.as_bytes()).await {
            drop(tmp_file);
            return Err(format!("write {}: {e}", tmp_path.display()));
        }
        drop(tmp_file);
        if let Some(permissions) = existing_permissions {
            if let Err(e) = fs::set_permissions(&tmp_path, permissions).await {
                return Err(format!("set permissions {}: {e}", tmp_path.display()));
            }
        }
        if let Err(e) = fs::rename(&tmp_path, &destination).await {
            return Err(format!("rename {} to {path}: {e}", tmp_path.display()));
        }
        tmp_guard.disarm();
        let n = if replace_all { count } else { 1 };

        let diff = TextDiff::from_lines(&original, &updated)
            .unified_diff()
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();

        Ok(AgentToolResult {
            content: vec![
                Content::text(format!("edited {path}: {n} replacement(s)")),
                Content::text(diff),
            ],
            details: Value::Null,
            terminate: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn retries_existing_temporary_path_without_truncating_it() {
        EDIT_COUNTER.store(0, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-edit-collision-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("target.txt");
        let occupied = temporary_path(&path, 0);
        fs::write(&path, "original").await.unwrap();
        fs::write(&occupied, "sentinel").await.unwrap();

        EditTool
            .execute(
                "collision",
                json!({
                    "path": path,
                    "old_string": "original",
                    "new_string": "replacement"
                }),
            )
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(&occupied).await.unwrap(), "sentinel");
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "replacement");
        let _ = fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn rejects_empty_old_string_and_allows_empty_new_string() {
        let dir = std::env::temp_dir().join(format!(
            "pi-rs-edit-empty-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("target.txt");
        fs::write(&path, "original").await.unwrap();

        let error = EditTool
            .execute(
                "empty-old",
                json!({"path": path, "old_string": "", "new_string": "x"}),
            )
            .await
            .unwrap_err();
        assert!(error.contains("old_string must not be empty"));

        EditTool
            .execute(
                "empty-new",
                json!({"path": path, "old_string": "original", "new_string": ""}),
            )
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "");
        let _ = fs::remove_dir_all(dir).await;
    }
}
