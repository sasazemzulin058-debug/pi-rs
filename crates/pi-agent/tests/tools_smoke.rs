//! Smoke tests for builtin tools. No LLM is required — these exercise the
//! tool implementations directly against a tempfile-backed scratch dir.

use std::path::PathBuf;
use std::sync::Arc;

use pi_agent::tools::{bash, edit, glob_tool, grep, ls, read, write};
use pi_agent::types::AgentTool;
use serde_json::json;

fn scratch_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tid = format!("{:?}", std::thread::current().id());
    let tid_filtered: String = tid.chars().filter(|c| c.is_alphanumeric()).collect();
    let dir = std::env::temp_dir().join(format!("pi-rs-test-{n:x}-{c}-{tid_filtered}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn write_then_read_roundtrips() {
    let dir = scratch_dir();
    let path = dir.join("hello.txt");
    let path_s = path.to_string_lossy().to_string();

    let res = write::WriteTool
        .execute("1", json!({"path": path_s, "content": "hello\nworld\n"}))
        .await
        .unwrap();
    assert!(matches!(res.content[0], pi_ai::Content::Text { .. }));

    let res = read::ReadTool
        .execute("2", json!({"path": path_s}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap().to_string();
    assert!(text.contains("hello"));
    assert!(text.contains("world"));
}

#[tokio::test]
async fn write_atomic_overwrite_leaves_no_temp_residue() {
    let dir = scratch_dir();
    let path = dir.join("replace.txt");
    std::fs::write(&path, "original").unwrap();

    write::WriteTool
        .execute("1", json!({"path": path, "content": "replaced"}))
        .await
        .unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "replaced");
    let residue = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .any(|name| name.to_string_lossy().contains(".tmp."));
    assert!(!residue, "temporary write file was left behind");
}

#[tokio::test]
async fn write_creates_nested_parents() {
    let dir = scratch_dir();
    let path = dir.join("a/b/c/nested.txt");

    write::WriteTool
        .execute("1", json!({"path": path, "content": "nested"}))
        .await
        .unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "nested");
}

#[tokio::test]
async fn write_failure_preserves_existing_path() {
    let dir = scratch_dir();
    let parent_file = dir.join("not-a-directory");
    std::fs::write(&parent_file, "original").unwrap();
    let path = parent_file.join("target.txt");

    let result = write::WriteTool
        .execute("1", json!({"path": path, "content": "replacement"}))
        .await;

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&parent_file).unwrap(), "original");
    let residue = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .any(|name| name.to_string_lossy().contains(".tmp."));
    assert!(
        !residue,
        "temporary write file was left behind after failure"
    );

    let target_dir = dir.join("target-dir");
    std::fs::create_dir(&target_dir).unwrap();
    let result = write::WriteTool
        .execute("2", json!({"path": target_dir, "content": "replacement"}))
        .await;
    assert!(result.is_err());
    assert!(target_dir.is_dir());
    let residue = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .any(|name| name.to_string_lossy().contains(".tmp."));
    assert!(
        !residue,
        "temporary write file was left behind after rename failure"
    );
}

#[tokio::test]
async fn edit_replace_all_replaces_every_occurrence() {
    let dir = scratch_dir();
    let path = dir.join("all.txt");
    std::fs::write(&path, "foo foo foo").unwrap();

    edit::EditTool
        .execute(
            "1",
            json!({"path": path, "old_string": "foo", "new_string": "bar", "replace_all": true}),
        )
        .await
        .unwrap();

    assert_eq!(std::fs::read_to_string(path).unwrap(), "bar bar bar");
}

#[tokio::test]
async fn edit_rejects_missing_and_ambiguous_matches() {
    let dir = scratch_dir();
    let path = dir.join("matches.txt");
    std::fs::write(&path, "foo foo").unwrap();

    let ambiguous = edit::EditTool
        .execute(
            "1",
            json!({"path": path, "old_string": "foo", "new_string": "bar"}),
        )
        .await
        .unwrap_err();
    assert!(ambiguous.contains("occurs 2 times"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo foo");

    let missing = edit::EditTool
        .execute(
            "2",
            json!({"path": path, "old_string": "missing", "new_string": "bar"}),
        )
        .await
        .unwrap_err();
    assert!(missing.contains("old_string not found"));
}

#[tokio::test]
async fn edit_preserves_crlf_line_endings() {
    let dir = scratch_dir();
    let path = dir.join("crlf.txt");
    std::fs::write(&path, b"foo\r\nbar\r\n").unwrap();

    edit::EditTool
        .execute(
            "1",
            json!({"path": path, "old_string": "bar", "new_string": "BAR"}),
        )
        .await
        .unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"foo\r\nBAR\r\n");
}

#[cfg(unix)]
#[tokio::test]
async fn edit_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch_dir();
    let path = dir.join("permissions.txt");
    std::fs::write(&path, "foo").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    edit::EditTool
        .execute(
            "1",
            json!({"path": path, "old_string": "foo", "new_string": "bar"}),
        )
        .await
        .unwrap();

    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777,
        0o600
    );
}

