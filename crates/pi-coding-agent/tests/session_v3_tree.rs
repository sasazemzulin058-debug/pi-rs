#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::Message;
use session::{load_tree_jsonl, save_tree_jsonl, Session, SessionEntry, SessionTree};

fn message(text: &str) -> SessionEntry {
    SessionEntry::Message {
        message: session::AgentMessageValue(
            serde_json::to_value(Message::user_text(text)).unwrap(),
        ),
    }
}

#[test]
fn branch_switch_and_context() {
    let mut tree = SessionTree::new();
    let root = tree.append(None, message("root")).unwrap();
    let first = tree.append(Some(&root), message("first")).unwrap();
    let old_leaf = tree.append(Some(&first), message("old")).unwrap();
    let new_leaf = tree.branch_at(&root, message("new")).unwrap();
    assert_eq!(tree.effective_messages(&old_leaf).unwrap().len(), 3);
    assert_eq!(tree.effective_messages(&new_leaf).unwrap().len(), 2);
    tree.set_active_leaf(&old_leaf).unwrap();
    assert_eq!(tree.active_leaf(), Some(old_leaf.as_str()));
}

#[test]
fn typed_unknown_and_roundtrip() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-tree-{}", std::process::id()));
    let path = dir.join("tree.jsonl");
    let mut tree = SessionTree::new();
    let root = tree.append(None, message("root")).unwrap();
    let model = tree
        .append(
            Some(&root),
            SessionEntry::ModelChange {
                model_id: "m2".into(),
                provider: "test".into(),
            },
        )
        .unwrap();
    let leaf = tree
        .append(
            Some(&model),
            SessionEntry::Unknown {
                raw: serde_json::json!({"type":"future","x":1}),
            },
        )
        .unwrap();
    assert_eq!(tree.effective_context(&leaf).unwrap().len(), 3);
    assert_eq!(tree.effective_messages(&leaf).unwrap().len(), 1);
    save_tree_jsonl(&path, &tree).unwrap();
    let loaded = load_tree_jsonl(&path).unwrap();
    assert_eq!(loaded.active_leaf(), Some(leaf.as_str()));
    assert_eq!(loaded.effective_messages(&leaf).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn linear_session_bridge_preserves_active_branch_messages() {
    let model = pi_ai::Model::openai_compat("test", "model", "https://example.invalid", 1024, 128);
    let mut session = Session::new(&model);
    session.messages.push(Message::user_text("root"));
    session.messages.push(Message::user_text("leaf"));
    let tree = SessionTree::from_session(&session).unwrap();
    let mut restored = Session::new(&model);
    tree.to_session(&mut restored).unwrap();
    assert_eq!(restored.messages.len(), session.messages.len());
    assert_eq!(
        serde_json::to_value(&restored.messages).unwrap(),
        serde_json::to_value(&session.messages).unwrap()
    );
}

#[test]
fn imported_unknown_records_are_preserved_without_messages() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-import-{}", std::process::id()));
    let path = dir.join("upstream.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"upstream","model":"m","provider":"p"}"#, "\n",
            r#"{"type":"future_entry","payload":{"x":1}}"#, "\n",
            r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#, "\n",
        ),
    )
    .unwrap();
    let tree = session::import_pi_session_as_tree(&path).unwrap();
    let leaf = tree.active_leaf().unwrap();
    assert_eq!(tree.nodes().count(), 2);
    assert_eq!(tree.effective_messages(leaf).unwrap().len(), 1);
    assert!(matches!(
        tree.nodes().next().unwrap().entry,
        SessionEntry::Unknown { .. }
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_compaction_retained_tail_checkpoint_resolution() {
    let mut tree = SessionTree::new();
    let root = tree.append(None, message("pre-compaction-1")).unwrap();
    let m2 = tree
        .append(Some(&root), message("pre-compaction-2"))
        .unwrap();
    let tail_val = session::AgentMessageValue(serde_json::json!({
        "role": "user",
        "content": "tail-msg",
        "timestamp": 1700000000000i64
    }));
    let comp = tree
        .append(
            Some(&m2),
            SessionEntry::Compaction {
                summary: "compacted old context".into(),
                first_kept_entry_id: None,
                tokens_before: Some(500),
                retained_tail: Some(vec![tail_val.clone()]),
                details: None,
                usage: None,
                from_hook: None,
            },
        )
        .unwrap();
    let post = tree
        .append(Some(&comp), message("post-compaction"))
        .unwrap();

    let eff_msgs = tree.effective_messages(&post).unwrap();
    assert_eq!(eff_msgs.len(), 3);
    let expected_summary = format!(
        "{}compacted old context{}",
        session::COMPACTION_SUMMARY_PREFIX,
        session::COMPACTION_SUMMARY_SUFFIX
    );
    assert!(
        matches!(&eff_msgs[0], Message::User { content, .. } if matches!(&content[0], pi_ai::Content::Text { text, .. } if text == &expected_summary))
    );
    assert!(
        matches!(&eff_msgs[1], Message::User { content, .. } if matches!(&content[0], pi_ai::Content::Text { text, .. } if text == "tail-msg"))
    );
    assert!(
        matches!(&eff_msgs[2], Message::User { content, .. } if matches!(&content[0], pi_ai::Content::Text { text, .. } if text == "post-compaction"))
    );
}

#[test]
fn imported_upstream_parent_entry_id_source_id_and_timestamps_preserved() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-upstream-{}", std::process::id()));
    let path = dir.join("upstream.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"upstream-s1"}"#, "\n",
            r#"{"type":"message","id":"node-1","timestamp":1700000000000,"message":{"role":"user","content":[{"type":"text","text":"root"}]}}"#, "\n",
            r#"{"type":"message","id":"node-2","parentEntryId":"node-1","timestamp":1700000001000,"message":{"role":"user","content":[{"type":"text","text":"reply"}]}}"#, "\n",
        ),
    )
    .unwrap();
    let tree = session::import_pi_session_as_tree(&path).unwrap();
    let nodes: Vec<_> = tree.nodes().collect();
    assert_eq!(nodes.len(), 2);

    assert_eq!(nodes[0].entry_id, "node-1");
    assert_eq!(nodes[0].parent_id, None);
    assert_eq!(nodes[0].source_entry_id.as_deref(), Some("node-1"));
    assert_eq!(nodes[0].timestamp_ms, Some(1700000000000));

    assert_eq!(nodes[1].entry_id, "node-2");
    assert_eq!(nodes[1].parent_id.as_deref(), Some("node-1"));
    assert_eq!(nodes[1].source_entry_id.as_deref(), Some("node-2"));
    assert_eq!(nodes[1].timestamp_ms, Some(1700000001000));

    // Test tree serialization roundtrip preserves source IDs & timestamps
    let json_tree_path = dir.join("tree.jsonl");
    save_tree_jsonl(&json_tree_path, &tree).unwrap();
    let loaded = load_tree_jsonl(&json_tree_path).unwrap();
    let loaded_nodes: Vec<_> = loaded.nodes().collect();

    assert_eq!(loaded_nodes[0].source_entry_id.as_deref(), Some("node-1"));
    assert_eq!(loaded_nodes[0].timestamp_ms, Some(1700000000000));
    assert_eq!(loaded_nodes[1].parent_id.as_deref(), Some("node-1"));
    assert_eq!(loaded_nodes[1].source_entry_id.as_deref(), Some("node-2"));
    assert_eq!(loaded_nodes[1].timestamp_ms, Some(1700000001000));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn invalid_parent_and_leaf_fail_closed() {
    let mut tree = SessionTree::new();
    assert!(tree.append(Some("missing"), message("x")).is_err());
    let root = tree.append(None, message("root")).unwrap();
    assert!(tree.append(None, message("second")).is_err());
    assert!(tree.set_active_leaf("missing").is_err());
    assert!(tree.effective_context(&root).is_ok());
}

#[test]
fn compaction_context_resolution_filters_summarized_entries() {
    let mut tree = SessionTree::new();
    let e0 = tree.append(None, message("msg0")).unwrap();
    let e1 = tree.append(Some(&e0), message("msg1")).unwrap();
    let e2 = tree.append(Some(&e1), message("msg2")).unwrap();
    let e3 = tree
        .append(
            Some(&e2),
            SessionEntry::Compaction {
                summary: "summary of e0 and e1".into(),
                first_kept_entry_id: Some(e2.clone()),
                tokens_before: Some(1000),
                retained_tail: None,
                details: None,
                usage: None,
                from_hook: None,
            },
        )
        .unwrap();
    let e4 = tree.append(Some(&e3), message("msg3")).unwrap();

    let ctx = tree.effective_context(&e4).unwrap();
    assert_eq!(ctx.len(), 3);
    assert!(matches!(ctx[0], SessionEntry::Compaction { .. }));
    assert!(matches!(ctx[1], SessionEntry::Message { .. }));
    assert!(matches!(ctx[2], SessionEntry::Message { .. }));

    let msgs = tree.effective_messages(&e4).unwrap();
    assert_eq!(msgs.len(), 3);

    let get_text = |msg: &Message| match msg {
        Message::User { content, .. } => match &content[0] {
            pi_ai::Content::Text { text, .. } => text.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    };

    assert!(get_text(&msgs[0]).contains("summary of e0 and e1"));
    assert_eq!(get_text(&msgs[1]), "msg2");
    assert_eq!(get_text(&msgs[2]), "msg3");
}

#[test]
fn active_leaf_import_fail_closed_on_invalid_id() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-invalid-leaf-{}", std::process::id()));
    let path = dir.join("invalid_leaf.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"s1","activeLeaf":"nonexistent-node"}"#, "\n",
            r#"{"type":"message","id":"e0","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#, "\n",
            r#"{"type":"session_info","name":"info"}"#, "\n",
        ),
    )
    .unwrap();
    assert!(session::import_pi_session_as_tree(&path).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cow_from_imported_preserves_tree_structure_and_provenance() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-cow-{}", std::process::id()));
    let path = dir.join("imported.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","version":3,"id":"src-123","model":"m1","provider":"p1"}"#, "\n",
            r#"{"type":"message","id":"e0","message":{"role":"user","content":[{"type":"text","text":"msg0"}]}}"#, "\n",
            r#"{"type":"message","id":"e1","parentId":"e0","message":{"role":"user","content":[{"type":"text","text":"msg1"}]}}"#, "\n",
        ),
    )
    .unwrap();

    let imported = session::import_pi_session_as_tree(&path).unwrap();
    let cow = SessionTree::cow_from_imported(&imported);

    assert_ne!(cow.metadata.session_id, imported.metadata.session_id);
    assert_eq!(cow.metadata.source_session_id.as_deref(), Some("src-123"));
    assert_eq!(cow.active_leaf(), imported.active_leaf());
    assert_eq!(cow.nodes().count(), imported.nodes().count());

    let imported_nodes: Vec<_> = imported.nodes().collect();
    let cow_nodes: Vec<_> = cow.nodes().collect();
    assert_eq!(imported_nodes.len(), cow_nodes.len());
    for (orig, copy) in imported_nodes.iter().zip(cow_nodes.iter()) {
        assert_eq!(orig.entry_id, copy.entry_id);
        assert_eq!(orig.parent_id, copy.parent_id);
        assert_eq!(orig.source_entry_id, copy.source_entry_id);
        assert_eq!(orig.timestamp_ms, copy.timestamp_ms);
        assert_eq!(orig.timestamp_str, copy.timestamp_str);
    }

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn timestamp_parsing_safe_on_out_of_bounds_or_malformed() {
    assert_eq!(session::parse_iso_timestamp("short"), None);
    assert_eq!(session::parse_iso_timestamp("2026-08-10T12:00"), None);
    assert_eq!(session::parse_iso_timestamp("XXXX-08-10T12:00:00Z"), None);
    assert!(session::parse_iso_timestamp("2026-08-10T12:00:00Z").is_some());
}

