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
    let dir = std::env::temp_dir().join(format!("pi-rs-jsonl-test-{pid}-{nanos}-{count}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn jsonl_save_and_load_roundtrip() {
    let dir = temp_dir();
    let model = Model::anthropic_claude_sonnet_4_6();
    let mut s = session::Session::new(&model);
    s.messages.push(Message::user_text("hello jsonl"));
    let path = session::save(&dir, &s).unwrap();
    assert!(path.exists());
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("jsonl"));

    let loaded = session::load(&dir, &s.id).unwrap();
    assert_eq!(loaded.id, s.id);
    assert_eq!(loaded.messages.len(), 1);

    let list = session::list(&dir).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, s.id);

    session::delete(&dir, &s.id).unwrap();
    assert!(!path.exists());

    let _ = std::fs::remove_dir_all(&dir);
}
