//! Session persistence: save fork-native JSONL transcripts under
//! `$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl`, list them, and load by id.
//! Legacy JSON loads read-only; explicit upstream import APIs create native JSONL.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Context;
use pi_ai::Message;
use serde::{Deserialize, Serialize};

fn default_native() -> SessionOrigin {
    SessionOrigin::Native
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionOrigin {
    Native,
    CopiedFromUpstream { source_session_id: String },
}

pub const COMPACTION_SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";

pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

pub fn bash_execution_to_text(val: &serde_json::Value) -> String {
    let command = val.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let output = val.get("output").and_then(|v| v.as_str()).unwrap_or("");
    let cancelled = val
        .get("cancelled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let exit_code = val
        .get("exitCode")
        .or_else(|| val.get("exit_code"))
        .and_then(|v| v.as_i64());
    let truncated = val
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let full_output_path = val
        .get("fullOutputPath")
        .or_else(|| val.get("full_output_path"))
        .and_then(|v| v.as_str());

    let mut text = format!("Ran `{command}`\n");
    if !output.is_empty() {
        text.push_str(&format!("```\n{output}\n```"));
    } else {
        text.push_str("(no output)");
    }
    if cancelled {
        text.push_str("\n\n(command cancelled)");
    } else if let Some(code) = exit_code {
        if code != 0 {
            text.push_str(&format!("\n\nCommand exited with code {code}"));
        }
    }
    if truncated {
        if let Some(p) = full_output_path {
            text.push_str(&format!("\n\n[Output truncated. Full output: {p}]"));
        }
    }
    text
}

pub fn agent_message_to_llm(val: &serde_json::Value) -> anyhow::Result<Option<Message>> {
    let role = val.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = val
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(pi_ai::now_ms);

    match role {
        "bashExecution" => {
            if val
                .get("excludeFromContext")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                return Ok(None);
            }
            let text = bash_execution_to_text(val);
            Ok(Some(Message::User {
                content: vec![pi_ai::Content::Text {
                    text,
                    text_signature: None,
                }],
                timestamp,
            }))
        }
        "custom" => {
            let content_val = val.get("content");
            let content = match content_val {
                Some(v) if v.is_string() => {
                    vec![pi_ai::Content::Text {
                        text: v.as_str().unwrap().to_string(),
                        text_signature: None,
                    }]
                }
                Some(v) => serde_json::from_value::<Vec<pi_ai::Content>>(v.clone())
                    .map_err(|e| anyhow::anyhow!("invalid custom content: {e}"))?,
                None => Vec::new(),
            };
            Ok(Some(Message::User { content, timestamp }))
        }
        "branchSummary" => {
            let summary = val.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let text = format!("{BRANCH_SUMMARY_PREFIX}{summary}{BRANCH_SUMMARY_SUFFIX}");
            Ok(Some(Message::User {
                content: vec![pi_ai::Content::Text {
                    text,
                    text_signature: None,
                }],
                timestamp,
            }))
        }
        "compactionSummary" => {
            let summary = val.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let text = format!("{COMPACTION_SUMMARY_PREFIX}{summary}{COMPACTION_SUMMARY_SUFFIX}");
            Ok(Some(Message::User {
                content: vec![pi_ai::Content::Text {
                    text,
                    text_signature: None,
                }],
                timestamp,
            }))
        }
        "user" | "assistant" | "toolResult" => {
            let mut val_clone = val.clone();
            // Upstream user content string normalization for Message parsing
            if role == "user" {
                if let Some(obj) = val_clone.as_object_mut() {
                    if let Some(text) = obj
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                    {
                        obj.insert(
                            "content".into(),
                            serde_json::json!([{"type":"text", "text":text}]),
                        );
                    }
                }
            }
            let msg = serde_json::from_value::<Message>(val_clone)
                .map_err(|e| anyhow::anyhow!("invalid standard message: {e}"))?;
            Ok(Some(msg))
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentMessageValue(pub serde_json::Value);

#[derive(Debug, Clone)]
pub enum SessionEntry {
    Message {
        message: AgentMessageValue,
    },
    ModelChange {
        model_id: String,
        provider: String,
    },
    ThinkingLevelChange {
        thinking_level: String,
    },
    Compaction {
        summary: String,
        first_kept_entry_id: Option<String>,
        tokens_before: Option<u64>,
        retained_tail: Option<Vec<AgentMessageValue>>,
        details: Option<serde_json::Value>,
        usage: Option<serde_json::Value>,
        from_hook: Option<bool>,
    },
    Summary {
        summary: String,
        summarized_entry_ids: Vec<String>,
    },
    BranchSummary {
        summary: String,
        from_id: Option<String>,
        details: Option<serde_json::Value>,
        usage: Option<serde_json::Value>,
        from_hook: Option<bool>,
    },
    Custom {
        custom_type: String,
        data: serde_json::Value,
    },
    CustomMessage {
        custom_type: String,
        content: serde_json::Value,
        details: Option<serde_json::Value>,
        display: bool,
    },
    Label {
        target_id: String,
        label: Option<String>,
    },
    SessionInfo {
        name: Option<String>,
        info: serde_json::Value,
    },
    Unknown {
        raw: serde_json::Value,
    },
}

impl<'de> Deserialize<'de> for SessionEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        let rec_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match rec_type {
            "message" => {
                if let Some(msg_val) = val.get("message") {
                    if msg_val.is_object() {
                        return Ok(SessionEntry::Message {
                            message: AgentMessageValue(msg_val.clone()),
                        });
                    } else {
                        return Err(serde::de::Error::custom("message field must be an object"));
                    }
                } else if val.is_object() && val.get("role").is_some() {
                    return Ok(SessionEntry::Message {
                        message: AgentMessageValue(val.clone()),
                    });
                } else {
                    return Err(serde::de::Error::custom(
                        "message entry missing message object",
                    ));
                }
            }
            "model_change" | "model-change" => {
                let model_id = val
                    .get("modelId")
                    .or_else(|| val.get("model_id"))
                    .or_else(|| val.get("model"))
                    .and_then(|v| v.as_str());
                let provider = val.get("provider").and_then(|v| v.as_str());
                if let (Some(m), Some(p)) = (model_id, provider) {
                    if !m.is_empty() && !p.is_empty() {
                        return Ok(SessionEntry::ModelChange {
                            model_id: m.into(),
                            provider: p.into(),
                        });
                    }
                }
            }
            "thinking_level_change" | "thinking-level-change" => {
                if let Some(l) = val
                    .get("thinkingLevel")
                    .or_else(|| val.get("thinking_level"))
                    .and_then(|v| v.as_str())
                {
                    if !l.is_empty() {
                        return Ok(SessionEntry::ThinkingLevelChange {
                            thinking_level: l.into(),
                        });
                    }
                }
            }
            "compaction" => {
                if let Some(s) = val.get("summary").and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        let first_kept = val
                            .get("firstKeptEntryId")
                            .or_else(|| val.get("first_kept_entry_id"))
                            .and_then(|v| v.as_str())
                            .map(Into::into);
                        let tokens = val
                            .get("tokensBefore")
                            .or_else(|| val.get("tokens_before"))
                            .and_then(|v| v.as_u64());
                        let retained_tail =
                            match val.get("retainedTail").or_else(|| val.get("retained_tail")) {
                                Some(v) if v.is_array() => {
                                    let items = v.as_array().unwrap();
                                    let mut vec = Vec::new();
                                    for item in items {
                                        if item.is_object() {
                                            vec.push(AgentMessageValue(item.clone()));
                                        } else {
                                            return Err(serde::de::Error::custom(
                                                "retainedTail array elements must be objects",
                                            ));
                                        }
                                    }
                                    Some(vec)
                                }
                                Some(v) if v.is_null() => {
                                    return Err(serde::de::Error::custom(
                                        "retainedTail must not be null",
                                    ));
                                }
                                Some(_) => {
                                    return Err(serde::de::Error::custom(
                                        "retainedTail must be an array",
                                    ));
                                }
                                None => None,
                            };
                        let details = val.get("details").cloned();
                        let usage = val.get("usage").cloned();
                        let from_hook = match val.get("fromHook").or_else(|| val.get("from_hook")) {
                            Some(v) => match v.as_bool() {
                                Some(b) => Some(b),
                                None => {
                                    return Err(serde::de::Error::custom(
                                        "fromHook must be a boolean",
                                    ))
                                }
                            },
                            None => None,
                        };
                        return Ok(SessionEntry::Compaction {
                            summary: s.into(),
                            first_kept_entry_id: first_kept,
                            tokens_before: tokens,
                            retained_tail,
                            details,
                            usage,
                            from_hook,
                        });
                    }
                }
            }
            "summary" => {
                if let Some(s) = val.get("summary").and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        let ids = val
                            .get("summarizedEntryIds")
                            .or_else(|| val.get("summarized_entry_ids"))
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default();
                        return Ok(SessionEntry::Summary {
                            summary: s.into(),
                            summarized_entry_ids: ids,
                        });
                    }
                }
            }
            "branch_summary" | "branch-summary" => {
                if let Some(s) = val.get("summary").and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        let from_id = val
                            .get("fromId")
                            .or_else(|| val.get("from_id"))
                            .and_then(|v| v.as_str())
                            .map(Into::into);
                        let details = val.get("details").cloned();
                        let usage = val.get("usage").cloned();
                        let from_hook = match val.get("fromHook").or_else(|| val.get("from_hook")) {
                            Some(v) => match v.as_bool() {
                                Some(b) => Some(b),
                                None => {
                                    return Err(serde::de::Error::custom(
                                        "fromHook must be a boolean",
                                    ))
                                }
                            },
                            None => None,
                        };
                        return Ok(SessionEntry::BranchSummary {
                            summary: s.into(),
                            from_id,
                            details,
                            usage,
                            from_hook,
                        });
                    }
                }
            }
            "custom" => {
                if let Some(ct) = val
                    .get("customType")
                    .or_else(|| val.get("key"))
                    .and_then(|v| v.as_str())
                {
                    if !ct.is_empty() {
                        let data = val
                            .get("data")
                            .or_else(|| val.get("value"))
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        return Ok(SessionEntry::Custom {
                            custom_type: ct.into(),
                            data,
                        });
                    }
                }
            }
            "custom_message" | "custom-message" => {
                let custom_type = val
                    .get("customType")
                    .or_else(|| val.get("key"))
                    .and_then(|v| v.as_str());
                if let Some(ct) = custom_type {
                    if !ct.is_empty() {
                        let content = if let Some(c) = val.get("content") {
                            c.clone()
                        } else if let Some(m) = val.get("message") {
                            m.get("content").cloned().unwrap_or(m.clone())
                        } else {
                            val.clone()
                        };
                        let details = val.get("details").cloned();
                        let display = val.get("display").and_then(|v| v.as_bool()).unwrap_or(true);
                        return Ok(SessionEntry::CustomMessage {
                            custom_type: ct.into(),
                            content,
                            details,
                            display,
                        });
                    }
                }
            }
            "label" => {
                let label = val
                    .get("label")
                    .and_then(|v| if v.is_null() { None } else { v.as_str() })
                    .map(str::to_owned);
                let target_id = val
                    .get("targetId")
                    .or_else(|| val.get("target_id"))
                    .and_then(|v| v.as_str());
                if let Some(tid) = target_id {
                    if !tid.is_empty() {
                        return Ok(SessionEntry::Label {
                            label,
                            target_id: tid.into(),
                        });
                    }
                }
            }
            "session_info" | "session-info" => {
                let name = val.get("name").and_then(|v| v.as_str()).map(Into::into);
                let info = val.get("info").cloned().unwrap_or(serde_json::Value::Null);
                return Ok(SessionEntry::SessionInfo { name, info });
            }
            _ => {}
        }
        Ok(SessionEntry::Unknown { raw: val })
    }
}

