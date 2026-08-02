use async_trait::async_trait;
use glob::{MatchOptions, Pattern};
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::path::Path;

use crate::types::{AgentTool, AgentToolResult};

pub struct GlobTool;

#[async_trait]
impl AgentTool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Expand a glob pattern (e.g. 'src/**/*.rs') and return matching paths. Honors .gitignore and skips hidden files by default."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "max": {"type": "integer", "default": 500},
                "show_hidden": {"type": "boolean", "default": false},
                "respect_gitignore": {"type": "boolean", "default": true}
            },
            "required": ["pattern"]
        })
    }
    async fn execute(&self, _id: &str, args: Value) -> Result<AgentToolResult, String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or("missing 'pattern'")?
            .to_string();
        let max = args.get("max").and_then(|v| v.as_u64()).unwrap_or(500) as usize;
        let show_hidden = args
            .get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let respect_gitignore = args
            .get("respect_gitignore")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let glob_pattern = Pattern::new(&pattern).map_err(|e| e.to_string())?;
            let options = MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: !show_hidden,
            };
            let wildcard = pattern.find(['*', '?', '[']).unwrap_or(pattern.len());
            let prefix = Path::new(&pattern[..wildcard]);
            let root = if prefix.is_dir() {
                prefix
            } else {
                prefix.parent().unwrap_or_else(|| Path::new("."))
            };
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let absolute_pattern = Path::new(&pattern).is_absolute();
            let mut paths = WalkBuilder::new(root)
                .hidden(!show_hidden)
                .standard_filters(respect_gitignore)
                .require_git(false)
                .follow_links(false)
                .build()
                .flatten()
                .filter_map(|entry| {
                    let path = entry.into_path();
                    let candidate = if absolute_pattern {
                        path.clone()
                    } else {
                        cwd.join(&path)
                            .strip_prefix(&cwd)
                            .unwrap_or(path.as_path())
                            .to_path_buf()
                    };
                    glob_pattern
                        .matches_path_with(&candidate, options)
                        .then_some(candidate)
                })
                .collect::<Vec<_>>();
            paths.sort();
            let truncated = paths.len() > max;
            paths.truncate(max);
            let mut buf = paths
                .into_iter()
                .map(|path| format!("{}\n", path.display()))
                .collect::<String>();
            if truncated {
                buf.push_str(&format!("... (truncated at {max})\n"));
            }
            Ok(buf)
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(AgentToolResult::text(if result.is_empty() {
            "(no matches)".to_string()
        } else {
            result
        }))
    }
}
