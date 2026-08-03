use std::path::PathBuf;

#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::{Message, Model};

fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "pi-rs-session-edges-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn value(id: &str, updated_ms: i64) -> session::Session {
    let mut value = session::Session::new(&Model::anthropic_claude_sonnet_4_6());
    value.id = id.into();
    value.updated_ms = updated_ms;
    value
        .messages
        .push(Message::user_text(format!("message-{id}")));
    value
}

#[test]
fn list_accepts_native_and_legacy_and_ignores_non_sessions() {
    let dir = temp_dir();
    let native = value("native", 20);
    session::save(&dir, &native).unwrap();

    let sessions = session::sessions_dir(&dir);
    let legacy = value("legacy", 10);
    std::fs::write(
        sessions.join("legacy.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    std::fs::write(sessions.join("malformed.json"), b"not json").unwrap();
    std::fs::write(sessions.join("unrelated.txt"), b"sentinel").unwrap();
    std::fs::write(sessions.join("native.jsonl.lock"), b"123").unwrap();
    std::fs::write(sessions.join("scratch.tmp"), b"temporary").unwrap();

    let listed = session::list(&dir).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["native", "legacy"]
    );
    assert_eq!(listed[0].first_message, "message-native");
    assert_eq!(listed[1].turns, 1);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delete_removes_only_the_resolved_file_and_missing_id_is_an_error() {
    let dir = temp_dir();
    let target = value("target", 1);
    let keep = value("keep", 2);
    let target_path = session::save(&dir, &target).unwrap();
    let keep_path = session::save(&dir, &keep).unwrap();
    let sentinel = dir.join("external-sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();

    assert_eq!(session::delete(&dir, "target").unwrap(), target_path);
    assert!(!target_path.exists());
    assert!(keep_path.exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
    assert!(session::delete(&dir, "missing").is_err());
    assert!(keep_path.exists());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delete_supports_legacy_json_without_touching_other_files() {
    let dir = temp_dir();
    let sessions = session::sessions_dir(&dir);
    std::fs::create_dir_all(&sessions).unwrap();
    let legacy_path = sessions.join("legacy.json");
    std::fs::write(
        &legacy_path,
        serde_json::to_vec(&value("legacy", 1)).unwrap(),
    )
    .unwrap();
    let unrelated = sessions.join("legacy.backup");
    std::fs::write(&unrelated, b"keep").unwrap();

    assert_eq!(session::delete(&dir, "legacy").unwrap(), legacy_path);
    assert!(!legacy_path.exists());
    assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn list_ignores_file_symlinks_and_delete_does_not_follow_them() {
    let dir = temp_dir();
    let sessions = session::sessions_dir(&dir);
    std::fs::create_dir_all(&sessions).unwrap();
    let sentinel = dir.join("sentinel");
    std::fs::write(&sentinel, b"outside").unwrap();
    let link = sessions.join("linked.jsonl");
    std::os::unix::fs::symlink(&sentinel, &link).unwrap();

    assert!(session::list(&dir).unwrap().is_empty());
    assert!(session::delete(&dir, "linked").is_err());
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"outside");

    let _ = std::fs::remove_dir_all(dir);
}