impl Serialize for SessionEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SessionEntry::Message { message } => serde_json::json!({
                "type": "message",
                "message": message.0,
            })
            .serialize(serializer),
            SessionEntry::ModelChange { model_id, provider } => serde_json::json!({
                "type": "model_change",
                "modelId": model_id,
                "provider": provider,
            })
            .serialize(serializer),
            SessionEntry::ThinkingLevelChange { thinking_level } => serde_json::json!({
                "type": "thinking_level_change",
                "thinkingLevel": thinking_level,
            })
            .serialize(serializer),
            SessionEntry::Compaction {
                summary,
                first_kept_entry_id,
                tokens_before,
                retained_tail,
                details,
                usage,
                from_hook,
            } => {
                let mut v = serde_json::json!({
                    "type": "compaction",
                    "summary": summary,
                });
                if let Some(fk) = first_kept_entry_id {
                    v["firstKeptEntryId"] = serde_json::json!(fk);
                }
                if let Some(tb) = tokens_before {
                    v["tokensBefore"] = serde_json::json!(tb);
                }
                if let Some(rt) = retained_tail {
                    v["retainedTail"] = serde_json::json!(rt);
                }
                if let Some(d) = details {
                    v["details"] = d.clone();
                }
                if let Some(u) = usage {
                    v["usage"] = u.clone();
                }
                if let Some(fh) = from_hook {
                    v["fromHook"] = serde_json::json!(fh);
                }
                v.serialize(serializer)
            }
            SessionEntry::Summary {
                summary,
                summarized_entry_ids,
            } => serde_json::json!({
                "type": "summary",
                "summary": summary,
                "summarizedEntryIds": summarized_entry_ids,
            })
            .serialize(serializer),
            SessionEntry::BranchSummary {
                summary,
                from_id,
                details,
                usage,
                from_hook,
            } => {
                let mut v = serde_json::json!({
                    "type": "branch_summary",
                    "summary": summary,
                });
                if let Some(fid) = from_id {
                    v["fromId"] = serde_json::json!(fid);
                }
                if let Some(d) = details {
                    v["details"] = d.clone();
                }
                if let Some(u) = usage {
                    v["usage"] = u.clone();
                }
                if let Some(fh) = from_hook {
                    v["fromHook"] = serde_json::json!(fh);
                }
                v.serialize(serializer)
            }
            SessionEntry::Custom { custom_type, data } => serde_json::json!({
                "type": "custom",
                "customType": custom_type,
                "data": data,
            })
            .serialize(serializer),
            SessionEntry::CustomMessage {
                custom_type,
                content,
                details,
                display,
            } => {
                let mut v = serde_json::json!({
                    "type": "custom_message",
                    "customType": custom_type,
                    "content": content,
                    "display": display,
                });
                if let Some(d) = details {
                    v["details"] = d.clone();
                }
                v.serialize(serializer)
            }
            SessionEntry::Label { label, target_id } => {
                let mut v = serde_json::json!({
                    "type": "label",
                    "targetId": target_id,
                });
                if let Some(lbl) = label {
                    v["label"] = serde_json::json!(lbl);
                }
                v.serialize(serializer)
            }
            SessionEntry::SessionInfo { name, info } => {
                let mut v = serde_json::json!({
                    "type": "session_info",
                    "info": info,
                });
                if let Some(n) = name {
                    v["name"] = serde_json::json!(n);
                }
                v.serialize(serializer)
            }
            SessionEntry::Unknown { raw } => raw.serialize(serializer),
        }
    }
}

pub fn format_iso_timestamp(ms: i64) -> String {
    let secs = ms / 1000;
    let millis = (ms % 1000).abs();
    let days = secs / 86400;
    let rem_secs = (secs % 86400).abs();
    let hours = rem_secs / 3600;
    let rem_secs = rem_secs % 3600;
    let mins = rem_secs / 60;
    let secs = rem_secs % 60;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146097) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + (if m <= 2 { 1 } else { 0 });

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{secs:02}.{millis:03}Z")
}

pub fn parse_iso_timestamp(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4).and_then(|v| v.parse().ok())?;
    let month: i64 = s.get(5..7).and_then(|v| v.parse().ok())?;
    let day: i64 = s.get(8..10).and_then(|v| v.parse().ok())?;
    let hour: i64 = s.get(11..13).and_then(|v| v.parse().ok())?;
    let min: i64 = s.get(14..16).and_then(|v| v.parse().ok())?;
    let sec: i64 = s.get(17..19).and_then(|v| v.parse().ok())?;

    let millis: i64 = if s.len() >= 23 && s.as_bytes()[19] == b'.' {
        s[20..23].parse().unwrap_or(0)
    } else {
        0
    };

    let (m, y) = if month <= 2 {
        (month + 9, year - 1)
    } else {
        (month - 3, year)
    };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + (doy as u32);
    let days = (era * 146097 + doe as i64) - 719468;

    let total_secs = days * 86400 + hour * 3600 + min * 60 + sec;
    Some(total_secs * 1000 + millis)
}

