use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[tokio::test]
async fn rpc_cli_boundary() -> anyhow::Result<()> {
    let bin_path = env!("CARGO_BIN_EXE_pi-rs");
    let tmp_dir = std::env::temp_dir().join(format!("pi-rs-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let mut child = Command::new(bin_path)
        .args(["--mode", "rpc"])
        .env("PI_MODEL", "gemini-2.0-flash")
        .env("XDG_CONFIG_HOME", &tmp_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stderr = BufReader::new(child.stderr.take().unwrap());

    async fn request(
        stdin: &mut tokio::process::ChildStdin,
        stdout: &mut BufReader<tokio::process::ChildStdout>,
        text: &str,
    ) -> anyhow::Result<serde_json::Value> {
        stdin.write_all(text.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            stdout.read_line(&mut line),
        )
        .await??;
        Ok(serde_json::from_str(&line)?)
    }

    let initial = request(&mut stdin, &mut stdout, r#"{"type":"get_state","id":"1"}"#).await?;
    assert_eq!(initial["success"], true);
    let initial_id = initial["data"]["sessionId"].as_str().unwrap().to_owned();
    assert!(!initial_id.is_empty());

    let malformed = request(&mut stdin, &mut stdout, "invalid json").await?;
    assert_eq!(malformed["command"], "parse");
    assert_eq!(malformed["success"], false);

    let missing = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"prompt","id":"missing"}"#,
    )
    .await?;
    assert_eq!(missing["id"], "missing");
    assert_eq!(missing["command"], "prompt");
    assert_eq!(missing["success"], false);
    let empty = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"prompt","id":"empty","message":""}"#,
    )
    .await?;
    assert_eq!(empty["id"], "empty");
    assert_eq!(empty["success"], false);

    let unknown = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"unknown_foo","id":"2"}"#,
    )
    .await?;
    assert_eq!(unknown["success"], false);

    // Test queue controls fixture replay via CLI
    let steer_all = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"set_steering_mode","id":"qc1","mode":"all"}"#,
    )
    .await?;
    assert_eq!(steer_all["id"], "qc1");
    assert_eq!(steer_all["command"], "set_steering_mode");
    assert_eq!(steer_all["success"], true);

    let followup_all = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"set_follow_up_mode","mode":"all"}"#,
    )
    .await?;
    assert!(followup_all.get("id").is_none());
    assert_eq!(followup_all["command"], "set_follow_up_mode");
    assert_eq!(followup_all["success"], true);

    let qc_state = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"get_state","id":"qc_state"}"#,
    )
    .await?;
    assert_eq!(qc_state["data"]["steeringMode"], "all");
    assert_eq!(qc_state["data"]["followUpMode"], "all");

    let reset = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"new_session","id":"3"}"#,
    )
    .await?;
    assert_eq!(reset["success"], true);
    let state = request(&mut stdin, &mut stdout, r#"{"type":"get_state","id":"4"}"#).await?;
    let replacement_id = state["data"]["sessionId"].as_str().unwrap();
    assert!(!replacement_id.is_empty());
    assert_ne!(replacement_id, initial_id);
    assert_eq!(state["data"]["messageCount"], 0);
    assert_eq!(state["data"]["pendingMessageCount"], 0);

    // Test thinking controls
    let avail = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"get_available_thinking_levels","id":"tc1"}"#,
    )
    .await?;
    assert_eq!(avail["id"], "tc1");
    assert_eq!(avail["success"], true);
    assert_eq!(avail["data"]["levels"], serde_json::json!(["off"]));

    let set_th = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"set_thinking_level","id":"tc2","level":"high"}"#,
    )
    .await?;
    assert_eq!(set_th["id"], "tc2");
    assert_eq!(set_th["success"], true);
    assert!(set_th.get("data").is_none());

    // Non-reasoning model gemini-2.0-flash clamps high to off
    let th_state = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"get_state","id":"tc3"}"#,
    )
    .await?;
    assert_eq!(th_state["data"]["thinkingLevel"], "off");

    let cycle = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"cycle_thinking_level","id":"tc4"}"#,
    )
    .await?;
    assert_eq!(cycle["id"], "tc4");
    assert_eq!(cycle["data"]["level"], serde_json::Value::Null);

    let err_level = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"set_thinking_level","id":"tc5","level":"invalid"}"#,
    )
    .await?;
    assert_eq!(err_level["id"], "tc5");
    assert_eq!(err_level["success"], false);

    let miss_level = request(
        &mut stdin,
        &mut stdout,
        r#"{"type":"set_thinking_level","id":"tc6"}"#,
    )
    .await?;
    assert_eq!(miss_level["id"], "tc6");
    assert_eq!(miss_level["success"], false);
    assert_eq!(miss_level["error"], "level is required");

    drop(stdin);
    let mut stdout_lines = Vec::new();
    let mut line = String::new();
    while stdout.read_line(&mut line).await? > 0 {
        if !line.trim().is_empty() {
            serde_json::from_str::<serde_json::Value>(&line)?;
            stdout_lines.push(line.clone());
        }
        line.clear();
    }
    let mut stderr_text = String::new();
    stderr.read_to_string(&mut stderr_text).await?;
    let status = child.wait().await?;
    assert!(status.success());
    assert!(stderr_text.is_empty(), "RPC stderr: {stderr_text}");
    Ok(())
}

#[tokio::test]
async fn rpc_cli_rejects_invalid_mode() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_pi-rs"))
        .args(["--mode", "invalid"])
        .output()
        .await?;
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("invalid value"));
    Ok(())
}

#[tokio::test]
async fn rpc_cli_flag_conflicts() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_pi-rs"))
        .args(["--mode", "rpc", "--json"])
        .env("PI_MODEL", "gemini-2.0-flash")
        .output()
        .await?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(!output.status.success());
    assert!(stderr.contains("--mode rpc cannot be combined with --prompt"));
    Ok(())
}
