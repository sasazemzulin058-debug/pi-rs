//! Session persistence: save transcripts as JSON under
//! `$XDG_CONFIG_HOME/pi-rs/sessions/<id>.json`, list them, and load by id.

use std::path::{Path, PathBuf};

use anyhow::Context;
use pi_ai::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionOrigin {
    Native,
    CopiedFromUpstream { source_session_id: String },
}

fn default_native() -> SessionOrigin {
    SessionOrigin::Native
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

pub fn save(config_dir: &Path, session: &Session) -> anyhow::Result<PathBuf> {
    let clean_id = validate_session_id(&session.id)?;
    let dir = sessions_dir(config_dir);
    check_sessions_dir_not_symlink(&dir)?;
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    check_sessions_dir_not_symlink(&dir)?;

    let path = dir.join(format!("{clean_id}.json"));
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("session file is a symlink: {}", path.display());
        }
    }

    let tmp_path = dir.join(format!("{clean_id}.json.tmp.{}", rand_u32()));
    if let Ok(meta) = std::fs::symlink_metadata(&tmp_path) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("temp session file is a symlink: {}", tmp_path.display());
        }
    }

    let json = serde_json::to_string_pretty(session)?;

    // ponytail: atomic rename provides crash-safety on POSIX/same filesystem; upgrade to lockfile or cross-dev fallback if needed.
    if let Err(e) = std::fs::write(&tmp_path, json) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e).with_context(|| format!("write {}", tmp_path.display()));
    }
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e)
            .with_context(|| format!("rename {} to {}", tmp_path.display(), path.display()));
    }
    Ok(path)
}

pub fn load(config_dir: &Path, id: &str) -> anyhow::Result<Session> {
    let path = session_file_path(config_dir, id)?;
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let s: Session = serde_json::from_str(&text)?;
    Ok(s)
}

pub fn save_jsonl(path: &Path, session: &Session) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let mut content = String::new();
    let header = serde_json::json!({
        "type": "session",
        "id": session.id,
        "created_ms": session.created_ms,
        "updated_ms": session.updated_ms,
        "model": session.model,
        "provider": session.provider,
        "origin": session.origin,
    });
    content.push_str(&serde_json::to_string(&header)?);
    content.push('\n');

    for msg in &session.messages {
        content.push_str(&serde_json::to_string(msg)?);
        content.push('\n');
    }

    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_jsonl(path: &Path) -> anyhow::Result<Session> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;

    let last_newline_boundary = text.rfind('\n').map(|p| p + 1).unwrap_or(0);

    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        anyhow::bail!("empty jsonl session file: {}", path.display());
    }

    let total = lines.len();
    let mut session: Option<Session> = None;
    let mut messages = Vec::new();
    let mut truncate_offset: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let is_last = idx == total - 1;
        let line_str = line.trim_end_matches('\r');

        if idx == 0 {
            let val: serde_json::Value = match serde_json::from_str(line_str) {
                Ok(v) => v,
                Err(e) => {
                    if is_last {
                        anyhow::bail!("empty header in jsonl session file: {}", path.display());
                    } else {
                        return Err(e).with_context(|| {
                            format!("malformed header line in {}", path.display())
                        });
                    }
                }
            };

            if val.get("type").and_then(|v| v.as_str()) != Some("session") {
                anyhow::bail!(
                    "invalid header type in jsonl session file: {}",
                    path.display()
                );
            }

            let id = val
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing session id in header: {}", path.display()))?
                .to_string();
            let created_ms = val.get("created_ms").and_then(|v| v.as_i64()).unwrap_or(0);
            let updated_ms = val.get("updated_ms").and_then(|v| v.as_i64()).unwrap_or(0);
            let model = val
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let provider = val
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let origin: SessionOrigin = val
                .get("origin")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(SessionOrigin::Native);

            session = Some(Session {
                id,
                created_ms,
                updated_ms,
                model,
                provider,
                messages: Vec::new(),
                origin,
            });
            continue;
        }

        match serde_json::from_str::<Message>(line_str) {
            Ok(msg) => messages.push(msg),
            Err(e) => {
                if !is_last || !e.is_eof() {
                    return Err(e).with_context(|| {
                        format!("malformed line {} in {}", idx + 1, path.display())
                    });
                }
                truncate_offset = Some(last_newline_boundary);
            }
        }
    }

    if let Some(trunc_len) = truncate_offset {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = f.set_len(trunc_len as u64);
        }
    }

    let mut session =
        session.ok_or_else(|| anyhow::anyhow!("missing header in {}", path.display()))?;
    session.messages = messages;
    Ok(session)
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
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
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
        let text = std::fs::read_to_string(&path)?;
        let s: Session = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => continue,
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