#[derive(Debug, Clone)]
pub struct SessionTreeNode {
    pub entry_id: String,
    pub parent_id: Option<String>,
    pub source_entry_id: Option<String>,
    pub timestamp_ms: Option<i64>,
    pub timestamp_str: Option<String>,
    pub entry: SessionEntry,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionTreeMetadata {
    pub session_id: Option<String>,
    pub source_session_id: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub created_ms: Option<i64>,
    pub updated_ms: Option<i64>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionTree {
    nodes: Vec<SessionTreeNode>,
    index: HashMap<String, usize>,
    active_leaf: Option<String>,
    pub metadata: SessionTreeMetadata,
}

impl SessionTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert existing linear Session persistence into native tree form.
    pub fn cow_from_imported(imported: &SessionTree) -> Self {
        let mut cow = imported.clone();
        let now = pi_ai::now_ms();
        cow.metadata.source_session_id = cow.metadata.session_id.clone();
        cow.metadata.session_id = Some(new_id());
        cow.metadata.created_ms = Some(now);
        cow.metadata.updated_ms = Some(now);
        cow
    }

    pub fn from_session(session: &Session) -> anyhow::Result<Self> {
        let mut tree = Self::new();
        tree.metadata = SessionTreeMetadata {
            session_id: Some(session.id.clone()),
            model: Some(session.model.clone()),
            provider: Some(session.provider.clone()),
            created_ms: Some(session.created_ms),
            updated_ms: Some(session.updated_ms),
            cwd: None,
            source_session_id: match &session.origin {
                SessionOrigin::CopiedFromUpstream { source_session_id } => {
                    Some(source_session_id.clone())
                }
                SessionOrigin::Native => None,
            },
        };
        let mut parent = None;
        for message in &session.messages {
            let msg_val = serde_json::to_value(message)?;
            let id = tree.append(
                parent.as_deref(),
                SessionEntry::Message {
                    message: AgentMessageValue(msg_val),
                },
            )?;
            parent = Some(id);
        }
        Ok(tree)
    }

    /// Convert active tree branch to existing Session persistence.
    pub fn to_session(&self, session: &mut Session) -> anyhow::Result<()> {
        let leaf = self.active_leaf.as_deref();
        let messages = match leaf {
            Some(id) => self.effective_messages(id)?,
            None => Vec::new(),
        };
        session.replace_messages(messages);
        Ok(())
    }

    pub fn active_leaf(&self) -> Option<&str> {
        self.active_leaf.as_deref()
    }

    pub fn is_leaf(&self, entry_id: &str) -> bool {
        self.index
            .get(entry_id)
            .and_then(|&i| self.nodes.get(i))
            .is_some_and(|node| node.children.is_empty())
    }

    pub fn active_path(&self) -> anyhow::Result<Vec<&SessionTreeNode>> {
        let leaf_id = match &self.active_leaf {
            Some(id) => id.as_str(),
            None => return Ok(Vec::new()),
        };
        let mut path = Vec::new();
        let mut current = Some(leaf_id);
        while let Some(id) = current {
            let idx = *self
                .index
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown entry: {id}"))?;
            let node = &self.nodes[idx];
            path.push(node);
            current = node.parent_id.as_deref();
        }
        path.reverse();
        Ok(path)
    }

    pub fn save_active_branch_jsonl(&self, path: &Path) -> anyhow::Result<()> {
        let leaf_id = self
            .active_leaf
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("no active leaf in tree"))?;
        anyhow::ensure!(
            self.is_leaf(leaf_id),
            "cannot save non-leaf active entry '{leaf_id}': active branch saving requires a leaf entry"
        );

        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("tree path has no parent"))?;
        ensure_private_dir(parent)?;
        let _lock = lock_session(path)?;
        let mut file = std::fs::File::create(path)?;

        let now_iso = format_iso_timestamp(pi_ai::now_ms());
        let session_id = self
            .metadata
            .session_id
            .clone()
            .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
            .unwrap_or_else(|| "session".into());

        let header_json = serde_json::json!({
            "type": "session",
            "version": TREE_SCHEMA_VERSION,
            "id": session_id,
            "timestamp": now_iso,
            "cwd": self.metadata.cwd.as_deref().unwrap_or("/"),
            "active_leaf": leaf_id,
            "model": self.metadata.model,
            "provider": self.metadata.provider,
            "parentSession": self.metadata.source_session_id,
        });
        writeln!(file, "{}", serde_json::to_string(&header_json)?)?;

