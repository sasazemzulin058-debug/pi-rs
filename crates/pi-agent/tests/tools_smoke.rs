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
    std::fs::write(dir.join("a.txt"), "x").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let res = ls::LsTool
        .execute("1", json!({"path": dir.to_string_lossy()}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("a.txt"));
    assert!(text.contains("sub"));
}

#[tokio::test]
async fn grep_finds_pattern() {
    let dir = scratch_dir();
    std::fs::write(dir.join("a.txt"), "needle\nhaystack\n").unwrap();
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

    let pattern = format!("{}/*.rs", dir.to_string_lossy());
    let res = glob_tool::GlobTool
        .execute("1", json!({"pattern": pattern}))
        .await
        .unwrap();
    let text = res.content[0].as_text().unwrap();
    assert!(text.contains("a.rs"));
    assert!(text.contains("b.rs"));
    assert!(!text.contains("c.txt"));
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
    use std::time::Instant;

    let dir = scratch_dir();
    let pidfile = dir.join("child.pid");
    let marker = dir.join("child.done");

    let tool = bash::BashTool::new();
    let cmd = format!(
        "sh -c 'echo $$ > {}; exec sleep 2' & ( sleep 2; touch {} ) & wait",
        pidfile.display(),
        marker.display()
    );

    let start = Instant::now();
    let res = tool
        .execute(
            "1",
            json!({
                "command": cmd,
                "timeout_ms": 200
            }),
        )
        .await;

    let elapsed = start.elapsed();
    assert!(res.is_err(), "expected timeout error");
    let err = res.unwrap_err();
    assert!(err.contains("timed out"), "unexpected error string: {err}");
    assert!(
        elapsed.as_millis() < 2000,
        "timeout took too long: {}ms",
        elapsed.as_millis()
    );

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let pid_str = std::fs::read_to_string(&pidfile)
        .expect("pidfile must be created and readable");
    let pid: i32 = pid_str
        .trim()
        .parse()
        .expect("pidfile must contain valid integer pid");

    let is_alive = unsafe { libc::kill(pid, 0) == 0 };
    assert!(!is_alive, "descendant process {pid} was not killed");

    assert!(!marker.exists(), "marker file should not exist");
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
