use std::io::Cursor;

#[path = "../src/permission.rs"]
#[allow(dead_code)]
mod permission;

use permission::{CliPermission, Mode};
use pi_agent::{PermissionDecision, PermissionPolicy};

#[tokio::test]
async fn test_cli_permission_eof_and_empty_reader_returns_deny() {
    let empty_cursor = Box::new(Cursor::new(Vec::<u8>::new()));
    let perm = CliPermission::with_reader(Mode::Interactive, empty_cursor);

    let decision = perm
        .check("bash", &serde_json::json!({"command": "echo bad"}))
        .await;

    match decision {
        PermissionDecision::Deny { reason } => {
            assert!(
                reason.contains("EOF") || reason.contains("denied"),
                "unexpected reason: {reason}"
            );
        }
        _ => panic!("expected Deny on EOF stdin, got {decision:?}"),
    }
}

#[tokio::test]
async fn test_cli_permission_yes_reader_returns_allow() {
    let input_cursor = Box::new(Cursor::new(b"yes\n".to_vec()));
    let perm = CliPermission::with_reader(Mode::Interactive, input_cursor);

    let decision = perm
        .check("bash", &serde_json::json!({"command": "echo ok"}))
        .await;

    assert_eq!(decision, PermissionDecision::Allow);
}