        for node in self.active_path()? {
            let timestamp = node.timestamp_str.clone().unwrap_or_else(|| {
                let ms = node.timestamp_ms.unwrap_or_else(pi_ai::now_ms);
                format_iso_timestamp(ms)
            });
            let mut entry_val = serde_json::to_value(&node.entry)?;
            if let Some(obj) = entry_val.as_object_mut() {
                obj.insert("id".into(), serde_json::json!(node.entry_id));
                obj.insert("parentId".into(), serde_json::json!(node.parent_id));
                obj.insert("timestamp".into(), serde_json::json!(timestamp));
                if let Some(source_id) = &node.source_entry_id {
                    obj.insert("source_entry_id".into(), serde_json::json!(source_id));
                }
            }
            writeln!(file, "{}", serde_json::to_string(&entry_val)?)?;
        }
        file.sync_all()?;
        Ok(())
    }

    pub fn nodes(&self) -> impl Iterator<Item = &SessionTreeNode> {
        self.nodes.iter()
    }

    pub fn to_json(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "active_leaf": self.active_leaf,
            "metadata": self.metadata,
            "entries": self.nodes.iter().map(|node| serde_json::json!({
                "entry_id": node.entry_id,
                "parent_id": node.parent_id,
                "source_entry_id": node.source_entry_id,
                "timestamp_ms": node.timestamp_ms,
                "timestamp_str": node.timestamp_str,
                "entry": node.entry,
            })).collect::<Vec<_>>(),
        }))
    }

    pub fn from_json(value: serde_json::Value) -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        struct Wire {
            active_leaf: Option<String>,
            metadata: Option<SessionTreeMetadata>,
            entries: Vec<WireEntry>,
        }
        #[derive(Deserialize)]
        struct WireEntry {
            entry_id: String,
            parent_id: Option<String>,
            entry: SessionEntry,
            source_entry_id: Option<String>,
            timestamp_ms: Option<i64>,
            timestamp_str: Option<String>,
        }
        let wire: Wire = serde_json::from_value(value)?;
        let mut tree = Self::new();
        if let Some(meta) = wire.metadata {
            tree.metadata = meta;
        }
        for item in wire.entries {
            let expected_parent = item.parent_id.as_deref();
            let actual = tree.append_with_id(expected_parent, item.entry_id.clone(), item.entry)?;
            anyhow::ensure!(actual == item.entry_id, "duplicate tree entry id");
            if let Some(node) = tree.nodes.last_mut() {
                node.source_entry_id = item.source_entry_id;
                node.timestamp_str = item.timestamp_str.clone();
                node.timestamp_ms = item
                    .timestamp_ms
                    .or_else(|| item.timestamp_str.as_deref().and_then(parse_iso_timestamp));
            }
        }
        if let Some(leaf) = wire.active_leaf {
            tree.set_active_leaf(&leaf)
                .with_context(|| format!("invalid active_leaf: {leaf}"))?;
        }
        Ok(tree)
    }

    fn append_with_id(
        &mut self,
        parent_id: Option<&str>,
        entry_id: String,
        entry: SessionEntry,
    ) -> anyhow::Result<String> {
        if let Some(parent) = parent_id {
            anyhow::ensure!(
                self.index.contains_key(parent),
                "unknown parent entry: {parent}"
            );
        } else {
            anyhow::ensure!(self.nodes.is_empty(), "root entry already exists");
        }
        anyhow::ensure!(
            !self.index.contains_key(&entry_id),
            "duplicate tree entry id"
        );
        let now_ms = pi_ai::now_ms();
        let index = self.nodes.len();
        self.nodes.push(SessionTreeNode {
            entry_id: entry_id.clone(),
            parent_id: parent_id.map(str::to_owned),
            source_entry_id: Some(entry_id.clone()),
            timestamp_ms: Some(now_ms),
            timestamp_str: Some(format_iso_timestamp(now_ms)),
            entry,
            children: Vec::new(),
        });
        self.index.insert(entry_id.clone(), index);
        if let Some(parent) = parent_id {
            self.nodes[*self.index.get(parent).expect("checked parent")]
                .children
                .push(entry_id.clone());
        }
        self.active_leaf = Some(entry_id.clone());
        Ok(entry_id)
    }

    pub fn append(
        &mut self,
        parent_id: Option<&str>,
        entry: SessionEntry,
    ) -> anyhow::Result<String> {
        if let Some(parent) = parent_id {
            anyhow::ensure!(
                self.index.contains_key(parent),
                "unknown parent entry: {parent}"
            );
        } else {
            anyhow::ensure!(self.nodes.is_empty(), "root entry already exists");
        }
        let entry_id = format!("e{}", self.nodes.len());
        let now_ms = pi_ai::now_ms();
        let index = self.nodes.len();
        self.nodes.push(SessionTreeNode {
            entry_id: entry_id.clone(),
            parent_id: parent_id.map(str::to_owned),
            source_entry_id: None,
            timestamp_ms: Some(now_ms),
            timestamp_str: Some(format_iso_timestamp(now_ms)),
            entry,
            children: Vec::new(),
        });
        self.index.insert(entry_id.clone(), index);
        if let Some(parent) = parent_id {
            self.nodes[*self.index.get(parent).expect("checked parent")]
                .children
                .push(entry_id.clone());
        }
        self.active_leaf = Some(entry_id.clone());
        Ok(entry_id)
    }

    pub fn branch_at(&mut self, entry_id: &str, entry: SessionEntry) -> anyhow::Result<String> {
        anyhow::ensure!(
            self.index.contains_key(entry_id),
            "unknown branch entry: {entry_id}"
        );
        self.append(Some(entry_id), entry)
    }

    pub fn set_active_leaf(&mut self, entry_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.index.contains_key(entry_id),
            "unknown leaf entry: {entry_id}"
        );
        self.active_leaf = Some(entry_id.to_owned());
        Ok(())
    }

    pub fn effective_context(&self, leaf_id: &str) -> anyhow::Result<Vec<&SessionEntry>> {
        let mut path = Vec::new();
        let mut current = Some(leaf_id);
        while let Some(id) = current {
            let node = self
                .index
                .get(id)
                .and_then(|i| self.nodes.get(*i))
                .ok_or_else(|| anyhow::anyhow!("unknown entry: {id}"))?;
            path.push((id, &node.entry));
            current = node.parent_id.as_deref();
        }
        path.reverse();

        // Compaction resolution: find latest compaction entry on leaf path
        let last_compaction_pos = path
            .iter()
            .rposition(|(_, entry)| matches!(entry, SessionEntry::Compaction { .. }));

        let mut result = Vec::new();
        if let Some(comp_idx) = last_compaction_pos {
            let comp_entry = path[comp_idx].1;
            result.push(comp_entry);

            if let SessionEntry::Compaction {
                retained_tail: Some(_),
                ..
            } = comp_entry
            {
                // If retained_tail is present on compaction, it acts as a self-contained checkpoint.
                // Preceding history is omitted.
                for (_, entry) in &path[comp_idx + 1..] {
                    result.push(*entry);
                }
            } else {
                let first_kept_id = match comp_entry {
                    SessionEntry::Compaction {
                        first_kept_entry_id,
                        ..
                    } => first_kept_entry_id.as_deref(),
                    _ => None,
                };

                if let Some(fk) = first_kept_id {
                    let mut found_first_kept = false;
                    for (id, entry) in &path[..comp_idx] {
                        if !found_first_kept && Some(*id) == Some(fk) {
                            found_first_kept = true;
                        }
                        if found_first_kept {
                            result.push(*entry);
                        }
                    }
                }
                for (_, entry) in &path[comp_idx + 1..] {
                    result.push(*entry);
                }
            }
        } else {
            for (_, entry) in path {
                result.push(entry);
            }
        }

        Ok(result)
    }

    pub fn effective_agent_messages(
        &self,
        leaf_id: &str,
    ) -> anyhow::Result<Vec<AgentMessageValue>> {
        let mut path: Vec<(&SessionTreeNode, &SessionEntry)> = Vec::new();
        let mut current = Some(leaf_id);
        while let Some(id) = current {
            let node = self
                .index
                .get(id)
                .and_then(|i| self.nodes.get(*i))
                .ok_or_else(|| anyhow::anyhow!("unknown entry: {id}"))?;
            path.push((node, &node.entry));
            current = node.parent_id.as_deref();
        }
        path.reverse();

        let last_compaction_pos = path
            .iter()
            .rposition(|(_, entry)| matches!(entry, SessionEntry::Compaction { .. }));

        let mut context_entries = Vec::new();
        if let Some(comp_idx) = last_compaction_pos {
            let comp_pair = path[comp_idx];
            context_entries.push(comp_pair);

            if let SessionEntry::Compaction {
                retained_tail: Some(_),
                ..
            } = comp_pair.1
            {
                for pair in &path[comp_idx + 1..] {
                    context_entries.push(*pair);
                }
            } else {
                let first_kept_id = match comp_pair.1 {
                    SessionEntry::Compaction {
                        first_kept_entry_id,
                        ..
                    } => first_kept_entry_id.as_deref(),
                    _ => None,
                };

                if let Some(fk) = first_kept_id {
                    let mut found_first_kept = false;
                    for pair in &path[..comp_idx] {
                        if !found_first_kept && Some(pair.0.entry_id.as_str()) == Some(fk) {
                            found_first_kept = true;
                        }
                        if found_first_kept {
                            context_entries.push(*pair);
                        }
                    }
                }
                for pair in &path[comp_idx + 1..] {
                    context_entries.push(*pair);
                }
            }
        } else {
            for pair in path {
                context_entries.push(pair);
            }
        }

        let mut result = Vec::new();
        for (node, entry) in context_entries {
            let ts = node.timestamp_ms.unwrap_or_else(pi_ai::now_ms);
            match entry {
                SessionEntry::Message { message } => {
                    let mut val = message.0.clone();
                    if let Some(obj) = val.as_object_mut() {
                        obj.entry("timestamp".to_string())
                            .or_insert_with(|| serde_json::json!(ts));
                    }
                    result.push(AgentMessageValue(val));
                }
                SessionEntry::CustomMessage {
                    custom_type,
                    content,
                    details,
                    display,
                } => {
                    let mut obj = serde_json::json!({
                        "role": "custom",
                        "customType": custom_type,
                        "content": content,
                        "display": display,
                        "timestamp": ts,
                    });
                    if let Some(d) = details {
                        obj["details"] = d.clone();
                    }
                    result.push(AgentMessageValue(obj));
                }
                SessionEntry::Compaction {
                    summary,
                    tokens_before,
                    retained_tail,
                    ..
                } => {
                    let mut comp_msg = serde_json::json!({
                        "role": "compactionSummary",
                        "summary": summary,
                        "timestamp": ts,
                    });
                    if let Some(tb) = tokens_before {
                        comp_msg["tokensBefore"] = serde_json::json!(tb);
                    }
                    result.push(AgentMessageValue(comp_msg));
                    if let Some(tail) = retained_tail {
                        result.extend(tail.clone());
                    }
                }
                SessionEntry::BranchSummary {
                    summary, from_id, ..
                } => {
                    let mut branch_msg = serde_json::json!({
                        "role": "branchSummary",
                        "summary": summary,
                        "timestamp": ts,
                    });
                    if let Some(fid) = from_id {
                        branch_msg["fromId"] = serde_json::json!(fid);
                    }
                    result.push(AgentMessageValue(branch_msg));
                }
                _ => {}
            }
        }

        Ok(result)
    }

    pub fn effective_messages(&self, leaf_id: &str) -> anyhow::Result<Vec<Message>> {
        let agent_msgs = self.effective_agent_messages(leaf_id)?;
        let mut messages = Vec::new();
        for am in agent_msgs {
            if let Some(msg) = agent_message_to_llm(&am.0)? {
                messages.push(msg);
            }
        }
        Ok(messages)
    }
}

const TREE_SCHEMA_VERSION: u32 = 3;

pub fn save_tree_jsonl(path: &Path, tree: &SessionTree) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("tree path has no parent"))?;
    ensure_private_dir(parent)?;
    let _lock = lock_session(path)?;
    let mut file = std::fs::File::create(path)?;

    let now_iso = format_iso_timestamp(pi_ai::now_ms());
    let session_id = tree
        .metadata
        .session_id
        .clone()
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
        .unwrap_or_else(|| "session".into());

    let header_json = serde_json::json!({
        "type": "session",
        "version": TREE_SCHEMA_VERSION,
        "id": session_id,
        "timestamp": now_iso,
        "cwd": tree.metadata.cwd.as_deref().unwrap_or("/"),
        "active_leaf": tree.active_leaf,
        "model": tree.metadata.model,
        "provider": tree.metadata.provider,
        "parentSession": tree.metadata.source_session_id,
    });
    writeln!(file, "{}", serde_json::to_string(&header_json)?)?;

    for node in tree.nodes() {
        let timestamp = node.timestamp_str.clone().unwrap_or_else(|| {
            let ms = node.timestamp_ms.unwrap_or_else(pi_ai::now_ms);
            format_iso_timestamp(ms)
        });
        let mut entry_val = serde_json::to_value(&node.entry)?;
        if let Some(obj) = entry_val.as_object_mut() {
            obj.insert("id".into(), serde_json::json!(node.entry_id));
            obj.insert("parentId".into(), serde_json::json!(node.parent_id));
            obj.insert("timestamp".into(), serde_json::json!(timestamp));
            if let Some(source_id) = &node.source_entry_id {
                obj.insert("source_entry_id".into(), serde_json::json!(source_id));
            }
        }
        writeln!(file, "{}", serde_json::to_string(&entry_val)?)?;
    }
    file.sync_all()?;
    Ok(())
}

