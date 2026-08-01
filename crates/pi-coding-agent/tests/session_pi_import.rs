//! Tests for Pi session import, SHA-256 checksum verification, and import-to-COW integration.

use std::path::PathBuf;

#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::Message;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pi-rs-session-import-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PI_V3_JSONL_FIXTURE: &str = r#"{"type":"session","id":"01abc","version":"v3","model":"claude-sonnet-4-6","provider":"anthropic","created_ms":1700000000000}
{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hello"}],"timestamp":1700000000001}}
{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"model":"claude-sonnet-4-6","provider":"anthropic","api":"anthropic-messages","usage":{},"stop_reason":"stop","timestamp":1700000000002}}
"#;

#[test]
fn sha256_known_vector() {
    let hash = session::compute_sha256(b"hello world");
    assert_eq!(
        hash,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn import_pi_session_jsonl_roundtrip() {
    let dir = temp_dir();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, PI_V3_JSONL_FIXTURE).unwrap();

    let import = session::import_pi_session(&path).unwrap();
    assert_eq!(import.session_id, "01abc");
    assert_eq!(import.model, "claude-sonnet-4-6");
    assert_eq!(import.provider, "anthropic");
    assert_eq!(import.messages.len(), 2);
    assert_eq!(import.checksum_sha256.len(), 64);
}

#[test]
fn verify_checksum_matches_and_detects_mutation() {
    let dir = temp_dir();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, PI_V3_JSONL_FIXTURE).unwrap();

    let import = session::import_pi_session(&path).unwrap();
    assert!(session::verify_pi_checksum(&path, &import.checksum_sha256).unwrap());

    // Mutate file
    std::fs::write(&path, format!("{PI_V3_JSONL_FIXTURE}\n")).unwrap();
    assert!(!session::verify_pi_checksum(&path, &import.checksum_sha256).unwrap());
}

#[test]
fn import_as_cow_creates_isolated_native_session() {
    let dir = temp_dir();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, PI_V3_JSONL_FIXTURE).unwrap();

    let import = session::import_pi_session(&path).unwrap();
    let cow = session::import_as_cow(&import);

    assert_ne!(cow.id, import.session_id);
    assert_eq!(
        cow.origin,
        session::SessionOrigin::CopiedFromUpstream {
            source_session_id: "01abc".to_string()
        }
    );
    assert_eq!(cow.messages.len(), 2);

    let saved_path = session::save(&dir, &cow).unwrap();
    assert!(saved_path.exists());

    let loaded = session::load(&dir, &cow.id).unwrap();
    assert_eq!(loaded.origin, cow.origin);
}

#[test]
fn fixture_contract_session_pi_import_checksum() {
    let dir = temp_dir();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, PI_V3_JSONL_FIXTURE).unwrap();

    let import = session::import_pi_session(&path).unwrap();
    let verified = session::verify_pi_checksum(&path, &import.checksum_sha256).unwrap();

    let report = serde_json::json!({
        "checksum_verified": verified,
        "imported": !import.session_id.is_empty(),
        "source": "pi-mono"
    });

    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/upstream-pi/session.pi-import-checksum.expected.json"
    ))
    .unwrap();

    assert_eq!(
        report["checksum_verified"],
        expected["expected"]["checksum_verified"]
    );
    assert_eq!(report["imported"], expected["expected"]["imported"]);
    assert_eq!(report["source"], expected["expected"]["source"]);
}

#[test]
fn fixture_contract_session_pi_cow_provenance() {
    let dir = temp_dir();
    let path = dir.join("session.jsonl");
    std::fs::write(&path, PI_V3_JSONL_FIXTURE).unwrap();

    let import = session::import_pi_session(&path).unwrap();
    let mut cow = session::import_as_cow(&import);

    // Initial state
    let cow_copied = matches!(
        cow.origin,
        session::SessionOrigin::CopiedFromUpstream { .. }
    );

    // Mutate cow messages
    let orig_count = cow.messages.len();
    cow.messages.push(Message::user_text("extra turn"));
    let mutation_isolated =
        import.messages.len() == orig_count && cow.messages.len() == orig_count + 1;

    let provenance_header = match cow.origin {
        session::SessionOrigin::CopiedFromUpstream { .. } => "copied-from-upstream",
        session::SessionOrigin::Native => "native",
    };

    let report = serde_json::json!({
        "cow_copied": cow_copied,
        "mutation_isolated": mutation_isolated,
        "provenance_header": provenance_header
    });

    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/upstream-pi/session.pi-cow-provenance.expected.json"
    ))
    .unwrap();

    assert_eq!(report["cow_copied"], expected["expected"]["cow_copied"]);
    assert_eq!(
        report["mutation_isolated"],
        expected["expected"]["mutation_isolated"]
    );
    assert_eq!(
        report["provenance_header"],
        expected["expected"]["provenance_header"]
    );
}