#[cfg(unix)]
#[tokio::test]
async fn edit_follows_symlink_and_preserves_target_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let dir = scratch_dir();
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    std::fs::write(&target, "foo").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &link).unwrap();

    edit::EditTool
        .execute(
            "1",
            json!({"path": link, "old_string": "foo", "new_string": "bar"}),
        )
        .await
        .unwrap();

    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "bar");
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
        0o600
    );
}

#[tokio::test]
async fn edit_replaces_single_occurrence() {
    let dir = scratch_dir();
    let path = dir.join("a.txt");
    let path_s = path.to_string_lossy().to_string();
    std::fs::write(&path, "foo bar baz").unwrap();

    let res = edit::EditTool
        .execute(
            "1",
            json!({"path": path_s, "old_string": "bar", "new_string": "BAR"}),
        )
        .await
        .unwrap();
    assert!(res.content[0].as_text().unwrap().contains("edited"));
    let diff = res.content[1].as_text().unwrap();
    assert!(
        diff.contains("-foo bar baz"),
        "diff missing '-foo bar baz':\n{diff}"
    );
    assert!(
        diff.contains("+foo BAR baz"),
        "diff missing '+foo BAR baz':\n{diff}"
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, "foo BAR baz");
}

#[tokio::test]
async fn ls_lists_dir() {
    let dir = scratch_dir();
    std::fs::write(dir.join("b.txt"), "x").unwrap();
    std::fs::write(dir.join("a.txt"), "x").unwrap();
    std::fs::write(dir.join(".hidden"), "x").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let res = ls::LsTool
        .execute("1", json!({"path": dir.to_string_lossy()}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("a.txt"));
    assert!(text.contains("sub"));
    assert!(!text.contains(".hidden"));
    assert!(text.find("a.txt").unwrap() < text.find("b.txt").unwrap());

    let res = ls::LsTool
        .execute(
            "2",
            json!({"path": dir.to_string_lossy(), "limit": 1, "show_hidden": true}),
        )
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("truncated at 1"));
    assert!(text.contains(".hidden"));
}

#[tokio::test]
async fn grep_finds_pattern() {
    let dir = scratch_dir();
    std::fs::write(dir.join("z.txt"), "needle\nhaystack\n").unwrap();
    std::fs::write(dir.join("a.txt"), "needle\nfirst\n").unwrap();
    std::fs::write(dir.join("b.txt"), "nothing here\n").unwrap();

    let res = grep::GrepTool
        .execute(
            "1",
            json!({"pattern": "needle", "path": dir.to_string_lossy()}),
        )
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("a.txt"));
    assert!(text.contains("needle"));
    assert!(text.find("a.txt").unwrap() < text.find("z.txt").unwrap());
}

#[tokio::test]
async fn grep_output_is_bounded() {
    let dir = scratch_dir();
    std::fs::write(dir.join("large.txt"), "needle\n".repeat(20_000)).unwrap();
    let res = grep::GrepTool
        .execute(
            "1",
            json!({"pattern": "needle", "path": dir.to_string_lossy(), "max_matches": 20_000}),
        )
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(
        text.len() <= 50 * 1024,
        "output exceeded bound: {}",
        text.len()
    );
    assert!(text.contains("... (truncated at 51200 bytes)"));
}

#[tokio::test]
async fn grep_context_lines() {
    let dir = scratch_dir();
    let sub = dir.join("dir");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("a.txt"), "alpha\nbeta needle gamma\ndelta\n").unwrap();

    let res = grep::GrepTool
        .execute(
            "1",
            json!({
                "pattern": "needle",
                "path": sub.to_string_lossy(),
                "before": 1,
                "after": 1,
            }),
        )
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("alpha"), "expected 'alpha' in: {text}");
    assert!(
        text.contains("beta needle gamma"),
        "expected 'beta needle gamma' in: {text}"
    );
    assert!(text.contains("delta"), "expected 'delta' in: {text}");
}

