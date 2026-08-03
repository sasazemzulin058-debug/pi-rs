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
        "valid.id.name",
        "session.json",
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
        "18e12345678-a1b2c3d4",
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
fn session_rejects_symlink_directory_and_files() {
    let config_dir = temp_dir();

    // Test 1: Reject if sessions directory is a symlink
    let target_dir = temp_dir();
    let symlink_sessions = session::sessions_dir(&config_dir);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target_dir, &symlink_sessions).unwrap();
        assert!(session::save(
            &config_dir,
            &session::Session::new(&Model::anthropic_claude_sonnet_4_6())
        )
        .is_err());
        assert!(session::load(&config_dir, "019fc623-1911-74cd-8e54-2861ae8c8bc0").is_err());
        assert!(session::list(&config_dir).is_err());
        std::fs::remove_file(&symlink_sessions).unwrap();
    }

    // Test 2: Reject if individual session file is a symlink
    let sessions_real_dir = session::sessions_dir(&config_dir);
    std::fs::create_dir_all(&sessions_real_dir).unwrap();
    let outside_file = target_dir.join("outside.json");
    std::fs::write(&outside_file, "{}").unwrap();
    let symlink_file = sessions_real_dir.join("session-symlink.json");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside_file, &symlink_file).unwrap();
        assert!(session::load(&config_dir, "session-symlink").is_err());
        assert!(session::save(
            &config_dir,
            &session::Session {
                id: "session-symlink".to_string(),
                created_ms: 0,
                updated_ms: 0,
                model: "m".into(),
                provider: "p".into(),
                messages: vec![],
                origin: session::SessionOrigin::Native,
            }
        )
        .is_err());
    }

    let _ = std::fs::remove_dir_all(&config_dir);
    let _ = std::fs::remove_dir_all(&target_dir);
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