#[test]
fn custom_message_wire_schema_supports_content_details_display_and_message_projection() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-cm-{}", std::process::id()));
    let path = dir.join("custom_msg.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"cm-session"}"#, "\n",
            r#"{"type":"custom_message","id":"cm1","customType":"test-ext","content":"injected context","details":{"foo":"bar"},"display":true}"#, "\n",
        ),
    )
    .unwrap();

    let tree = session::import_pi_session_as_tree(&path).unwrap();
    let leaf = tree.active_leaf().unwrap();
    let msgs = tree.effective_messages(leaf).unwrap();
    assert_eq!(msgs.len(), 1);

    let saved_path = dir.join("saved_cm.jsonl");
    save_tree_jsonl(&saved_path, &tree).unwrap();
    let raw = std::fs::read_to_string(&saved_path).unwrap();
    assert!(raw.contains(r#""customType":"test-ext""#));
    assert!(raw.contains(r#""content":"injected context""#));
    assert!(raw.contains(r#""details":{"foo":"bar"}"#));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn label_clear_record_preservation_and_non_leaf_save_rejection() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-label-{}", std::process::id()));
    let path = dir.join("label_clear.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"lbl-session"}"#, "\n",
            r#"{"type":"message","id":"m1","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#, "\n",
            r#"{"type":"label","id":"l1","parentId":"m1","targetId":"m1","label":"checkpoint"}"#, "\n",
            r#"{"type":"label","id":"l2","parentId":"l1","targetId":"m1","label":null}"#, "\n",
            r#"{"type":"message","id":"m2","parentId":"l2","message":{"role":"user","content":[{"type":"text","text":"world"}]}}"#, "\n",
        ),
    )
    .unwrap();

    let mut tree = session::import_pi_session_as_tree(&path).unwrap();
    let nodes: Vec<_> = tree.nodes().collect();
    assert_eq!(nodes.len(), 4);
    assert!(matches!(
        &nodes[2].entry,
        SessionEntry::Label { label: None, ref target_id } if target_id == "m1"
    ));

    // Test non-leaf active save rejection
    tree.set_active_leaf("m1").unwrap();
    let active_path = dir.join("active_branch.jsonl");
    assert!(tree.save_active_branch_jsonl(&active_path).is_err());

    // Switch back to leaf and verify active branch save emits correct ancestor path
    tree.set_active_leaf("m2").unwrap();
    tree.save_active_branch_jsonl(&active_path).unwrap();
    let active_tree = load_tree_jsonl(&active_path).unwrap();
    assert_eq!(active_tree.nodes().count(), 4);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cross_parser_interoperability_with_upstream_node_parser() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-cross-{}", std::process::id()));
    let path = dir.join("pi_rs_v3_session.jsonl");
    std::fs::create_dir_all(&dir).unwrap();

    let mut tree = SessionTree::new();
    tree.metadata.session_id = Some("cross-parser-test-123".into());
    tree.metadata.model = Some("claude-3-5-sonnet".into());
    tree.metadata.provider = Some("anthropic".into());
    tree.metadata.cwd = Some("/data/data/com.termux/files/home".into());

    let e0 = tree.append(None, message("user hello")).unwrap();
    let e1 = tree
        .append(
            Some(&e0),
            SessionEntry::ModelChange {
                model_id: "gpt-4o".into(),
                provider: "openai".into(),
            },
        )
        .unwrap();
    let _e2 = tree.append(Some(&e1), message("assistant reply")).unwrap();

    save_tree_jsonl(&path, &tree).unwrap();
    let loaded = load_tree_jsonl(&path).unwrap();
    assert_eq!(
        loaded.metadata.session_id.as_deref(),
        Some("cross-parser-test-123")
    );
    assert_eq!(loaded.metadata.model.as_deref(), Some("claude-3-5-sonnet"));

    // Invoke upstream node parser against pi-rs generated file
    let node_script = format!(
        concat!(
            r#"import {{ buildSessionContext }} from "./packages/coding-agent/src/core/session-manager.ts";"#,
            r#"import fs from "fs";"#,
            r#"const text = fs.readFileSync("{}", "utf8");"#,
            r#"const lines = text.trim().split("\n").map(l => JSON.parse(l));"#,
            r#"if (lines[0].type !== "session" || lines[0].version !== 3) process.exit(1);"#,
            r#"const ctx = buildSessionContext(lines.slice(1));"#,
            r#"if (ctx.messages.length !== 2) process.exit(2);"#,
            r#"console.log("OK:" + ctx.messages.length);"#,
        ),
        path.display()
    );

    let output = std::process::Command::new("node")
        .args(["--import", "tsx/esm", "-e", &node_script])
        .current_dir("/data/data/com.termux/files/home/pi-mono")
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("OK:2"),
            "Upstream Node parser failed: status={:?}, stdout={}, stderr={}",
            out.status,
            stdout,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_retained_tail_upstream_shaped_jsonl_and_lossless_roundtrip() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-retained-{}", std::process::id()));
    let path = dir.join("retained_upstream.jsonl");
    std::fs::create_dir_all(&dir).unwrap();

    let jsonl_content = concat!(
        r#"{"type":"session","version":3,"id":"s-retained"}"#,
        "\n",
        r#"{"type":"message","id":"e0","timestamp":"2023-11-15T00:00:00.000Z","message":{"role":"user","content":[{"type":"text","text":"pre-compaction"}]}}"#,
        "\n",
        r#"{"type":"compaction","id":"e1","parentId":"e0","timestamp":"2023-11-15T00:00:01.000Z","summary":"compacted context","tokensBefore":1000,"retainedTail":[{"role":"user","content":"string user content","timestamp":1700000002000},{"role":"bashExecution","command":"ls -la","output":"file1\nfile2","exitCode":0,"cancelled":false,"truncated":false,"timestamp":1700000003000},{"role":"bashExecution","command":"secret","output":"hidden","excludeFromContext":true,"timestamp":1700000004000},{"role":"custom","customType":"my-type","content":"custom payload","display":true,"timestamp":1700000005000},{"role":"branchSummary","summary":"branch summary text","fromId":"b1","timestamp":1700000006000},{"role":"compactionSummary","summary":"inner compaction summary","tokensBefore":500,"timestamp":1700000007000},{"role":"assistant","content":[{"type":"text","text":"hello"}],"api":"openai","provider":"openai","model":"gpt-4","stopReason":"stop","timestamp":1700000008500},{"role":"toolResult","toolCallId":"tc1","toolName":"bash","content":[{"type":"text","text":"ok"}],"isError":false,"timestamp":1700000008600},{"role":"futureRole","someField":"value","timestamp":1700000008000}]}"#,
        "\n",
        r#"{"type":"message","id":"e2","parentId":"e1","timestamp":"2023-11-15T00:00:09.000Z","message":{"role":"user","content":[{"type":"text","text":"post-compaction"}]}}"#,
        "\n"
    );
    std::fs::write(&path, jsonl_content).unwrap();

    let tree = session::import_pi_session_as_tree(&path).unwrap();
    let leaf = tree.active_leaf().unwrap();

    // Verify ISO entry timestamp parsed to timestamp_ms correctly (2023-11-15T00:00:01.000Z = 1700006401000)
    let comp_node = tree.nodes().nth(1).unwrap();
    assert_eq!(comp_node.timestamp_ms, Some(1700006401000));

    let agent_msgs = tree.effective_agent_messages(leaf).unwrap();
    assert_eq!(agent_msgs.len(), 11);
    assert_eq!(agent_msgs[0].0["role"], "compactionSummary");
    assert_eq!(agent_msgs[0].0["summary"], "compacted context");
    assert_eq!(agent_msgs[1].0["role"], "user");
    assert_eq!(agent_msgs[1].0["content"], "string user content");
    assert_eq!(agent_msgs[7].0["role"], "assistant");
    assert_eq!(agent_msgs[8].0["role"], "toolResult");
    assert_eq!(agent_msgs[9].0["role"], "futureRole");
    assert_eq!(agent_msgs[9].0["someField"], "value");

    let llm_msgs = tree.effective_messages(leaf).unwrap();
    assert_eq!(llm_msgs.len(), 9);

    let saved_path = dir.join("saved_retained.jsonl");
    save_tree_jsonl(&saved_path, &tree).unwrap();
    let reloaded = session::import_pi_session_as_tree(&saved_path).unwrap();
    let reloaded_agent_msgs = reloaded
        .effective_agent_messages(reloaded.active_leaf().unwrap())
        .unwrap();
    assert_eq!(agent_msgs.len(), reloaded_agent_msgs.len());
    for (orig, reload) in agent_msgs.iter().zip(reloaded_agent_msgs.iter()) {
        assert_eq!(orig.0["role"], reload.0["role"]);
        if orig.0["role"] == "user" {
            assert_eq!(orig.0["content"], reload.0["content"]);
        }
    }

    let raw_saved = std::fs::read_to_string(&saved_path).unwrap();
    assert!(raw_saved.contains(r#""retainedTail":["#));
    assert!(raw_saved.contains(r#""futureRole""#));
    assert!(raw_saved.contains(r#""someField":"value""#));

    // Full Value equality test for retainedTail round-trip
    let saved_lines: Vec<&str> = raw_saved.lines().collect();
    let comp_line: serde_json::Value = serde_json::from_str(saved_lines[2]).unwrap();
    let original_comp_line: serde_json::Value =
        serde_json::from_str(jsonl_content.lines().nth(2).unwrap()).unwrap();
    assert_eq!(
        comp_line["retainedTail"],
        original_comp_line["retainedTail"]
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_compaction_without_first_kept_excludes_pre_compaction() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-nofk-{}", std::process::id()));
    let path = dir.join("nofk.jsonl");
    std::fs::create_dir_all(&dir).unwrap();

    let jsonl_content = concat!(
        r#"{"type":"session","version":3,"id":"s-nofk"}"#,
        "\n",
        r#"{"type":"message","id":"e0","timestamp":1700000000000,"message":{"role":"user","content":[{"type":"text","text":"pre-compaction message"}]}}"#,
        "\n",
        r#"{"type":"compaction","id":"e1","parentId":"e0","timestamp":1700000001000,"summary":"compacted context summary"}"#,
        "\n",
        r#"{"type":"message","id":"e2","parentId":"e1","timestamp":1700000002000,"message":{"role":"user","content":[{"type":"text","text":"post-compaction message"}]}}"#,
        "\n"
    );
    std::fs::write(&path, jsonl_content).unwrap();

    let tree = session::import_pi_session_as_tree(&path).unwrap();
    let leaf = tree.active_leaf().unwrap();

    let ctx = tree.effective_context(leaf).unwrap();
    assert_eq!(ctx.len(), 2);
    assert!(matches!(ctx[0], SessionEntry::Compaction { .. }));
    assert!(matches!(ctx[1], SessionEntry::Message { .. }));

    let msgs = tree.effective_messages(leaf).unwrap();
    assert_eq!(msgs.len(), 2);
    let get_text = |msg: &Message| match msg {
        Message::User { content, .. } => match &content[0] {
            pi_ai::Content::Text { text, .. } => text.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    };
    assert!(get_text(&msgs[0]).contains("compacted context summary"));
    assert_eq!(get_text(&msgs[1]), "post-compaction message");

    let agent_msgs = tree.effective_agent_messages(leaf).unwrap();
    assert_eq!(agent_msgs.len(), 2);
    assert_eq!(agent_msgs[0].0["role"], "compactionSummary");
    assert_eq!(agent_msgs[1].0["role"], "user");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_retained_tail_negative_parsing() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-neg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let invalid_object = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"compaction","id":"e0","summary":"comp","retainedTail":{}}"#,
        "\n"
    );
    let p1 = dir.join("obj.jsonl");
    std::fs::write(&p1, invalid_object).unwrap();
    assert!(session::import_pi_session_as_tree(&p1).is_err());

    let invalid_string = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"compaction","id":"e0","summary":"comp","retainedTail":"bad"}"#,
        "\n"
    );
    let p2 = dir.join("str.jsonl");
    std::fs::write(&p2, invalid_string).unwrap();
    assert!(session::import_pi_session_as_tree(&p2).is_err());

    let invalid_array_element = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"compaction","id":"e0","summary":"comp","retainedTail":[123]}"#,
        "\n"
    );
    let p3 = dir.join("elem.jsonl");
    std::fs::write(&p3, invalid_array_element).unwrap();
    assert!(session::import_pi_session_as_tree(&p3).is_err());

    let null_tail = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"compaction","id":"e0","summary":"comp","retainedTail":null}"#,
        "\n"
    );
    let p4 = dir.join("null.jsonl");
    std::fs::write(&p4, null_tail).unwrap();
    assert!(session::import_pi_session_as_tree(&p4).is_err());

    let null_json = serde_json::json!({"type":"compaction","summary":"s","retainedTail":null});
    assert!(serde_json::from_value::<SessionEntry>(null_json).is_err());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_ordinary_message_negative_parsing() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-msg-neg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let null_msg = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"message","id":"e0","message":null}"#,
        "\n"
    );
    let p1 = dir.join("null_msg.jsonl");
    std::fs::write(&p1, null_msg).unwrap();
    let err1 = session::import_pi_session_as_tree(&p1).unwrap_err();
    assert!(err1.to_string().contains("line 2"));

    let string_msg = concat!(
        r#"{"type":"session","id":"s1"}"#,
        "\n",
        r#"{"type":"message","id":"e0","message":"bad"}"#,
        "\n"
    );
    let p2 = dir.join("string_msg.jsonl");
    std::fs::write(&p2, string_msg).unwrap();
    let err2 = session::import_pi_session_as_tree(&p2).unwrap_err();
    assert!(err2.to_string().contains("line 2"));

    let null_serde = serde_json::json!({"type":"message","message":null});
    assert!(serde_json::from_value::<SessionEntry>(null_serde).is_err());

    let bad_serde = serde_json::json!({"type":"message","message":"bad"});
    assert!(serde_json::from_value::<SessionEntry>(bad_serde).is_err());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_ordinary_message_full_value_roundtrip_and_projection() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-msg-rt-{}", std::process::id()));
    let in_path = dir.join("input.jsonl");
    let out_path = dir.join("output.jsonl");
    std::fs::create_dir_all(&dir).unwrap();

    let user_msg = serde_json::json!({
        "role": "user",
        "content": "raw string content preserved",
        "timestamp": 1700000000000i64
    });

    let assistant_msg = serde_json::json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "hello", "textSignature": "sig1"},
            {"type": "thinking", "thinking": "hm", "thinkingSignature": "sig2", "redacted": true},
            {"type": "toolCall", "id": "tc1", "name": "read", "arguments": {"path": "/foo"}, "thoughtSignature": "sig3"}
        ],
        "api": "openai-chat",
        "provider": "openai",
        "model": "gpt-4o",
        "responseModel": "gpt-4o-2024-08-06",
        "responseId": "resp_123",
        "diagnostics": {"rawHeader": "val"},
        "rawStopReason": "end_turn",
        "usage": {
            "input": 10,
            "output": 20,
            "cacheRead": 5,
            "cacheWrite": 2,
            "totalTokens": 37,
            "cacheWrite1h": 1,
            "reasoning": 15,
            "cost": {
                "input": 0.01,
                "output": 0.02,
                "cacheRead": 0.001,
                "cacheWrite": 0.0005,
                "total": 0.0315
            }
        },
        "stopReason": "pending",
        "errorMessage": "none_error",
        "timestamp": 1700000001000i64
    });

    let tool_result_msg = serde_json::json!({
        "role": "toolResult",
        "toolCallId": "tc1",
        "toolName": "read",
        "content": [
            {"type": "image", "data": "base64data", "mimeType": "image/png"}
        ],
        "isError": false,
        "details": {"exitCode": 0},
        "usage": {
            "input": 1,
            "output": 2,
            "cacheRead": 0,
            "cacheWrite": 0,
            "totalTokens": 3,
            "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0, "total": 0.0}
        },
        "addedToolNames": ["bash"],
        "timestamp": 1700000002000i64
    });

    let jsonl = format!(
        "{}\n{}\n{}\n{}\n",
        r#"{"type":"session","id":"s_rt"}"#,
        serde_json::json!({"type":"message","id":"m1","message": user_msg}),
        serde_json::json!({"type":"message","id":"m2","parentHeaderId":"m1","message": assistant_msg}),
        serde_json::json!({"type":"message","id":"m3","parentHeaderId":"m2","message": tool_result_msg}),
    );

    std::fs::write(&in_path, &jsonl).unwrap();

    let tree = session::import_pi_session_as_tree(&in_path).unwrap();
    save_tree_jsonl(&out_path, &tree).unwrap();

    let reloaded_tree = session::import_pi_session_as_tree(&out_path).unwrap();

    let eff_agent_msgs = reloaded_tree.effective_agent_messages("m3").unwrap();
    assert_eq!(eff_agent_msgs.len(), 3);
    assert_eq!(
        eff_agent_msgs[0].0["content"],
        "raw string content preserved"
    );
    assert_eq!(eff_agent_msgs[1].0, assistant_msg);
    assert_eq!(eff_agent_msgs[2].0, tool_result_msg);

    let eff_msgs = reloaded_tree.effective_messages("m3").unwrap();
    assert_eq!(eff_msgs.len(), 3);

    // Verify LLM Projection
    if let Message::User { content, .. } = &eff_msgs[0] {
        assert_eq!(content[0].as_text(), Some("raw string content preserved"));
    } else {
        panic!("Expected user message");
    }

    if let Message::Assistant(a) = &eff_msgs[1] {
        assert_eq!(a.stop_reason, pi_ai::StopReason::Pending);
        assert_eq!(a.response_model.as_deref(), Some("gpt-4o-2024-08-06"));
        assert_eq!(a.response_id.as_deref(), Some("resp_123"));
        assert_eq!(a.raw_stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(a.diagnostics.as_ref().unwrap()["rawHeader"], "val");
        assert_eq!(a.usage.cache_read, 5);
        assert_eq!(a.usage.cache_write, 2);
        assert_eq!(a.usage.total_tokens, 37);
        assert_eq!(a.usage.cache_write_1h, Some(1));
        assert_eq!(a.usage.reasoning, Some(15));

        if let pi_ai::Content::Text { text_signature, .. } = &a.content[0] {
            assert_eq!(text_signature.as_deref(), Some("sig1"));
        } else {
            panic!("expected text content");
        }

        if let pi_ai::Content::Thinking {
            thinking_signature,
            redacted,
            ..
        } = &a.content[1]
        {
            assert_eq!(thinking_signature.as_deref(), Some("sig2"));
            assert_eq!(*redacted, Some(true));
        } else {
            panic!("expected thinking content");
        }

        if let pi_ai::Content::ToolCall {
            thought_signature, ..
        } = &a.content[2]
        {
            assert_eq!(thought_signature.as_deref(), Some("sig3"));
        } else {
            panic!("expected toolCall content");
        }
    } else {
        panic!("Expected assistant message");
    }

    if let Message::ToolResult(tr) = &eff_msgs[2] {
        assert_eq!(tr.details.as_ref().unwrap()["exitCode"], 0);
        assert_eq!(
            tr.added_tool_names.as_ref().unwrap(),
            &vec!["bash".to_string()]
        );
        assert_eq!(tr.usage.as_ref().unwrap().total_tokens, 3);
        if let pi_ai::Content::Image { mime_type, data } = &tr.content[0] {
            assert_eq!(mime_type, "image/png");
            assert_eq!(data, "base64data");
        } else {
            panic!("expected image content");
        }
    } else {
        panic!("Expected toolResult message");
    }

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn compaction_and_branch_summary_direct_serde_roundtrip() {
    let compaction_orig = serde_json::json!({
        "type": "compaction",
        "summary": "compacted context summary",
        "firstKeptEntryId": "entry-100",
        "tokensBefore": 12000,
        "retainedTail": [
            {
                "role": "user",
                "content": "tail content",
                "timestamp": 1700000000000i64
            }
        ],
        "details": {
            "hookName": "auto-compact",
            "nested": {
                "strategy": "retained-tail",
                "keptCount": 1
            }
        },
        "usage": {
            "promptTokens": 100,
            "completionTokens": 50,
            "cacheRead": 20,
            "customProviderField": "extra_val"
        },
        "fromHook": true
    });

    let compaction_entry: SessionEntry = serde_json::from_value(compaction_orig.clone()).unwrap();
    let compaction_saved = serde_json::to_value(&compaction_entry).unwrap();
    assert_eq!(compaction_saved, compaction_orig);

    let branch_orig = serde_json::json!({
        "type": "branch_summary",
        "summary": "branch context summary",
        "fromId": "node-5",
        "details": {
            "branchName": "feature/parity",
            "config": [1, 2, 3]
        },
        "usage": {
            "totalTokens": 450,
            "cost": {
                "input": 0.001,
                "output": 0.002
            }
        },
        "fromHook": false
    });

    let branch_entry: SessionEntry = serde_json::from_value(branch_orig.clone()).unwrap();
    let branch_saved = serde_json::to_value(&branch_entry).unwrap();
    assert_eq!(branch_saved, branch_orig);
}

#[test]
fn compaction_and_branch_summary_jsonl_import_save_reload_preserves_metadata() {
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-comp-meta-{}", std::process::id()));
    let in_path = dir.join("in.jsonl");
    let out_path = dir.join("out.jsonl");
    std::fs::create_dir_all(&dir).unwrap();

    let compaction_details = serde_json::json!({
        "hookName": "pre-compact-hook",
        "deep": {"a": [1, true, null]}
    });
    let compaction_usage = serde_json::json!({
        "promptTokens": 500,
        "completionTokens": 100,
        "vendor": {"rate": "flat"}
    });

    let branch_details = serde_json::json!({
        "originBranch": "main",
        "merged": false
    });
    let branch_usage = serde_json::json!({
        "totalTokens": 250
    });

    let lines = [
        serde_json::json!({"type": "session", "id": "s1"}),
        serde_json::json!({
            "type": "message",
            "id": "node-1",
            "timestamp": 1700000000000i64,
            "message": {
                "role": "user",
                "content": "msg 1"
            }
        }),
        serde_json::json!({
            "type": "compaction",
            "id": "node-2",
            "parentEntryId": "node-1",
            "timestamp": 1700000001000i64,
            "summary": "compacted state",
            "firstKeptEntryId": "node-1",
            "tokensBefore": 5000,
            "retainedTail": [
                {
                    "role": "user",
                    "content": "msg 1",
                    "timestamp": 1700000000000i64
                }
            ],
            "details": compaction_details,
            "usage": compaction_usage,
            "fromHook": true
        }),
        serde_json::json!({
            "type": "message",
            "id": "node-3",
            "parentEntryId": "node-2",
            "timestamp": 1700000002000i64,
            "message": {
                "role": "user",
                "content": "msg 2"
            }
        }),
        serde_json::json!({
            "type": "branch_summary",
            "id": "node-4",
            "parentEntryId": "node-3",
            "timestamp": 1700000003000i64,
            "summary": "branched summary state",
            "fromId": "node-1",
            "details": branch_details,
            "usage": branch_usage,
            "fromHook": false
        }),
    ];

    let jsonl = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        lines[0], lines[1], lines[2], lines[3], lines[4]
    );
    std::fs::write(&in_path, &jsonl).unwrap();

    let tree = session::import_pi_session_as_tree(&in_path).unwrap();
    save_tree_jsonl(&out_path, &tree).unwrap();

    let reloaded = session::import_pi_session_as_tree(&out_path).unwrap();
    let node2 = reloaded.nodes().find(|n| n.entry_id == "node-2").unwrap();
    if let SessionEntry::Compaction {
        details,
        usage,
        from_hook,
        ..
    } = &node2.entry
    {
        assert_eq!(details.as_ref(), Some(&compaction_details));
        assert_eq!(usage.as_ref(), Some(&compaction_usage));
        assert_eq!(*from_hook, Some(true));
    } else {
        panic!("expected compaction node");
    }

    let node4 = reloaded.nodes().find(|n| n.entry_id == "node-4").unwrap();
    if let SessionEntry::BranchSummary {
        details,
        usage,
        from_hook,
        ..
    } = &node4.entry
    {
        assert_eq!(details.as_ref(), Some(&branch_details));
        assert_eq!(usage.as_ref(), Some(&branch_usage));
        assert_eq!(*from_hook, Some(false));
    } else {
        panic!("expected branch_summary node");
    }

    // Verify saved JSONL line payloads for compaction and branch_summary after stripping node envelope fields equal original entry payloads
    let saved_content = std::fs::read_to_string(&out_path).unwrap();
    let saved_lines: Vec<serde_json::Value> = saved_content
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    assert_eq!(saved_lines.len(), 5);

    let strip_envelope = |mut val: serde_json::Value| {
        if let Some(obj) = val.as_object_mut() {
            obj.remove("id");
            obj.remove("parentId");
            obj.remove("parentEntryId");
            obj.remove("timestamp");
            obj.remove("source_entry_id");
        }
        val
    };

    // Line 2: compaction
    assert_eq!(
        strip_envelope(saved_lines[2].clone()),
        strip_envelope(lines[2].clone())
    );

    // Line 4: branch_summary
    assert_eq!(
        strip_envelope(saved_lines[4].clone()),
        strip_envelope(lines[4].clone())
    );

    // Verify context projection still projects summary correctly
    let eff_msgs = reloaded.effective_messages("node-4").unwrap();
    assert_eq!(eff_msgs.len(), 4); // compaction summary text, msg1 (retained tail), msg2, branch summary text

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn malformed_from_hook_fails_closed() {
    // Direct serde
    let bad_comp_null = serde_json::json!({
        "type": "compaction",
        "summary": "s",
        "fromHook": null
    });
    assert!(serde_json::from_value::<SessionEntry>(bad_comp_null).is_err());

    let bad_branch_str = serde_json::json!({
        "type": "branch_summary",
        "summary": "s",
        "fromHook": "true"
    });
    assert!(serde_json::from_value::<SessionEntry>(bad_branch_str).is_err());

    // Importer
    let dir = std::env::temp_dir().join(format!("pi-rs-u1-bad-hook-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let path_comp = dir.join("bad_comp.jsonl");
    std::fs::write(
        &path_comp,
        concat!(
            r#"{"type":"session","id":"s1"}"#,
            "\n",
            r#"{"type":"compaction","id":"c1","summary":"sum","fromHook":123}"#,
            "\n"
        ),
    )
    .unwrap();
    let err_comp = session::import_pi_session_as_tree(&path_comp).unwrap_err();
    assert!(err_comp.to_string().contains("line 2"));

    let path_branch = dir.join("bad_branch.jsonl");
    std::fs::write(
        &path_branch,
        concat!(
            r#"{"type":"session","id":"s1"}"#,
            "\n",
            r#"{"type":"branch_summary","id":"b1","summary":"sum","fromHook":[]}"#,
            "\n"
        ),
    )
    .unwrap();
    let err_branch = session::import_pi_session_as_tree(&path_branch).unwrap_err();
    assert!(err_branch.to_string().contains("line 2"));

    let _ = std::fs::remove_dir_all(dir);
}
