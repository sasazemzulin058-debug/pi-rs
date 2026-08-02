use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

use crate::types::{AgentTool, AgentToolResult};

pub struct LsTool;

#[async_trait]
impl AgentTool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }
    fn description(&self) -> &str {
        "List entries in a directory. Returns name and kind (file/dir/symlink)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "limit": {"type": "integer", "default": 1000},
                "show_hidden": {"type": "boolean", "default": false}
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("missing 'path'")?;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(1000) as usize;
        let show_hidden = args
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut read = fs::read_dir(path)
            .await
            .map_err(|e| format!("ls {path}: {e}"))?;
        let mut entries = Vec::new();
        while let Some(entry) = read.next_entry().await.map_err(|e| e.to_string())? {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let ft = entry.file_type().await.map_err(|e| e.to_string())?;
            let kind = if ft.is_dir() {
                "dir"
            } else if ft.is_symlink() {
                "symlink"
            } else {
                "file"
            };
            entries.push((name, kind));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let truncated = entries.len() > limit;
        entries.truncate(limit);
        let mut buf = entries
            .into_iter()
            .map(|(name, kind)| format!("{kind}\t{name}\n"))
            .collect::<String>();
        if truncated {
            buf.push_str(&format!("... (truncated at {limit})\n"));
        }
        Ok(AgentToolResult::text(buf))
    }
}
