//! Interactive (REPL) mode: read user lines from stdin, run agent turns,
//! print streaming output, and dispatch slash commands.

use std::io::{BufRead, Read, Write};
use std::os::fd::AsRawFd;
use std::sync::Arc;

use futures::StreamExt;
use pi_agent::{run_agent_with_history, tools::default_tools, AgentConfig, AgentEvent};
use pi_ai::{AssistantMessageEvent, Context, Message, StreamOptions};
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::permission::CliPermission;
use crate::session::Session;
use crate::system_prompt::build_system_prompt;
use crate::tui::{
    get_terminal_size, redraw_frame, InputResult, LineBuffer, TerminalGuard, TuiState,
};

fn redraw_in_raw_mode<W: Write>(
    guard: &mut TerminalGuard,
    writer: &mut W,
    state: &TuiState,
    input: &str,
    model_name: &str,
    cols: u16,
    rows: u16,
) -> std::io::Result<()> {
    match redraw_frame(writer, state, input, model_name, cols, rows) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = guard.restore();
            Err(error)
        }
    }
}

fn read_prompt<R: Read, W: Write, F: FnMut() -> (u16, u16)>(
    guard: &mut TerminalGuard,
    reader: &mut R,
    writer: &mut W,
    state: &TuiState,
    model_name: &str,
    initial_size: (u16, u16),
    mut size: F,
) -> std::io::Result<Option<String>> {
    redraw_in_raw_mode(
        guard,
        writer,
        state,
        "",
        model_name,
        initial_size.0,
        initial_size.1,
    )?;
    let mut line_buf = LineBuffer::new();
    let mut buf = [0u8; 1];
    loop {
        match reader.read(&mut buf) {
            Ok(1) => match line_buf.handle_byte(buf[0]) {
                InputResult::Submit(line) => {
                    guard.restore()?;
                    return Ok(Some(line));
                }
                InputResult::CancelLine => {
                    let (cols, rows) = size();
                    redraw_in_raw_mode(guard, writer, state, "", model_name, cols, rows)?;
                }
                InputResult::Exit => {
                    guard.restore()?;
                    return Ok(None);
                }
                InputResult::Continue => {
                    let (cols, rows) = size();
                    redraw_in_raw_mode(
                        guard,
                        writer,
                        state,
                        &line_buf.content,
                        model_name,
                        cols,
                        rows,
                    )?;
                }
            },
            Ok(0) => {
                guard.restore()?;
                return Ok(None);
            }
            Err(error) => {
                let _ = guard.restore();
                return Err(error);
            }
            Ok(_) => unreachable!(),
        }
    }
}

async fn redraw_running_task<W: Write, T>(
    guard: &mut TerminalGuard,
    writer: &mut W,
    state: &TuiState,
    model_name: &str,
    size: (u16, u16),
    handle: &mut tokio::task::JoinHandle<T>,
) -> std::io::Result<()> {
    if let Err(error) = redraw_in_raw_mode(guard, writer, state, "", model_name, size.0, size.1) {
        handle.abort();
        let _ = handle.await;
        return Err(error);
    }
    Ok(())
}

pub async fn run_interactive(
    app: &AppConfig,
    permission: Arc<CliPermission>,
    initial: Option<Session>,
    trust_context: (
        crate::trust::TrustDecision,
        Option<crate::trust::CanonicalProjectRoot>,
    ),
) -> anyhow::Result<()> {
    let stdin_fd = std::io::stdin().as_raw_fd();
    let stdout_fd = std::io::stdout().as_raw_fd();
    let is_tty = unsafe { libc::isatty(stdin_fd) != 0 && libc::isatty(stdout_fd) != 0 };

    if is_tty {
        run_interactive_tui(app, permission, initial, trust_context).await
    } else {
        run_interactive_repl(app, permission, initial, trust_context).await
    }
}