#[tokio::test]
async fn glob_finds_files() {
    let dir = scratch_dir();
    std::fs::write(dir.join("a.rs"), "").unwrap();
    std::fs::write(dir.join("b.rs"), "").unwrap();
    std::fs::write(dir.join("c.txt"), "").unwrap();
    std::fs::create_dir(dir.join(".git")).unwrap();
    std::fs::write(dir.join(".gitignore"), "ignored.rs\n").unwrap();
    std::fs::write(dir.join("ignored.rs"), "").unwrap();
    let nested = dir.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("nested.rs"), "").unwrap();

    let pattern = format!("{}/*.rs", dir.to_string_lossy());
    let res = glob_tool::GlobTool
        .execute("1", json!({"pattern": pattern}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("a.rs"));
    assert!(text.contains("b.rs"));
    assert!(!text.contains("c.txt"));
    assert!(!text.contains("ignored.rs"));
    assert!(!text.contains("nested.rs"));
    assert!(text.find("a.rs").unwrap() < text.find("b.rs").unwrap());

    let res = glob_tool::GlobTool
        .execute(
            "2",
            json!({"pattern": format!("{}/*.rs", dir.display()), "max": 1}),
        )
        .await
        .unwrap();
    let truncated = res.content[0].as_text().unwrap();
    assert!(truncated.contains("a.rs"));
    assert!(truncated.contains("truncated at 1"));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn glob_relative_patterns_use_relative_paths() {
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = CWD_LOCK.lock().unwrap();
    let dir = scratch_dir();
    let nested = dir.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(dir.join("root.rs"), "").unwrap();
    std::fs::write(nested.join("child.rs"), "").unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let root = glob_tool::GlobTool
        .execute("relative-root", json!({"pattern": "*.rs"}))
        .await
        .unwrap();
    let root_text = root.content[0].as_text().unwrap().to_string();

    let nested_result = glob_tool::GlobTool
        .execute("relative-nested", json!({"pattern": "nested/*.rs"}))
        .await
        .unwrap();
    let nested_text = nested_result.content[0].as_text().unwrap().to_string();
    std::env::set_current_dir(old).unwrap();
    assert_eq!(root_text, "root.rs\n", "root output: {root_text:?}");
    assert_eq!(
        nested_text, "nested/child.rs\n",
        "nested output: {nested_text:?}"
    );
}

#[tokio::test]
async fn glob_filter_controls_are_explicit() {
    let dir = scratch_dir();
    std::fs::create_dir(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join(".hidden/file.rs"), "").unwrap();
    let pattern = format!("{}/.hidden/*.rs", dir.display());
    let result = glob_tool::GlobTool
        .execute("filters", json!({"pattern": pattern, "show_hidden": true}))
        .await
        .unwrap();
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .contains(".hidden/file.rs"),
        "glob output: {:?}",
        result.content[0].as_text().unwrap()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bash_bounded_timeout_and_pipe_closure() {
    use std::time::Instant;

    let tool = bash::BashTool::new();
    let start = Instant::now();
    let res = tool
        .execute(
            "1",
            json!({
                "command": "sleep 30",
                "timeout_ms": 200
            }),
        )
        .await;

    let elapsed = start.elapsed();
    assert!(res.is_err(), "expected timeout error");
    let err = res.unwrap_err();
    assert!(err.contains("timed out"), "unexpected error string: {err}");
    assert!(
        elapsed.as_millis() < 5000,
        "timeout took too long: {}ms",
        elapsed.as_millis()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn bash_timeout_kills_descendants() {
    let dir = scratch_dir();
    let started = dir.join("started");
    let done = dir.join("done");

    std::fs::write(&started, "").unwrap();

    let tool = bash::BashTool::new();
    let done_str = done.to_string_lossy();

    let cmd = format!("(sleep 10; touch \"{done_str}\") & wait");

    let res = tool
        .execute(
            "1",
            json!({
                "command": cmd,
                "timeout_ms": 1000
            }),
        )
        .await;

    assert!(res.is_err(), "expected timeout error");
    let err = res.unwrap_err();
    assert!(err.contains("timed out"), "unexpected error string: {err}");

    assert!(started.exists(), "started marker file should exist");

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(!done.exists(), "done marker file should not exist");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn bash_runs_simple_command() {
    let tool = bash::BashTool::new();
    let res = tool
        .execute("1", json!({"command": "echo hi-from-bash"}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("hi-from-bash"));
    assert!(text.contains("[exit 0]"));
}

#[tokio::test]
async fn bash_persists_cwd_across_calls() {
    let tool: Arc<bash::BashTool> = Arc::new(bash::BashTool::new());

    let res = tool
        .execute("1", json!({"command": "cd /tmp"}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("cwd"), "got: {text}");

    let res = tool.execute("2", json!({"command": "pwd"})).await.unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("/tmp"), "got: {text}");
}
