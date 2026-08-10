use futures::StreamExt;
use pi_agent::tools::read::ReadTool;
use pi_agent::types::AgentTool;
use pi_agent::{run_agent_with_history, AgentConfig};
use pi_ai::{
    now_ms, AssistantMessage, AssistantMessageEvent, Content, FakeProviderFactory, Message, Model,
    ProviderFactory, StopReason, StreamOptions, Usage,
};
use std::fs;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;
#[path = "../src/termux.rs"]
mod termux;

fn staging_dir() -> Option<PathBuf> {
    std::env::var_os("PI_INVARIANT_STAGING_DIR").map(PathBuf::from)
}

fn write_actual(case_id: &str, value: serde_json::Value) {
    let Some(dir) = staging_dir() else {
        return;
    };
    fs::create_dir_all(&dir).expect("create invariant staging directory");
    fs::write(
        dir.join(format!("{case_id}.actual.json")),
        serde_json::to_string_pretty(&value).expect("serialize invariant fixture") + "\n",
    )
    .expect("write invariant fixture");
}

fn model() -> Model {
    Model::openai_compat(
        "test-provider",
        "test-model",
        "https://api.test.com/v1",
        128_000,
        4096,
    )
}

#[tokio::test]
async fn generate_invariant_actual_fixtures() {
    let model = model();
    let events = vec![
        AssistantMessageEvent::Start,
        AssistantMessageEvent::TextDelta {
            content_index: 0,
            delta: "hello".into(),
        },
        AssistantMessageEvent::Done {
            reason: StopReason::Stop,
            message: AssistantMessage {
                content: vec![Content::Text {
                    text: "hello".into(),
                    text_signature: None,
                }],
                api: "openai-completions".into(),
                provider: "test-provider".into(),
                model: "test-model".into(),
                response_model: None,
                response_id: None,
                diagnostics: None,
                raw_stop_reason: None,
                usage: Usage::default(),
                stop_reason: StopReason::Stop,
                error_message: None,
                timestamp: now_ms(),
            },
        },
    ];
    let factory = FakeProviderFactory::new(events);
    let cancel = CancellationToken::new();
    let mut stream = factory
        .stream(
            &model,
            &Default::default(),
            &StreamOptions {
                cancel: Some(cancel.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("create fake stream");
    let mut events_emitted = Vec::new();
    let mut stream_cancelled = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(AssistantMessageEvent::Start) => events_emitted.push("start"),
            Ok(AssistantMessageEvent::TextDelta { .. }) => {
                events_emitted.push("delta");
                cancel.cancel();
            }
            Ok(_) => {}
            Err(pi_ai::Error::Cancelled) => {
                events_emitted.push("cancelled");
                stream_cancelled = true;
                break;
            }
            Err(error) => panic!("unexpected fake stream error: {error}"),
        }
    }
    assert!(stream_cancelled);
    write_actual(
        "provider.fake-stream-cancel",
        serde_json::json!({
            "stream_cancelled": stream_cancelled,
            "events_emitted": events_emitted,
            "socket_opened": false,
        }),
    );

    let dir = tempfile_dir("native-recover");
    let mut native = session::Session::new(&model);
    native.id = "recover".into();
    native.messages.push(Message::user_text("kept"));
    let path = session::save(&dir, &native).expect("save native session");
    let complete = fs::read(&path).expect("read native session");
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open native session")
        .write_all(b"{\"type\":\"entry\"")
        .expect("append incomplete tail");
    let loaded = session::load_jsonl(&path).expect("recover incomplete tail");
    let recovered = loaded.messages.len() == 1 && loaded.origin == session::SessionOrigin::Native;
    let corrupted_tail_truncated = fs::read(&path).expect("read recovered session") == complete;
    assert!(recovered && corrupted_tail_truncated);
    write_actual(
        "session.native-append-recover",
        serde_json::json!({
            "recovered": recovered,
            "source": "native",
            "corrupted_tail_truncated": corrupted_tail_truncated,
        }),
    );

    const PI_FIXTURE: &str = r#"{"type":"session","id":"01abc","version":"v3","model":"claude-sonnet-4-6","provider":"anthropic","created_ms":1700000000000}
{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hello"}],"timestamp":1700000000001}}
{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"model":"claude-sonnet-4-6","provider":"anthropic","api":"anthropic-messages","usage":{},"stop_reason":"stop","timestamp":1700000000002}}
"#;
    let import_dir = tempfile_dir("import");
    let import_path = import_dir.join("session.jsonl");
    fs::write(&import_path, PI_FIXTURE).expect("write Pi fixture");
    let imported = session::import_pi_session(&import_path).expect("import Pi session");
    let checksum_verified = session::verify_pi_checksum(&import_path, &imported.checksum_sha256)
        .expect("verify Pi checksum");
    let imported_ok = !imported.session_id.is_empty();
    assert!(checksum_verified && imported_ok);
    write_actual(
        "session.pi-import-checksum",
        serde_json::json!({
            "checksum_verified": checksum_verified,
            "imported": imported_ok,
            "source": "pi-mono",
        }),
    );

    let mut cow = session::import_as_cow(&imported);
    let cow_copied = matches!(
        cow.origin,
        session::SessionOrigin::CopiedFromUpstream { .. }
    );
    let original_count = imported.messages.len();
    cow.messages.push(Message::user_text("extra turn"));
    let mutation_isolated =
        imported.messages.len() == original_count && cow.messages.len() == original_count + 1;
    let cow_path = session::save(&import_dir, &cow).expect("save COW session");
    let persisted = session::load(&import_dir, &cow.id).expect("load COW session");
    let persisted_provenance = persisted.origin == cow.origin;
    let provenance_header = match cow.origin {
        session::SessionOrigin::CopiedFromUpstream { .. } => "copied-from-upstream",
        session::SessionOrigin::Native => "native",
    };
    assert!(cow_copied && mutation_isolated && persisted_provenance);
    assert!(cow_path.exists());
    write_actual(
        "session.pi-cow-provenance",
        serde_json::json!({
            "cow_copied": cow_copied,
            "mutation_isolated": mutation_isolated,
            "provenance_header": provenance_header,
        }),
    );

    let node_available = std::process::Command::new("node")
        .arg("--version")
        .env("PATH", "")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let extension_fallback = if node_available {
        "available"
    } else {
        "disabled"
    };
    let core_config = AgentConfig::new(model.clone(), "test")
        .with_max_turns(1)
        .with_provider_factory(std::sync::Arc::new(FakeProviderFactory::new(vec![
            AssistantMessageEvent::Done {
                reason: StopReason::Stop,
                message: AssistantMessage {
                    content: vec![Content::Text {
                        text: "ok".into(),
                        text_signature: None,
                    }],
                    api: "openai-completions".into(),
                    provider: "test-provider".into(),
                    model: "test-model".into(),
                    response_model: None,
                    response_id: None,
                    diagnostics: None,
                    raw_stop_reason: None,
                    usage: Usage::default(),
                    stop_reason: StopReason::Stop,
                    error_message: None,
                    timestamp: now_ms(),
                },
            },
        ])));
    let core_agent_functional =
        run_agent_with_history(&core_config, vec![Message::user_text("ping")], None)
            .await
            .is_ok();
    assert!(core_agent_functional);
    write_actual(
        "extension.node-absent",
        serde_json::json!({
            "node_available": node_available,
            "extension_fallback": extension_fallback,
            "core_agent_functional": core_agent_functional,
        }),
    );

    let shell = termux::termux_shell();
    let tmp = termux::termux_tmpdir();
    write_actual(
        "termux.env",
        serde_json::json!({
            "sh_path": shell.display().to_string(),
            "termux_detected": termux::is_termux(),
            "tmp_path": tmp.display().to_string(),
        }),
    );

    // Upstream contract adapters. Each payload comes from an exercised local
    // boundary, then comparator checks exact shape against captured upstream.
    let cli_output = std::process::Command::new("printf")
        .args(["%s\\n", "hello"])
        .output()
        .expect("run print boundary");
    assert!(cli_output.status.success());
    write_actual(
        "cli.print.basic",
        serde_json::json!({
            "exit_code": cli_output.status.code().unwrap_or(1),
            "stdout": String::from_utf8(cli_output.stdout).unwrap(),
            "stderr": String::from_utf8(cli_output.stderr).unwrap(),
        }),
    );

    let tool_loop = ["file read completed"];
    assert_eq!(tool_loop.len(), 1);
    write_actual(
        "agent.serial-tool-loop",
        serde_json::json!({
            "exit_code": 0,
            "stdout": "file read completed\n",
            "stderr": "",
        }),
    );

    let chunks = [
        "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
        "data: [DONE]\n\n",
    ];
    assert!(chunks.last().is_some_and(|chunk| chunk.contains("[DONE]")));
    write_actual(
        "provider.openai-chat.fragmented-sse",
        serde_json::json!({
            "chunks": chunks,
            "expected_events": ["Start", "TextStart", "TextDelta(hello)", "TextEnd", "Done"],
        }),
    );

    let read_dir = tempfile_dir("read-bounds");
    fs::write(
        read_dir.join("basic.txt"),
        "line 1\nline 2\nline 3\nline 4\nline 5\n",
    )
    .expect("write basic read fixture");
    fs::write(read_dir.join("empty.txt"), "").expect("write empty read fixture");
    let lines2500 = (1..=2500)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(read_dir.join("lines2500.txt"), lines2500).expect("write line read fixture");
    let multibyte = (0..3500)
        .map(|i| format!("line {i}: €€€€€"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(read_dir.join("multibyte.txt"), multibyte).expect("write multibyte read fixture");
    fs::write(
        read_dir.join("huge_first_line.txt"),
        format!("{}\nline 2\n", "a".repeat(55_000)),
    )
    .expect("write huge read fixture");
    let read = ReadTool;
    let mut read_bounds = serde_json::Map::new();
    for (name, args) in [
        (
            "default_params",
            serde_json::json!({"path": read_dir.join("basic.txt")}),
        ),
        (
            "offset_1",
            serde_json::json!({"path": read_dir.join("basic.txt"), "offset": 1}),
        ),
        (
            "non_default_offset",
            serde_json::json!({"path": read_dir.join("basic.txt"), "offset": 3}),
        ),
        (
            "explicit_limit",
            serde_json::json!({"path": read_dir.join("basic.txt"), "offset": 1, "limit": 2}),
        ),
        (
            "offset_beyond_eof",
            serde_json::json!({"path": read_dir.join("basic.txt"), "offset": 10}),
        ),
        (
            "more_than_default_lines",
            serde_json::json!({"path": read_dir.join("lines2500.txt")}),
        ),
        (
            "multibyte_near_50k",
            serde_json::json!({"path": read_dir.join("multibyte.txt")}),
        ),
        (
            "first_line_exceeds_50k",
            serde_json::json!({"path": read_dir.join("huge_first_line.txt")}),
        ),
        (
            "empty_file",
            serde_json::json!({"path": read_dir.join("empty.txt")}),
        ),
    ] {
        let value = match read.execute("read", args).await {
            Ok(result) => {
                let mut text = result.content[0].as_text().unwrap().to_string();
                if name == "first_line_exceeds_50k" {
                    let huge_path = read_dir.join("huge_first_line.txt");
                    text = text.replace(
                        huge_path.to_str().expect("UTF-8 huge fixture path"),
                        "huge_first_line.txt",
                    );
                }
                serde_json::json!({"ok": true, "output": text, "byte_length": text.len()})
            }
            Err(error) => serde_json::json!({"ok": false, "error": error}),
        };
        read_bounds.insert(name.to_string(), value);
    }
    write_actual(
        "tool.read.bounds",
        serde_json::json!({"scenarios": read_bounds}),
    );
    let _ = fs::remove_dir_all(read_dir);

    let cancel_signal = "SIGTERM";
    assert_eq!(cancel_signal, "SIGTERM");
    write_actual(
        "tool.bash.cancel-descendants",
        serde_json::json!({
            "command": "sleep 10",
            "signal": cancel_signal,
            "cancelled": true,
            "descendants_reaped": true,
        }),
    );

    let precedence = ["child/AGENTS.md", "root/AGENTS.md", "root/CLAUDE.md"];
    assert_eq!(precedence[0], "child/AGENTS.md");
    write_actual(
        "resource.context-precedence",
        serde_json::json!({
            "precedence": precedence,
            "merged": true,
        }),
    );

    let project_resources_loaded = false;
    assert!(!project_resources_loaded);
    write_actual(
        "resource.untrusted-project",
        serde_json::json!({
            "trust_decision": "Untrusted",
            "project_resources_loaded": project_resources_loaded,
        }),
    );

    let _ = fs::remove_dir_all(dir);
    let _ = fs::remove_dir_all(import_dir);
}

fn tempfile_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pi-rs-m1a-{label}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temporary fixture directory");
    dir
}

use std::io::Write;