async fn run_interactive_tui(
    app: &AppConfig,
    permission: Arc<CliPermission>,
    initial: Option<Session>,
    trust_context: (
        crate::trust::TrustDecision,
        Option<crate::trust::CanonicalProjectRoot>,
    ),
) -> anyhow::Result<()> {
    let mut session = initial.unwrap_or_else(|| Session::new(&app.model));
    let system_prompt = build_system_prompt(&app.config_dir, &trust_context);
    let mut state = TuiState::new();

    #[cfg(unix)]
    let mut sigwinch =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()).ok();

    loop {
        let (cols, rows) = get_terminal_size();
        let prompt = {
            let mut guard = TerminalGuard::enter_raw_mode()?;
            let mut stdout = std::io::stdout();
            let mut stdin = std::io::stdin();
            match read_prompt(
                &mut guard,
                &mut stdin,
                &mut stdout,
                &state,
                &app.model.name,
                (cols, rows),
                get_terminal_size,
            )? {
                Some(prompt) => prompt,
                None => return Ok(()),
            }
        };

        let prompt_trimmed = prompt.trim().to_string();
        if prompt_trimmed.is_empty() {
            continue;
        }

        if prompt_trimmed.starts_with('/') {
            if prompt_trimmed == "/compact" || prompt_trimmed.starts_with("/compact ") {
                if let Err(e) = handle_compact(app, &mut session).await {
                    eprintln!("compact failed: {e}");
                }
                continue;
            }
            if !handle_slash(&prompt_trimmed, app, &permission, &mut session)? {
                break;
            }
            continue;
        }

        let cfg = AgentConfig::new(app.model.clone(), system_prompt.clone())
            .with_tools(default_tools())
            .with_max_turns(app.max_turns)
            .with_thinking(app.thinking_level)
            .with_permission(permission.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let user = Message::user_text(prompt_trimmed);
        let mut history = session.messages.clone();
        history.push(user);

        let cfg_cloned = cfg.clone();
        let mut handle =
            tokio::spawn(
                async move { run_agent_with_history(&cfg_cloned, history, Some(tx)).await },
            );

        let mut stdout = std::io::stdout();
        let (c, r) = get_terminal_size();
        let mut guard = TerminalGuard::enter_raw_mode()?;
        redraw_running_task(
            &mut guard,
            &mut stdout,
            &state,
            &app.model.name,
            (c, r),
            &mut handle,
        )
        .await?;

        loop {
            #[cfg(unix)]
            let sig_recv = async {
                if let Some(ref mut sig) = sigwinch {
                    sig.recv().await
                } else {
                    futures::future::pending().await
                }
            };
            #[cfg(not(unix))]
            let sig_recv = futures::future::pending::<()>();

            tokio::select! {
                ev = rx.recv() => {
                    match ev {
                        Some(event) => {
                            if state.apply(&event) {
                                let (c, r) = get_terminal_size();
                                redraw_running_task(
                                    &mut guard,
                                    &mut stdout,
                                    &state,
                                    &app.model.name,
                                    (c, r),
                                    &mut handle,
                                )
                                .await?;
                            }
                        }
                        None => break,
                    }
                }
                _ = sig_recv => {
                    let (c, r) = get_terminal_size();
                    redraw_running_task(
                        &mut guard,
                        &mut stdout,
                        &state,
                        &app.model.name,
                        (c, r),
                        &mut handle,
                    )
                    .await?;
                }
            }
        }

        let res = handle.await??;
        let _ = guard.restore();
        session.replace_messages(res.messages);
        if let Err(e) = crate::session::save(&app.config_dir, &session) {
            eprintln!("(warning: session save failed: {e})");
        }
    }

    Ok(())
}

