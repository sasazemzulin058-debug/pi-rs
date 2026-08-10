#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::Message;
use session::{load_tree_jsonl, save_tree_jsonl, Session, SessionEntry, SessionTree};

fn message(text: &str) -> SessionEntry {
    SessionEntry::Message {
        message: Message::user_text(text),
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
            pi_ai::Content::Text { text } => text.clone(),
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