pub fn load_tree_jsonl(path: &Path) -> anyhow::Result<SessionTree> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header_line = lines.next().ok_or_else(|| anyhow::anyhow!("empty tree"))?;
    let header: serde_json::Value = serde_json::from_str(header_line)?;

    anyhow::ensure!(
        header.get("type").and_then(|v| v.as_str()) == Some("session"),
        "invalid header record type"
    );

    let version = header.get("version").and_then(|v| v.as_u64()).unwrap_or(1);
    anyhow::ensure!(
        version == 1 || version == 3,
        "unsupported session version: {version}"
    );

    let metadata = SessionTreeMetadata {
        session_id: header.get("id").and_then(|v| v.as_str()).map(str::to_owned),
        source_session_id: header
            .get("parentSession")
            .or_else(|| header.get("source_session_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        model: header
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        provider: header
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        cwd: header
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        created_ms: None,
        updated_ms: None,
    };

    let active_leaf_from_header = header
        .get("active_leaf")
        .or_else(|| header.get("activeLeaf"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let mut tree_entries = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)?;
        let (entry_id, parent_id, timestamp_str, source_entry_id, entry) =
            if value.get("type").and_then(|v| v.as_str()) == Some("entry")
                && value.get("entry").is_some()
            {
                let id = value
                    .get("entry_id")
                    .or_else(|| value.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing entry_id"))?
                    .to_string();
                let parent = value
                    .get("parent_id")
                    .or_else(|| value.get("parentId"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let ts = value
                    .get("timestamp")
                    .or_else(|| value.get("timestamp_str"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let source_id = value
                    .get("source_entry_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let entry_obj: SessionEntry = serde_json::from_value(value["entry"].clone())?;
                (id, parent, ts, source_id, entry_obj)
            } else {
                let id = value
                    .get("id")
                    .or_else(|| value.get("entry_id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing entry id"))?
                    .to_string();
                let parent = value
                    .get("parentId")
                    .or_else(|| value.get("parent_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let ts = value
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let source_id = value
                    .get("source_entry_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let entry_obj: SessionEntry = serde_json::from_value(value.clone())?;
                (id, parent, ts, source_id, entry_obj)
            };

        tree_entries.push(serde_json::json!({
            "entry_id": entry_id,
            "parent_id": parent_id,
            "timestamp_str": timestamp_str,
            "source_entry_id": source_entry_id,
            "entry": entry,
        }));
    }

    let tree = SessionTree::from_json(serde_json::json!({
        "active_leaf": active_leaf_from_header,
        "metadata": metadata,
        "entries": tree_entries,
    }))?;
    Ok(tree)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub model: String,
    pub provider: String,
    pub messages: Vec<Message>,
    #[serde(default = "default_native")]
    pub origin: SessionOrigin,
}

impl Session {
    pub fn new(model: &pi_ai::Model) -> Self {
        let now = pi_ai::now_ms();
        Self {
            id: new_id(),
            created_ms: now,
            updated_ms: now,
            model: model.id.clone(),
            provider: model.provider.clone(),
            messages: Vec::new(),
            origin: SessionOrigin::Native,
        }
    }

    pub fn cow_from(upstream: &Session) -> Self {
        let now = pi_ai::now_ms();
        Self {
            id: new_id(),
            created_ms: now,
            updated_ms: now,
            model: upstream.model.clone(),
            provider: upstream.provider.clone(),
            messages: upstream.messages.clone(),
            origin: SessionOrigin::CopiedFromUpstream {
                source_session_id: upstream.id.clone(),
            },
        }
    }

    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.updated_ms = pi_ai::now_ms();
    }
}

pub fn validate_session_id(id: &str) -> anyhow::Result<&str> {
    if id.is_empty() {
        anyhow::bail!("session id cannot be empty");
    }
    if id.len() > 128 {
        anyhow::bail!("session id exceeds maximum length (128 characters)");
    }
    // Reject path traversal, dots, directory separators, control chars, null bytes, backslashes, non-alphanumeric/hyphen/underscore
    for c in id.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' {
            anyhow::bail!("invalid character in session id: {c:?}");
        }
    }
    Ok(id)
}

pub fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("sessions")
}

pub fn check_sessions_dir_not_symlink(dir: &Path) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("sessions directory is a symlink: {}", dir.display());
        }
    }
    Ok(())
}

pub fn session_file_path(config_dir: &Path, id: &str) -> anyhow::Result<PathBuf> {
    let clean_id = validate_session_id(id)?;
    let dir = sessions_dir(config_dir);
    check_sessions_dir_not_symlink(&dir)?;

    let path = dir.join(format!("{clean_id}.json"));

    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("session file is a symlink: {}", path.display());
        }
    }

    // Ensure resolved path is strictly contained within sessions_dir
    if let (Ok(canonical_dir), Ok(canonical_path)) = (dir.canonicalize(), path.canonicalize()) {
        if !canonical_path.starts_with(&canonical_dir) {
            anyhow::bail!("session path escapes session directory");
        }
    }
    Ok(path)
}

pub fn session_file_path_jsonl(config_dir: &Path, id: &str) -> anyhow::Result<PathBuf> {
    let clean_id = validate_session_id(id)?;
    let dir = sessions_dir(config_dir);
    check_sessions_dir_not_symlink(&dir)?;

    let path = dir.join(format!("{clean_id}.jsonl"));

    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("session file is a symlink: {}", path.display());
        }
    }

    if let (Ok(canonical_dir), Ok(canonical_path)) = (dir.canonicalize(), path.canonicalize()) {
        if !canonical_path.starts_with(&canonical_dir) {
            anyhow::bail!("session path escapes session directory");
        }
    }
    Ok(path)
}

pub fn save(config_dir: &Path, session: &Session) -> anyhow::Result<PathBuf> {
    let path = session_file_path_jsonl(config_dir, &session.id)?;
    save_jsonl(&path, session)?;
    Ok(path)
}

pub fn load(config_dir: &Path, id: &str) -> anyhow::Result<Session> {
    let jsonl_path = session_file_path_jsonl(config_dir, id)?;
    if jsonl_path.exists() {
        return load_jsonl(&jsonl_path);
    }
    let legacy_path = session_file_path(config_dir, id)?;
    let text = std::fs::read_to_string(&legacy_path)
        .with_context(|| format!("read {}", legacy_path.display()))?;
    let s: Session = serde_json::from_str(&text)?;
    Ok(s)
}

pub fn delete(config_dir: &Path, id: &str) -> anyhow::Result<PathBuf> {
    let jsonl_path = session_file_path_jsonl(config_dir, id)?;
    if jsonl_path.exists() {
        std::fs::remove_file(&jsonl_path)?;
        return Ok(jsonl_path);
    }
    let legacy_path = session_file_path(config_dir, id)?;
    std::fs::remove_file(&legacy_path)?;
    Ok(legacy_path)
}

const NATIVE_SCHEMA: &str = "pi-rs-session";
const NATIVE_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct NativeEntry {
    #[serde(rename = "type")]
    record_type: String,
    version: u32,
    entry_id: String,
    parent_id: Option<String>,
    message: Message,
}

struct SessionLock(PathBuf);
impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn lock_session(path: &Path) -> anyhow::Result<SessionLock> {
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&lock_path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                file.sync_all()?;
                return Ok(SessionLock(lock_path));
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("acquire session lock {}", lock_path.display()))
            }
        }
    }
}

fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("mkdir {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn entry_id(index: usize, message: &Message) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(message)?;
    Ok(format!("e{index:016x}-{}", &compute_sha256(&bytes)[..16]))
}

