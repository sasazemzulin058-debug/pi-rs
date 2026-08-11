use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn run(input: &str) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pi-rs"))
        .args(["--mode", "acp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let mut output = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    child.wait().unwrap();
    serde_json::from_str(output.trim()).unwrap()
}

#[test]
fn parse_error_uses_parse_code_and_null_id() {
    let response = run("not json\n");
    assert_eq!(response["id"], serde_json::Value::Null);
    assert_eq!(response["error"]["code"], -32700);
}

#[test]
fn invalid_request_and_null_id_use_invalid_request_code() {
    let response =
        run("{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"initialize\",\"params\":{}}\n");
    assert_eq!(response["id"], serde_json::Value::Null);
    assert_eq!(response["error"]["code"], -32600);
}

#[test]
fn numeric_ids_must_be_integers() {
    let response =
        run("{\"jsonrpc\":\"2.0\",\"id\":1.5,\"method\":\"initialize\",\"params\":{}}\n");
    assert_eq!(response["error"]["code"], -32600);
}
