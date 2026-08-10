use async_trait::async_trait;
use serde_json::{json, Value};
use std::convert::TryFrom;
use tokio::fs;

use crate::types::{AgentTool, AgentToolResult};

const MAX_LINES: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;

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
        let offset = match args.get("offset") {
            None => 0,
            Some(value) => value
                .as_u64()
                .ok_or("offset must be a positive integer")?
                .checked_sub(1)
                .ok_or("offset must be a positive integer")
                .and_then(|n| usize::try_from(n).map_err(|_| "offset is too large"))?,
        };
        let limit = args
            .get("limit")
            .map(|v| {
                v.as_u64()
                    .ok_or("limit must be a non-negative integer")
                    .and_then(|n| usize::try_from(n).map_err(|_| "limit is too large"))
            })
            .transpose()?;

        let text = fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {path}: {e}"))?;
        let all_lines: Vec<&str> = text.split('\n').collect();
        if offset >= all_lines.len() {
            return Err(format!(
                "Offset {} is beyond end of file ({} lines total)",
                args.get("offset").and_then(Value::as_u64).unwrap_or(1),
                all_lines.len()
            ));
        }

        let end = limit
            .map(|n| offset.saturating_add(n).min(all_lines.len()))
            .unwrap_or(all_lines.len());
        let selected = all_lines[offset..end].join("\n");
        let selected_lines = split_read_lines(&selected);
        let first_bytes = selected_lines.first().map(|line| line.len()).unwrap_or(0);
        if first_bytes > MAX_BYTES {
            let size = format_size(first_bytes);
            return Ok(AgentToolResult::text(format!(
                "[Line {} is {}, exceeds 50.0KB limit. Use bash: sed -n '{}p' {} | head -c 51200]",
                offset + 1,
                size,
                offset + 1,
                path
            )));
        }

        let mut output_lines = Vec::new();
        let mut bytes: usize = 0;
        for (index, line) in selected_lines.iter().enumerate() {
            let line_bytes = line.len() + usize::from(index > 0);
            if index >= MAX_LINES || bytes.saturating_add(line_bytes) > MAX_BYTES {
                break;
            }
            output_lines.push(*line);
            bytes += line_bytes;
        }
        let truncated = output_lines.len() < selected_lines.len();
        let output = if !truncated {
            selected.clone()
        } else {
            output_lines.join("\n")
        };
        let start_line = offset + 1;
        let end_line = start_line + output_lines.len().saturating_sub(1);
        let mut result = if truncated {
            let by_lines = output_lines.len() >= MAX_LINES;
            format!(
                "{}\n\n[Showing lines {}-{} of {}{} Use offset={} to continue.]",
                output,
                start_line,
                end_line,
                all_lines.len(),
                if by_lines { "." } else { " (50.0KB limit)." },
                end_line + 1
            )
        } else {
            output
        };
        if !truncated {
            if let Some(n) = limit {
                let consumed = offset.saturating_add(n).min(all_lines.len());
                if consumed < all_lines.len() {
                    result.push_str(&format!(
                        "\n\n[{} more lines in file. Use offset={} to continue.]",
                        all_lines.len() - consumed,
                        consumed + 1
                    ));
                }
            }
        }
        Ok(AgentToolResult::text(result))
    }
}

fn split_read_lines(content: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    if content.is_empty() {
        lines.clear();
    } else if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