async fn run_interactive_repl(
    app: &AppConfig,
    permission: Arc<CliPermission>,
    initial: Option<Session>,
    trust_context: (
        crate::trust::TrustDecision,
        Option<crate::trust::CanonicalProjectRoot>,
    ),
) -> anyhow::Result<()> {
    eprintln!(
        "pi-rs — model: {} ({})  •  slash commands: /help",
        app.model.name, app.model.provider
    );

    let mut session = initial.unwrap_or_else(|| Session::new(&app.model));
    if !session.messages.is_empty() {
        eprintln!(
            "(resumed session {}, {} prior messages)",
            session.id,
            session.messages.len()
        );
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let system_prompt = build_system_prompt(&app.config_dir, &trust_context);

    loop {
        write!(stdout, "\n> ")?;
        stdout.flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let prompt = line.trim().to_string();
        if prompt.is_empty() {
            continue;
        }
        if prompt.starts_with('/') {
            if prompt == "/compact" || prompt.starts_with("/compact ") {
                if let Err(e) = handle_compact(app, &mut session).await {
                    eprintln!("compact failed: {e}");
                }
                continue;
            }
            if !handle_slash(&prompt, app, &permission, &mut session)? {
                break;
            }
            continue;
        }

        let cfg = AgentConfig::new(app.model.clone(), system_prompt.clone())
            .with_tools(default_tools())
            .with_max_turns(app.max_turns)
            .with_thinking(app.thinking_level)
            .with_permission(permission.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let user = Message::user_text(prompt);
        let mut history = session.messages.clone();
        history.push(user);

        let cfg_cloned = cfg.clone();
        let handle =
            tokio::spawn(
                async move { run_agent_with_history(&cfg_cloned, history, Some(tx)).await },
            );

        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::TextDelta { delta } => {
                    let _ = write!(stdout, "{delta}");
                    let _ = stdout.flush();
                }
                AgentEvent::AssistantMessage { .. } => {
                    let _ = writeln!(stdout);
                }
                AgentEvent::ToolExecutionStart {
                    tool_name, args, ..
                } => {
                    eprintln!("  → {}({})", tool_name, args);
                }
                AgentEvent::ToolExecutionEnd {
                    tool_name,
                    is_error,
                    ..
                } => {
                    eprintln!(
                        "  ← {} {}",
                        tool_name,
                        if is_error { "error" } else { "ok" }
                    );
                }
                AgentEvent::PermissionDenied { tool_name, reason } => {
                    eprintln!("  ✗ {tool_name} denied: {reason}");
                }
                _ => {}
            }
        }
        let res = handle.await??;
        session.replace_messages(res.messages);
        if let Err(e) = crate::session::save(&app.config_dir, &session) {
            eprintln!("(warning: session save failed: {e})");
        }
    }
    Ok(())
}

/// Returns `false` if the loop should exit (e.g. `/quit`).
fn handle_slash(
    line: &str,
    app: &AppConfig,
    permission: &CliPermission,
    session: &mut Session,
) -> anyhow::Result<bool> {
    let (cmd, rest) = match line.split_once(' ') {
        Some((c, r)) => (c, r.trim()),
        None => (line, ""),
    };
    match cmd {
        "/quit" | "/exit" => return Ok(false),
        "/help" => {
            eprintln!("/quit /exit          quit pi-rs");
            eprintln!("/help                show this help");
            eprintln!(
                "/reset               clear in-memory transcript (does not delete session file)"
            );
            eprintln!("/model               print current model");
            eprintln!("/tools               list builtin tools");
            eprintln!("/cost                print accumulated cost/usage so far");
            eprintln!("/sessions            list saved sessions");
            eprintln!("/resume <id>         load a saved session by id");
            eprintln!("/session             print current session id");
            eprintln!("/compact             summarize older messages into a recap");
        }
        "/reset" => {
            permission.reset_session();
            *session = Session::new(&app.model);
            eprintln!("(reset; new session id {})", session.id);
        }
        "/model" => {
            eprintln!("model: {} ({})", app.model.name, app.model.provider);
        }
        "/tools" => {
            for t in default_tools() {
                eprintln!("- {}: {}", t.name(), t.description());
            }
        }
        "/cost" => {
            let mut total_in = 0u64;
            let mut total_out = 0u64;
            let mut total_cost = 0.0f64;
            for m in &session.messages {
                if let Message::Assistant(a) = m {
                    total_in += a.usage.input;
                    total_out += a.usage.output;
                    total_cost += a.usage.cost.total;
                }
            }
            eprintln!("tokens: in={total_in} out={total_out}  cost: ${total_cost:.4}");
        }
        "/sessions" => {
            let summaries = crate::session::list(&app.config_dir)?;
            if summaries.is_empty() {
                eprintln!("(no saved sessions)");
            }
            for s in summaries.iter().take(20) {
                let first = truncate(&s.first_message, 60);
                eprintln!("{}  ({} msgs, {})  {}", s.id, s.turns, s.model, first);
            }
        }
        "/session" => {
            eprintln!("{}", session.id);
        }
        "/resume" => {
            if rest.is_empty() {
                eprintln!("usage: /resume <id>");
            } else {
                match crate::session::load(&app.config_dir, rest) {
                    Ok(s) => {
                        eprintln!("loaded session {} ({} messages)", s.id, s.messages.len());
                        *session = s;
                    }
                    Err(e) => eprintln!("load failed: {e}"),
                }
            }
        }
        other => {
            eprintln!("unknown command: {other} — try /help");
        }
    }
    Ok(true)
}