pub fn save_jsonl(path: &Path, session: &Session) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("session path has no parent"))?;
    ensure_private_dir(parent)?;
    let _lock = lock_session(path)?;

    let (persisted, native) = if path.exists() {
        let loaded = load_jsonl_unlocked(path)?;
        let native = native_file(path)?;
        (loaded.messages, native)
    } else {
        (Vec::new(), true)
    };
    if !native {
        anyhow::bail!("cannot append to legacy or unversioned JSONL session; save under a new id");
    }
    if persisted.len() > session.messages.len()
        || persisted
            .iter()
            .zip(&session.messages)
            .any(|(a, b)| serde_json::to_value(a).ok() != serde_json::to_value(b).ok())
    {
        anyhow::bail!("persisted session history diverges from supplied session");
    }

    let new_file = !path.exists();
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    if new_file {
        let header = serde_json::json!({"type":"session","schema":NATIVE_SCHEMA,"version":NATIVE_SCHEMA_VERSION,
            "id":session.id,"created_ms":session.created_ms,"updated_ms":session.updated_ms,
            "model":session.model,"provider":session.provider,"origin":session.origin});
        writeln!(file, "{}", serde_json::to_string(&header)?)?;
    }
    for (index, message) in session.messages.iter().enumerate().skip(persisted.len()) {
        let record = NativeEntry {
            record_type: "entry".into(),
            version: NATIVE_SCHEMA_VERSION,
            entry_id: entry_id(index, message)?,
            parent_id: if index == 0 {
                None
            } else {
                Some(entry_id(index - 1, &session.messages[index - 1])?)
            },
            message: message.clone(),
        };
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
    }
    file.flush()?;
    file.sync_all()?;
    // The file is durable here. Directory fsync is not uniformly available on supported platforms.
    Ok(())
}

fn native_file(path: &Path) -> anyhow::Result<bool> {
    let mut text = String::new();
    std::fs::File::open(path)?.read_to_string(&mut text)?;
    let first = text
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty jsonl session file"))?;
    let val: serde_json::Value = serde_json::from_str(first)?;
    Ok(val.get("schema").and_then(|v| v.as_str()) == Some(NATIVE_SCHEMA))
}

pub fn load_jsonl(path: &Path) -> anyhow::Result<Session> {
    let _lock = lock_session(path)?;
    load_jsonl_unlocked(path)
}

fn load_jsonl_unlocked(path: &Path) -> anyhow::Result<Session> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        anyhow::bail!("empty jsonl session file: {}", path.display());
    }
    let complete_len = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1);
    if complete_len < bytes.len() {
        let tail = &bytes[complete_len..];
        match serde_json::from_slice::<serde_json::Value>(tail) {
            Err(e) if e.is_eof() => {
                file.set_len(complete_len as u64)?;
                file.seek(SeekFrom::Start(complete_len as u64))?;
                file.sync_all()?;
                bytes.truncate(complete_len);
            }
            Err(e) => return Err(e).context("malformed complete final JSONL record"),
            Ok(_) => anyhow::bail!("complete final JSONL record is missing newline"),
        }
    }
    let text = std::str::from_utf8(&bytes)?;
    let mut lines = text.lines();
    let header: serde_json::Value =
        serde_json::from_str(lines.next().unwrap()).context("malformed session header")?;
    if header.get("type").and_then(|v| v.as_str()) != Some("session") {
        anyhow::bail!("invalid header type in {}", path.display());
    }
    let native = header.get("schema").and_then(|v| v.as_str()) == Some(NATIVE_SCHEMA);
    if native
        && header.get("version").and_then(|v| v.as_u64()) != Some(NATIVE_SCHEMA_VERSION as u64)
    {
        anyhow::bail!("unsupported native session schema version");
    }
    let mut messages = Vec::new();
    let mut previous: Option<String> = None;
    for (index, line) in lines.enumerate() {
        if native {
            let entry: NativeEntry = serde_json::from_str(line)
                .with_context(|| format!("malformed line {}", index + 2))?;
            if entry.record_type != "entry"
                || entry.version != NATIVE_SCHEMA_VERSION
                || entry.parent_id != previous
            {
                anyhow::bail!("invalid native entry chain at line {}", index + 2);
            }
            let expected = entry_id(index, &entry.message)?;
            if entry.entry_id != expected {
                anyhow::bail!("invalid native entry id at line {}", index + 2);
            }
            previous = Some(entry.entry_id);
            messages.push(entry.message);
        } else {
            messages.push(
                serde_json::from_str(line)
                    .with_context(|| format!("malformed line {}", index + 2))?,
            );
        }
    }
    Ok(Session {
        id: header
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing session id"))?
            .into(),
        created_ms: header
            .get("created_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        updated_ms: header
            .get("updated_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        model: header
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        provider: header
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        origin: header
            .get("origin")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(SessionOrigin::Native),
        messages,
    })
}

/// Read-only Pi session import representation.
#[derive(Debug, Clone)]
pub struct PiSessionImport {
    pub session_id: String,
    pub model: String,
    pub provider: String,
    pub messages: Vec<Message>,
    pub checksum_sha256: String,
    pub source_path: PathBuf,
}

/// Computes raw SHA-256 hex digest of file contents if checksum verification is requested.
/// Pure std/stdlib SHA-256 implementation to avoid external dependency additions.
pub fn compute_sha256(bytes: &[u8]) -> String {
    // SHA-256 constants
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (bytes.len() as u64) * 8;
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h_val] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut out = String::with_capacity(64);
    for val in h {
        out.push_str(&format!("{val:08x}"));
    }
    out
}

pub fn verify_pi_checksum(path: &Path, expected_sha256: &str) -> anyhow::Result<bool> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let actual = compute_sha256(&bytes);
    Ok(actual.eq_ignore_ascii_case(expected_sha256))
}

pub fn import_pi_session(path: &Path) -> anyhow::Result<PiSessionImport> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let checksum_sha256 = compute_sha256(&bytes);
    let text = String::from_utf8(bytes).with_context(|| format!("utf8 {}", path.display()))?;

    // Try single JSON format first
    if let Ok(session) = serde_json::from_str::<Session>(&text) {
        return Ok(PiSessionImport {
            session_id: session.id,
            model: session.model,
            provider: session.provider,
            messages: session.messages,
            checksum_sha256,
            source_path: path.to_path_buf(),
        });
    }

    // JSONL line parsing
    let mut session_id = String::new();
    let mut model = String::new();
    let mut provider = String::new();
    let mut messages = Vec::new();
    let mut recognized_records = 0;

    let lines: Vec<&str> = text.split('\n').collect();
    let lines_to_process = if lines.last().is_some_and(|l| l.trim().is_empty()) {
        &lines[..lines.len() - 1]
    } else {
        &lines[..]
    };

    let total = lines_to_process.len();

    for (idx, line) in lines_to_process.iter().enumerate() {
        let is_last = idx == total - 1;
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            anyhow::bail!("empty line at line {} in {}", idx + 1, path.display());
        }

        let val: serde_json::Value = match serde_json::from_str(line_trimmed) {
            Ok(v) => v,
            Err(e) => {
                if is_last && e.is_eof() {
                    // Incomplete final JSON line tolerated during import
                    break;
                }
                return Err(e).with_context(|| {
                    format!("malformed JSON at line {} in {}", idx + 1, path.display())
                });
            }
        };

        let rec_type = val.get("type").and_then(|v| v.as_str());
        if rec_type == Some("session") {
            if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
                session_id = id.to_string();
            }
            if let Some(m) = val.get("model").and_then(|v| v.as_str()) {
                model = m.to_string();
            }
            if let Some(p) = val.get("provider").and_then(|v| v.as_str()) {
                provider = p.to_string();
            }
            recognized_records += 1;
            continue;
        }

        if rec_type == Some("model_change") {
            if let Some(m) = val
                .get("modelId")
                .or_else(|| val.get("model"))
                .and_then(|v| v.as_str())
            {
                model = m.to_string();
            }
            if let Some(p) = val.get("provider").and_then(|v| v.as_str()) {
                provider = p.to_string();
            }
            recognized_records += 1;
            continue;
        }

        // Try outer Message or nested message field (e.g. type: "message", message: {...})
        let mut msg_val = if rec_type == Some("message") {
            val.get("message").cloned().unwrap_or(val)
        } else {
            val
        };

        // Convert string user/assistant content to Content::Text array if string format
        if let Some(obj) = msg_val.as_object_mut() {
            if let Some(c_str) = obj
                .get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
            {
                obj.insert(
                    "content".to_string(),
                    serde_json::json!([{"type": "text", "text": c_str}]),
                );
            }
        }

        match serde_json::from_value::<Message>(msg_val) {
            Ok(msg) => {
                messages.push(msg);
                recognized_records += 1;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "invalid message record at line {} in {}",
                        idx + 1,
                        path.display()
                    )
                });
            }
        }
    }

    if recognized_records == 0 || (session_id.is_empty() && messages.is_empty()) {
        anyhow::bail!("No valid Pi session records found in {}", path.display());
    }

    if session_id.is_empty() {
        anyhow::bail!("Missing session header in {}", path.display());
    }

    if session_id.is_empty() {
        session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported-session")
            .to_string();
    }

    Ok(PiSessionImport {
        session_id,
        model,
        provider,
        messages,
        checksum_sha256,
        source_path: path.to_path_buf(),
    })
}

