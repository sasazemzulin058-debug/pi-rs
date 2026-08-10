#[path = "../src/session.rs"]
#[allow(dead_code)]
mod session;

use pi_ai::Message;
use session::{load_tree_jsonl, save_tree_jsonl, SessionEntry, SessionTree};

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
fn invalid_parent_and_leaf_fail_closed() {
    let mut tree = SessionTree::new();
    assert!(tree.append(Some("missing"), message("x")).is_err());
    let root = tree.append(None, message("root")).unwrap();
    assert!(tree.append(None, message("second")).is_err());
    assert!(tree.set_active_leaf("missing").is_err());
    assert!(tree.effective_context(&root).is_ok());
}
