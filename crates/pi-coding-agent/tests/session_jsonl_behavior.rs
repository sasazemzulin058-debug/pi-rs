use std::path::{Path, PathBuf};

#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::{Message, Model};

fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pi-rs-session-jsonl-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn native_session(id: &str, texts: &[&str]) -> session::Session {
    let model = Model::anthropic_claude_sonnet_4_6();
    let mut value = session::Session::new(&model);
    value.id = id.into();
    value.messages = texts.iter().map(|text| Message::user_text(*text)).collect();
    value
}

fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn creates_versioned_header_and_deterministic_parent_chain() {
    let dir = temp_dir();
    let value = native_session("chain", &["one", "two", "three"]);
    let path = session::save(&dir, &value).unwrap();
    let records = lines(&path);

    assert_eq!(records.len(), 4);
    assert_eq!(records[0]["type"], "session");
    assert_eq!(records[0]["schema"], "pi-rs-session");
    assert_eq!(records[0]["version"], 1);
    assert_eq!(records[0]["id"], "chain");
    for record in &records[1..] {
        assert_eq!(record["type"], "entry");
        assert_eq!(record["version"], 1);
    }
    assert!(records[1]["parent_id"].is_null());
    assert_eq!(records[2]["parent_id"], records[1]["entry_id"]);
    assert_eq!(records[3]["parent_id"], records[2]["entry_id"]);

    let other_dir = temp_dir();
    let other_path = session::save(&other_dir, &value).unwrap();
    let other = lines(&other_path);
    assert_eq!(records[1]["entry_id"], other[1]["entry_id"]);
    assert_eq!(records[2]["entry_id"], other[2]["entry_id"]);

    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other_dir);
}

#[test]
fn append_preserves_the_exact_persisted_prefix() {
    let dir = temp_dir();
    let mut value = native_session("append", &["first"]);
    let path = session::save(&dir, &value).unwrap();
    let prefix = std::fs::read(&path).unwrap();

    value.messages.push(Message::user_text("second"));
    session::save(&dir, &value).unwrap();
    let appended = std::fs::read(&path).unwrap();
    assert!(appended.starts_with(&prefix));
    assert!(appended.len() > prefix.len());
    assert_eq!(session::load_jsonl(&path).unwrap().messages.len(), 2);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn incomplete_tail_is_truncated_but_malformed_middle_record_is_not_rewritten() {
    let dir = temp_dir();
    let value = native_session("recover", &["kept"]);
    let path = session::save(&dir, &value).unwrap();
    let complete = std::fs::read(&path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"entry\"")
        .unwrap();

    let loaded = session::load_jsonl(&path).unwrap();
    assert_eq!(loaded.messages.len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), complete);

    let mut malformed = complete.clone();
    let newline = malformed.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    malformed.splice(newline..newline, b"not-json\n".iter().copied());
    std::fs::write(&path, &malformed).unwrap();
    assert!(session::load_jsonl(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), malformed);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_waits_for_an_existing_lock_and_cleans_up_its_lock() {
    let dir = temp_dir();
    let value = native_session("locked", &["message"]);
    let path = session::session_file_path_jsonl(&dir, &value.id).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = PathBuf::from(format!("{}.lock", path.display()));
    std::fs::write(&lock, b"held").unwrap();
    let release = lock.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        std::fs::remove_file(release).unwrap();
    });

    session::save(&dir, &value).unwrap();
    thread.join().unwrap();
    assert!(path.exists());
    assert!(!lock.exists());

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn session_directory_file_and_lock_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let value = native_session("private", &["secret"]);
    let path = session::save(&dir, &value).unwrap();
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!PathBuf::from(format!("{}.lock", path.display())).exists());

    let _ = std::fs::remove_dir_all(dir);
}

use std::io::Write;