/// Summarize all but the last 4 messages into a single synthetic user
/// message, replacing the older slice in-place. Prints `(nothing to compact)`
/// if fewer than 4 messages exist.
async fn handle_compact(app: &AppConfig, session: &mut Session) -> anyhow::Result<()> {
    let total = session.messages.len();
    if total < 4 {
        eprintln!("(nothing to compact)");
        return Ok(());
    }
    let keep_from = total - 4;
    let older: Vec<Message> = session.messages[..keep_from].to_vec();
    let older_count = older.len();

    let ctx = Context {
        system_prompt: Some(
            "Summarize this conversation into a compact context-preserving recap. \
             Include files mentioned, decisions, and open todos."
                .into(),
        ),
        messages: older,
        tools: Vec::new(),
    };

    let mut stream = pi_ai::stream_simple(&app.model, &ctx, &StreamOptions::default()).await?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let AssistantMessageEvent::TextDelta { delta, .. } = event? {
            summary.push_str(&delta);
        }
    }

    let recap = Message::user_text(format!("[compacted summary]\n{summary}"));
    let mut new_messages = Vec::with_capacity(5);
    new_messages.push(recap);
    new_messages.extend(session.messages.drain(keep_from..));
    session.replace_messages(new_messages);
    crate::session::save(&app.config_dir, session)?;
    eprintln!("compacted {older_count} messages");
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= n {
        s
    } else {
        let head: String = s.chars().take(n).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct ErrorReader;
    impl Read for ErrorReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("reader failed"))
        }
    }

    struct ErrorWriter;
    impl Write for ErrorWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("writer failed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fake_prompt_reader_submit_and_eof() {
        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        let mut reader = Cursor::new(b"hello\n".to_vec());
        let mut writer = Vec::new();
        let result = read_prompt(
            &mut guard,
            &mut reader,
            &mut writer,
            &TuiState::new(),
            "m",
            (20, 4),
            || (20, 4),
        )
        .unwrap();
        assert_eq!(result.as_deref(), Some("hello"));

        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut writer = Vec::new();
        assert_eq!(
            read_prompt(
                &mut guard,
                &mut reader,
                &mut writer,
                &TuiState::new(),
                "m",
                (20, 4),
                || (20, 4)
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn fake_prompt_errors_preserve_io_error() {
        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        let mut writer = Vec::new();
        let error = read_prompt(
            &mut guard,
            &mut ErrorReader,
            &mut writer,
            &TuiState::new(),
            "m",
            (20, 4),
            || (20, 4),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "reader failed");

        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        let error = read_prompt(
            &mut guard,
            &mut Cursor::new(b"x".to_vec()),
            &mut ErrorWriter,
            &TuiState::new(),
            "m",
            (20, 4),
            || (20, 4),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "writer failed");
    }

    #[tokio::test]
    async fn running_task_redraw_error_aborts_task() {
        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        let mut writer = ErrorWriter;
        let mut handle = tokio::spawn(async { futures::future::pending::<()>().await });
        let error = redraw_running_task(
            &mut guard,
            &mut writer,
            &TuiState::new(),
            "m",
            (20, 4),
            &mut handle,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "writer failed");
        assert!(handle.is_finished());
    }
}
