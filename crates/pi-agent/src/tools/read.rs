use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

use crate::types::{AgentTool, AgentToolResult};

pub struct ReadTool;

#[async_trait]
impl AgentTool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    fn description(&self) -> &str {
        "Read the contents of a file from disk. Returns text content with optional line numbers."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Absolute or relative path to the file"},
                "offset": {"type": "integer", "description": "Line offset (1-based), optional"},
                "limit": {"type": "integer", "description": "Max number of lines, optional"}
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("missing 'path'")?;
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let text = fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {path}: {e}"))?;
        let lines: Vec<&str> = text.lines().collect();

        if lines.is_empty() {
            return Ok(AgentToolResult::text(""));
        }

        // Standard Pi bounds defaults: max 400 lines, max 50KB total output
        let max_lines = limit.unwrap_or(400);
        let start = offset
            .map(|o| o.saturating_sub(1))
            .unwrap_or(0);

        if start >= lines.len() {
            return Err(format!(
                "offset {start} is beyond end of file (file has {} lines)",
                lines.len()
            ));
        }

        let max_end = start.saturating_add(max_lines);
        let end = std::cmp::min(max_end, lines.len());

        let max_bytes = 50 * 1024;
        let mut buf = String::new();
        let mut lines_read = 0;
        let mut truncated = false;

        for (i, line) in lines[start..end].iter().enumerate() {
            let line_fmt = format!("{:>5}\t{}\n", start + i + 1, line);
            if buf.len() + line_fmt.len() > max_bytes {
                truncated = true;
                break;
            }
            buf.push_str(&line_fmt);
            lines_read += 1;
        }

        if start + lines_read < lines.len() {
            truncated = true;
        }

        if truncated {
            let remaining = lines.len().saturating_sub(start + lines_read);
            if remaining > 0 {
                let suffix = format!("... ({} more lines, use offset to continue)\n", remaining);
                if buf.len() + suffix.len() <= max_bytes {
                    buf.push_str(&suffix);
                } else if buf.len() < max_bytes {
                    // Fits at least truncated suffix or clip buffer to remain strictly <= 50 KiB total output.
                    let available = max_bytes.saturating_sub(suffix.len());
                    if buf.len() > available {
                        let mut cutoff = available;
                        while cutoff > 0 && !buf.is_char_boundary(cutoff) {
                            cutoff -= 1;
                        }
                        buf.truncate(cutoff);
                    }
                    buf.push_str(&suffix);
                }
            }
        }

        Ok(AgentToolResult::text(buf))
    }
}
