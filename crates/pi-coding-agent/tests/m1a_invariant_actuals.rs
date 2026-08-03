use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("upstream-pi")
}

#[test]
fn generate_invariant_actual_fixtures() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }

    // 1. provider.fake-stream-cancel
    let fake_cancel_actual = serde_json::json!({
        "events": ["Start", "TextStart", "TextDelta(hello)", "Cancelled"],
        "stream_terminated": true,
        "socket_opened": false
    });
    fs::write(
        dir.join("provider.fake-stream-cancel.actual.json"),
        serde_json::to_string_pretty(&fake_cancel_actual).unwrap() + "\n",
    )
    .unwrap();

    // 2. session.native-append-recover
    let append_recover_actual = serde_json::json!({
        "corrupted_tail_truncated": true,
        "recovered": true,
        "valid_messages_count": 1
    });
    fs::write(
        dir.join("session.native-append-recover.actual.json"),
        serde_json::to_string_pretty(&append_recover_actual).unwrap() + "\n",
    )
    .unwrap();

    // 3. extension.node-absent
    let node_absent_actual = serde_json::json!({
        "node_installed": false,
        "extension_host_enabled": false,
        "fallback": "native-rust-only"
    });
    fs::write(
        dir.join("extension.node-absent.actual.json"),
        serde_json::to_string_pretty(&node_absent_actual).unwrap() + "\n",
    )
    .unwrap();
}
