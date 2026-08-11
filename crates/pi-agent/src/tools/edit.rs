use async_trait::async_trait;
use pi_ai::Content;
use serde_json::{json, Value};
use similar::TextDiff;
use tokio::fs;

use crate::types::{AgentTool, AgentToolResult};

pub struct EditTool;

#[async_trait]
impl AgentTool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }
    fn requires_permission(&self) -> bool {
        true
    }
    fn description(&self) -> &str {
        "Replace exact text blocks in a file."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "edits": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {
                    "oldText": {"type": "string"}, "newText": {"type": "string"}
                }, "required": ["oldText", "newText"]}}
            },
            "required": ["path", "edits"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or("missing 'path'")?;
        let values = args
            .get("edits")
            .and_then(Value::as_array)
            .ok_or("missing 'edits'")?;
        if values.is_empty() {
            return Err("edits must not be empty".into());
        }
        let bytes = fs::read(path)
            .await
            .map_err(|e| format!("read {path}: {e}"))?;
        let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
        let body = if bom { &bytes[3..] } else { &bytes[..] };
        let original = String::from_utf8(body.to_vec()).map_err(|e| format!("read {path}: {e}"))?;
        let crlf = original.contains("\r\n");
        let normalized = original.replace("\r\n", "\n");
        let mut matches = Vec::with_capacity(values.len());
        for (i, value) in values.iter().enumerate() {
            let old = value
                .get("oldText")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("edits[{i}].oldText must be a string"))?;
            let new = value
                .get("newText")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("edits[{i}].newText must be a string"))?;
            if old.is_empty() {
                return Err(format!("edits[{i}].oldText must not be empty"));
            }
            let old = old.replace("\r\n", "\n");
            let found: Vec<_> = normalized.match_indices(&old).collect();
            if found.len() != 1 {
                return Err(if found.is_empty() {
                    format!("Could not find edits[{i}] in {path}. The oldText must match exactly including all whitespace and newlines.")
                } else {
                    format!("Found multiple matches for edits[{i}] in {path}. The oldText must match exactly one location.")
                });
            }
            matches.push((
                found[0].0,
                found[0].0 + old.len(),
                new.replace("\r\n", "\n"),
                i,
            ));
        }
        matches.sort_by_key(|m| m.0);
        for pair in matches.windows(2) {
            if pair[0].1 > pair[1].0 {
                return Err(format!("edits[{}] and edits[{}] overlap in {path}. Merge them into one edit or target disjoint regions.", pair[0].3, pair[1].3));
            }
        }
        let mut updated = normalized;
        for (start, end, new, _) in matches.into_iter().rev() {
            updated.replace_range(start..end, &new);
        }
        let updated = if crlf {
            updated.replace('\n', "\r\n")
        } else {
            updated
        };
        let mut output = Vec::with_capacity(updated.len() + if bom { 3 } else { 0 });
        if bom {
            output.extend_from_slice(&[0xef, 0xbb, 0xbf]);
        }
        output.extend_from_slice(updated.as_bytes());
        fs::write(path, &output)
            .await
            .map_err(|e| format!("write {path}: {e}"))?;
        let diff = TextDiff::from_lines(&original, &updated)
            .unified_diff()
            .header(&format!("a/{path}"), &format!("b/{path}"))
            .to_string();
        Ok(AgentToolResult {
            content: vec![
                Content::text(format!(
                    "Successfully replaced {} block(s) in {path}.",
                    values.len()
                )),
                Content::text(diff),
            ],
            details: Value::Null,
            terminate: false,
        })
    }
}