pub fn import_pi_session_as_tree(path: &Path) -> anyhow::Result<SessionTree> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut tree = SessionTree::new();
    let mut parent: Option<String> = None;
    let mut explicit_active_leaf = None;
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("malformed JSON at line {}", line_no + 1))?;
        let record_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if record_type == "session" {
            let created_ms = value
                .get("created_ms")
                .or_else(|| value.get("timestamp_ms"))
                .and_then(|v| v.as_i64())
                .or_else(|| value.get("timestamp").and_then(|v| v.as_i64()));
            tree.metadata = SessionTreeMetadata {
                session_id: value.get("id").and_then(|v| v.as_str()).map(str::to_owned),
                source_session_id: value
                    .get("parentSession")
                    .or_else(|| value.get("parent_session"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                model: value
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                provider: value
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                cwd: value.get("cwd").and_then(|v| v.as_str()).map(str::to_owned),
                created_ms,
                updated_ms: None,
            };
            if let Some(leaf) = value
                .get("active_leaf")
                .or_else(|| value.get("activeLeaf"))
                .and_then(|v| v.as_str())
            {
                explicit_active_leaf = Some(leaf.to_string());
            }
            continue;
        }
        let entry_id = value
            .get("id")
            .or_else(|| value.get("entryId"))
            .or_else(|| value.get("entry_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("e{}", tree.nodes.len()));
        let source_entry_id = value
            .get("sourceEntryId")
            .or_else(|| value.get("source_entry_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| Some(entry_id.clone()));
        let parent_id = value
            .get("parentEntryId")
            .or_else(|| value.get("parentId"))
            .or_else(|| value.get("parent_id"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or(parent.clone());
        let timestamp_ms = value
            .get("timestamp_ms")
            .and_then(|v| v.as_i64())
            .or_else(|| value.get("timestamp").and_then(|v| v.as_i64()));
        let timestamp_str = value
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or_else(|| timestamp_ms.map(format_iso_timestamp));

        let entry = match record_type {
            "model_change" | "model-change" => {
                let model_id = value
                    .get("modelId")
                    .or_else(|| value.get("model_id"))
                    .or_else(|| value.get("model"))
                    .and_then(|v| v.as_str());
                let provider = value.get("provider").and_then(|v| v.as_str());
                match (model_id, provider) {
                    (Some(m), Some(p)) if !m.is_empty() && !p.is_empty() => {
                        SessionEntry::ModelChange {
                            model_id: m.into(),
                            provider: p.into(),
                        }
                    }
                    _ => SessionEntry::Unknown { raw: value.clone() },
                }
            }
            "thinking_level_change" | "thinking-level-change" => match value
                .get("thinkingLevel")
                .or_else(|| value.get("thinking_level"))
                .and_then(|v| v.as_str())
            {
                Some(level) if !level.is_empty() => SessionEntry::ThinkingLevelChange {
                    thinking_level: level.into(),
                },
                _ => SessionEntry::Unknown { raw: value.clone() },
            },
            "compaction" => match value.get("summary").and_then(|v| v.as_str()) {
                Some(summary) if !summary.is_empty() => {
                    let retained_tail_val = value
                        .get("retainedTail")
                        .or_else(|| value.get("retained_tail"));
                    let retained_tail = match retained_tail_val {
                        Some(v) if v.is_array() => {
                            let mut vec = Vec::new();
                            for item in v.as_array().unwrap() {
                                if item.is_object() {
                                    vec.push(AgentMessageValue(item.clone()));
                                } else {
                                    anyhow::bail!("malformed compaction retainedTail array element at line {}", line_no + 1);
                                }
                            }
                            Some(vec)
                        }
                        Some(v) if v.is_null() => {
                            anyhow::bail!("retainedTail must not be null at line {}", line_no + 1)
                        }
                        Some(_) => anyhow::bail!(
                            "malformed compaction retainedTail non-array value at line {}",
                            line_no + 1
                        ),
                        None => None,
                    };
                    let details = value.get("details").cloned();
                    let usage = value.get("usage").cloned();
                    let from_hook = match value.get("fromHook").or_else(|| value.get("from_hook")) {
                        Some(v) => match v.as_bool() {
                            Some(b) => Some(b),
                            None => anyhow::bail!(
                                "malformed compaction fromHook at line {}",
                                line_no + 1
                            ),
                        },
                        None => None,
                    };
                    SessionEntry::Compaction {
                        summary: summary.into(),
                        first_kept_entry_id: value
                            .get("firstKeptEntryId")
                            .or_else(|| value.get("first_kept_entry_id"))
                            .and_then(|v| v.as_str())
                            .map(Into::into),
                        tokens_before: value
                            .get("tokensBefore")
                            .or_else(|| value.get("tokens_before"))
                            .and_then(|v| v.as_u64()),
                        retained_tail,
                        details,
                        usage,
                        from_hook,
                    }
                }
                _ => SessionEntry::Unknown { raw: value.clone() },
            },
            "message" => {
                let msg_val = if let Some(m) = value.get("message") {
                    if m.is_null() || !m.is_object() {
                        anyhow::bail!(
                            "line {}: message entry message field must be an object",
                            line_no + 1
                        );
                    }
                    m.clone()
                } else if value.is_object() && value.get("role").is_some() {
                    value.clone()
                } else {
                    anyhow::bail!("line {}: message entry missing message object", line_no + 1);
                };
                SessionEntry::Message {
                    message: AgentMessageValue(msg_val),
                }
            }
            "summary" => match value.get("summary").and_then(|v| v.as_str()) {
                Some(summary) if !summary.is_empty() => SessionEntry::Summary {
                    summary: summary.into(),
                    summarized_entry_ids: value
                        .get("summarizedEntryIds")
                        .or_else(|| value.get("summarized_entry_ids"))
                        .and_then(|v| v.as_array())
                        .map(|ids| {
                            ids.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                _ => SessionEntry::Unknown { raw: value.clone() },
            },
            "branch_summary" | "branch-summary" => {
                match value.get("summary").and_then(|v| v.as_str()) {
                    Some(summary) if !summary.is_empty() => {
                        let details = value.get("details").cloned();
                        let usage = value.get("usage").cloned();
                        let from_hook =
                            match value.get("fromHook").or_else(|| value.get("from_hook")) {
                                Some(v) => match v.as_bool() {
                                    Some(b) => Some(b),
                                    None => anyhow::bail!(
                                        "malformed branch_summary fromHook at line {}",
                                        line_no + 1
                                    ),
                                },
                                None => None,
                            };
                        SessionEntry::BranchSummary {
                            summary: summary.into(),
                            from_id: value
                                .get("fromId")
                                .or_else(|| value.get("from_id"))
                                .and_then(|v| v.as_str())
                                .map(Into::into),
                            details,
                            usage,
                            from_hook,
                        }
                    }
                    _ => SessionEntry::Unknown { raw: value.clone() },
                }
            }
            "custom" => {
                let custom_type = value
                    .get("customType")
                    .or_else(|| value.get("key"))
                    .and_then(|v| v.as_str());
                match custom_type {
                    Some(ct) if !ct.is_empty() => SessionEntry::Custom {
                        custom_type: ct.into(),
                        data: value
                            .get("data")
                            .or_else(|| value.get("value"))
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    },
                    _ => SessionEntry::Unknown { raw: value.clone() },
                }
            }
            "custom_message" | "custom-message" => {
                let custom_type = value
                    .get("customType")
                    .or_else(|| value.get("key"))
                    .and_then(|v| v.as_str());
                if let Some(ct) = custom_type {
                    if !ct.is_empty() {
                        let content = if let Some(c) = value.get("content") {
                            c.clone()
                        } else if let Some(m) = value.get("message") {
                            m.get("content").cloned().unwrap_or(m.clone())
                        } else {
                            value.clone()
                        };
                        let details = value.get("details").cloned();
                        let display = value
                            .get("display")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        SessionEntry::CustomMessage {
                            custom_type: ct.into(),
                            content,
                            details,
                            display,
                        }
                    } else {
                        SessionEntry::Unknown { raw: value.clone() }
                    }
                } else {
                    SessionEntry::Unknown { raw: value.clone() }
                }
            }
            "label" => {
                let label = value
                    .get("label")
                    .and_then(|v| if v.is_null() { None } else { v.as_str() })
                    .map(str::to_owned);
                let target_id = value
                    .get("targetId")
                    .or_else(|| value.get("target_id"))
                    .and_then(|v| v.as_str());
                match target_id {
                    Some(tid) if !tid.is_empty() => SessionEntry::Label {
                        label,
                        target_id: tid.into(),
                    },
                    _ => SessionEntry::Unknown { raw: value.clone() },
                }
            }
            "session_info" | "session-info" => SessionEntry::SessionInfo {
                name: value.get("name").and_then(|v| v.as_str()).map(Into::into),
                info: value
                    .get("info")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            },
            _ => SessionEntry::Unknown { raw: value.clone() },
        };
        tree.append_with_id(parent_id.as_deref(), entry_id.clone(), entry)?;
        if let Some(node) = tree.nodes.last_mut() {
            node.source_entry_id = source_entry_id;
            node.timestamp_ms =
                timestamp_ms.or_else(|| timestamp_str.as_deref().and_then(parse_iso_timestamp));
            node.timestamp_str = timestamp_str;
        }
        parent = Some(entry_id);
    }
    if let Some(leaf) = explicit_active_leaf {
        tree.set_active_leaf(&leaf)
            .with_context(|| format!("invalid active leaf: {leaf}"))?;
    } else if tree.active_leaf.is_none() {
        tree.active_leaf = parent;
    }
    Ok(tree)
}

pub fn import_as_cow(import: &PiSessionImport) -> Session {
    let now = pi_ai::now_ms();
    Session {
        id: new_id(),
        created_ms: now,
        updated_ms: now,
        model: import.model.clone(),
        provider: import.provider.clone(),
        messages: import.messages.clone(),
        origin: SessionOrigin::CopiedFromUpstream {
            source_session_id: import.session_id.clone(),
        },
    }
}

pub fn list(config_dir: &Path) -> anyhow::Result<Vec<SessionSummary>> {
    let dir = sessions_dir(config_dir);
    check_sessions_dir_not_symlink(&dir)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<SessionSummary> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str());
        if ext != Some("json") && ext != Some("jsonl") {
            continue;
        }
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() {
                continue;
            }
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if validate_session_id(stem).is_err() {
            continue;
        }
        let s = if ext == Some("jsonl") {
            match load_jsonl(&path) {
                Ok(s) => s,
                Err(_) => continue,
            }
        } else {
            let text = std::fs::read_to_string(&path)?;
            match serde_json::from_str::<Session>(&text) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };
        let first_user = s
            .messages
            .iter()
            .find_map(|m| match m {
                Message::User { content, .. } => content
                    .iter()
                    .find_map(|c| c.as_text().map(|s| s.to_string())),
                _ => None,
            })
            .unwrap_or_default();
        out.push(SessionSummary {
            id: s.id,
            updated_ms: s.updated_ms,
            model: s.model,
            provider: s.provider,
            first_message: first_user,
            turns: s.messages.len(),
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_ms));
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub updated_ms: i64,
    pub model: String,
    #[allow(dead_code)] // exposed for callers, not yet rendered.
    pub provider: String,
    pub first_message: String,
    pub turns: usize,
}

fn new_id() -> String {
    let now = pi_ai::now_ms();
    let suffix: u32 = rand_u32();
    format!("{now:x}-{suffix:08x}")
}

// Tiny xorshift PRNG seeded from time — we don't pull in `rand` just for this.
fn rand_u32() -> u32 {
    use std::cell::Cell;
    thread_local!(static STATE: Cell<u32> = const { Cell::new(0) });
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            x = (now as u32) ^ 0x9E37_79B9;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s.set(x);
        x
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-rs-session-test-{name}-{}", rand_u32()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_import_pi_session_jsonl_extended() {
        let dir = temp_test_dir("jsonl-ext");
        let file_path = dir.join("session.jsonl");
        let mut f = std::fs::File::create(&file_path).unwrap();

        writeln!(
            f,
            r#"{{"type":"session","id":"s123","model":"gpt-4","provider":"openai"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"model_change","modelId":"claude-3-5-sonnet","provider":"anthropic"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"message","message":{{"role":"user","content":"hello"}}}}"#
        )
        .unwrap();
        drop(f);

        let imported = import_pi_session(&file_path).unwrap();
        assert_eq!(imported.session_id, "s123");
        assert_eq!(imported.model, "claude-3-5-sonnet");
        assert_eq!(imported.provider, "anthropic");
        assert_eq!(imported.messages.len(), 1);

        let empty_path = dir.join("empty.jsonl");
        std::fs::File::create(&empty_path).unwrap();
        assert!(import_pi_session(&empty_path).is_err());

        let header_only_path = dir.join("header_only.jsonl");
        std::fs::write(&header_only_path, r#"{"type":"session"}"#).unwrap();
        assert!(import_pi_session(&header_only_path).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_load_jsonl_roundtrip() {
        let dir = temp_test_dir("jsonl-roundtrip");
        let file_path = dir.join("session.jsonl");

        let model = pi_ai::Model::openai_compat(
            "openai",
            "gpt-4o",
            "https://api.openai.com/v1",
            128_000,
            4096,
        );
        let mut session = Session::new(&model);
        session.messages.push(Message::user_text("Hello"));
        session
            .messages
            .push(Message::Assistant(pi_ai::AssistantMessage {
                content: vec![pi_ai::Content::text("Hi there")],
                api: "openai-chat".into(),
                provider: "openai".into(),
                model: "gpt-4o".into(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                raw_stop_reason: None,
                usage: Default::default(),
                stop_reason: pi_ai::StopReason::Stop,
                error_message: None,
                timestamp: pi_ai::now_ms(),
            }));

        save_jsonl(&file_path, &session).unwrap();
        let loaded = load_jsonl(&file_path).unwrap();

        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.model, session.model);
        assert_eq!(loaded.provider, session.provider);
        assert_eq!(loaded.messages.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_jsonl_truncated_final_line_tolerated() {
        let dir = temp_test_dir("jsonl-truncated-final");
        let file_path = dir.join("session.jsonl");

        let header = r#"{"type":"session","id":"s_trunc","model":"m","provider":"p"}"#;
        let msg1 = serde_json::to_string(&Message::user_text("valid msg")).unwrap();
        let truncated_msg = r#"{"type":"assistant","content":"incom"#;

        let content = format!("{header}\n{msg1}\n{truncated_msg}");
        std::fs::write(&file_path, content).unwrap();

        let loaded = load_jsonl(&file_path).unwrap();
        assert_eq!(loaded.id, "s_trunc");
        assert_eq!(loaded.messages.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_jsonl_malformed_interior_line_errors() {
        let dir = temp_test_dir("jsonl-malformed-interior");
        let file_path = dir.join("session.jsonl");

        let header = r#"{"type":"session","id":"s_bad_mid","model":"m","provider":"p"}"#;
        let malformed = r#"{"role":"user", invalid_json"#;
        let msg2 = serde_json::to_string(&Message::user_text("valid msg")).unwrap();

        let content = format!("{header}\n{malformed}\n{msg2}");
        std::fs::write(&file_path, content).unwrap();

        let res = load_jsonl(&file_path);
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_jsonl_complete_malformed_final_message_errors() {
        let dir = temp_test_dir("jsonl-malformed-final-msg");
        let file_path = dir.join("session.jsonl");

        let header = r#"{"type":"session","id":"s_bad_final","model":"m","provider":"p"}"#;
        let valid_msg = serde_json::to_string(&Message::user_text("valid msg")).unwrap();
        // Valid JSON object, but malformed as a Message (missing required fields / incompatible shape)
        let malformed_msg = r#"{"unknown_field": 123}"#;

        let content = format!("{header}\n{valid_msg}\n{malformed_msg}");
        std::fs::write(&file_path, content).unwrap();

        let res = load_jsonl(&file_path);
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_jsonl_invalid_header_type_errors() {
        let dir = temp_test_dir("jsonl-invalid-header-type");
        let file_path = dir.join("session.jsonl");

        let header_wrong_type = r#"{"type":"other","id":"s_wrong","model":"m","provider":"p"}"#;
        std::fs::write(&file_path, header_wrong_type).unwrap();

        let res = load_jsonl(&file_path);
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_jsonl_missing_header_id_errors() {
        let dir = temp_test_dir("jsonl-missing-header-id");
        let file_path = dir.join("session.jsonl");

        let header_no_id = r#"{"type":"session","model":"m","provider":"p"}"#;
        std::fs::write(&file_path, header_no_id).unwrap();

        let res = load_jsonl(&file_path);
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
