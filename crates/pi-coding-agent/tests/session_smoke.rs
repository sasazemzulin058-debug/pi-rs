//! Session round-trip smoke test against a temp config dir.

use std::path::PathBuf;

#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::{Message, Model};

fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pi-rs-session-test-{pid}-{nanos}-{count}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn session_id_validation_rejects_traversal_and_malformed() {
    let bad_ids = [
        "../etc/passwd",
        "..",
        ".",
        "foo/bar",
        "foo\\bar",
        "foo\0bar",
        "foo\nbar",
        "",
        &"a".repeat(129),
    ];

    for bad in bad_ids {
        assert!(
            session::validate_session_id(bad).is_err(),
            "expected error for id: {bad:?}"
        );
    }

    let good_ids = [
        "019fc623-1911-74cd-8e54-2861ae8c8bc0",
        "session-123_abc",
        "valid.id.name",
    ];

    for good in good_ids {
        assert_eq!(
            session::validate_session_id(good).unwrap(),
            good,
            "expected success for id: {good:?}"
        );
    }
}

#[test]
fn session_load_prevents_path_traversal() {
    let dir = temp_dir();
    let err_msg = session::load(&dir, "../outside").unwrap_err().to_string();
    assert!(
        err_msg.contains("path traversal") || err_msg.contains("invalid character"),
        "unexpected error message: {err_msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_and_load_roundtrip() {
    let dir = temp_dir();
    let model = Model::anthropic_claude_sonnet_4_6();
    let mut s = session::Session::new(&model);
    s.messages.push(Message::user_text("hello"));
    let path = session::save(&dir, &s).unwrap();
    assert!(path.exists());

    let loaded = session::load(&dir, &s.id).unwrap();
    assert_eq!(loaded.id, s.id);
    assert_eq!(loaded.messages.len(), 1);

    let list = session::list(&dir).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, s.id);
    assert_eq!(list[0].turns, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_crash_safe_temp_file_cleanup_and_overwrite() {
    let dir = temp_dir();
    let model = Model::anthropic_claude_sonnet_4_6();
    let mut s = session::Session::new(&model);
    s.messages.push(Message::user_text("initial"));

    // Save initial session
    let path = session::save(&dir, &s).unwrap();
    assert!(path.exists());

    // Check no temp files left in sessions directory
    let sessions_dir = dir.join("sessions");
    for entry in std::fs::read_dir(&sessions_dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains(".tmp."), "found leftover temp file: {name}");
    }

    // Overwrite with second save
    s.messages.push(Message::user_text("second turn"));
    session::save(&dir, &s).unwrap();

    // Verify overwritten content and no leftover temp files
    let loaded = session::load(&dir, &s.id).unwrap();
    assert_eq!(loaded.messages.len(), 2);

    for entry in std::fs::read_dir(&sessions_dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains(".tmp."), "found leftover temp file: {name}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
