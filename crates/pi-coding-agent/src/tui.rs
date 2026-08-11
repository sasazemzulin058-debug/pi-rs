use std::io::Write;
use std::os::fd::AsRawFd;

use pi_agent::{AgentEvent, SessionPhase};

pub struct TerminalGuard {
    saved_termios: Option<libc::termios>,
    fd: i32,
}

impl TerminalGuard {
    pub fn enter_raw_mode() -> std::io::Result<Self> {
        let fd = std::io::stdin().as_raw_fd();
        if unsafe { libc::isatty(fd) } == 0 {
            return Ok(Self {
                saved_termios: None,
                fd,
            });
        }

        unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut termios) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            let saved_termios = termios;
            let mut raw = termios;
            libc::cfmakeraw(&mut raw);

            if libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) != 0 {
                return Err(std::io::Error::last_os_error());
            }

            Ok(Self {
                saved_termios: Some(saved_termios),
                fd,
            })
        }
    }

    pub fn is_raw(&self) -> bool {
        self.saved_termios.is_some()
    }

    /// Restore terminal settings. Safe to call repeatedly.
    pub fn restore(&mut self) -> std::io::Result<()> {
        let Some(saved) = self.saved_termios.take() else {
            return Ok(());
        };
        if unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &saved) } != 0 {
            self.saved_termios = Some(saved);
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentStatus {
    #[default]
    Idle,
    Running,
    Thinking,
    ToolExecuting(String),
    Error(String),
}

pub fn reduce(state: &mut TuiState, event: &AgentEvent) -> bool {
    let old_status = state.status.clone();
    let old_text_len = state.text_buffer.len();
    let old_thinking_len = state.thinking_buffer.len();

    match event {
        AgentEvent::AgentStart => {
            state.status = AgentStatus::Running;
            state.text_buffer.clear();
            state.thinking_buffer.clear();
        }
        AgentEvent::TextDelta { delta } => {
            state.status = AgentStatus::Running;
            state.text_buffer.push_str(delta);
        }
        AgentEvent::ThinkingDelta { delta } => {
            state.status = AgentStatus::Thinking;
            state.thinking_buffer.push_str(delta);
        }
        AgentEvent::ToolExecutionStart { tool_name, .. } => {
            state.status = AgentStatus::ToolExecuting(tool_name.clone());
        }
        AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        } => {
            state.status = if *is_error {
                AgentStatus::Error(format!("tool failed: {tool_name}"))
            } else {
                AgentStatus::Running
            };
        }
        AgentEvent::PermissionDenied { tool_name, reason } => {
            state.status = AgentStatus::Error(format!("permission denied: {tool_name}: {reason}"));
        }
        AgentEvent::AgentEnd { .. } | AgentEvent::Settlement { .. } => {
            state.status = AgentStatus::Idle;
        }
        AgentEvent::PhaseChange { phase } => {
            state.status = match phase {
                SessionPhase::Idle | SessionPhase::Settled => AgentStatus::Idle,
                SessionPhase::Executing | SessionPhase::Steering | SessionPhase::Compacting => {
                    AgentStatus::Running
                }
            };
        }
        AgentEvent::TurnStart
        | AgentEvent::TurnEnd
        | AgentEvent::AssistantMessage { .. }
        | AgentEvent::UserMessage { .. } => {}
    }

    state.status != old_status
        || state.text_buffer.len() != old_text_len
        || state.thinking_buffer.len() != old_thinking_len
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiState {
    pub status: AgentStatus,
    pub text_buffer: String,
    pub thinking_buffer: String,
}

impl TuiState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, event: &AgentEvent) -> bool {
        reduce(self, event)
    }
}

fn resolve_terminal_size(
    ioctl: Option<(u16, u16)>,
    columns: Option<u16>,
    lines: Option<u16>,
) -> (u16, u16) {
    if let Some((cols, rows)) = ioctl.filter(|(cols, rows)| *cols > 0 && *rows > 0) {
        return (cols, rows);
    }
    (
        columns.filter(|value| *value > 0).unwrap_or(80),
        lines.filter(|value| *value > 0).unwrap_or(24),
    )
}

pub fn get_terminal_size() -> (u16, u16) {
    #[cfg(unix)]
    let ioctl_size = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0)
            .then_some((ws.ws_col, ws.ws_row))
    };
    #[cfg(not(unix))]
    let ioctl_size = None;

    resolve_terminal_size(
        ioctl_size,
        std::env::var("COLUMNS").ok().and_then(|s| s.parse().ok()),
        std::env::var("LINES").ok().and_then(|s| s.parse().ok()),
    )
}

pub fn clean_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut mode = 0u8; // 0 plain, 1 ESC, 2 CSI, 3 OSC/string, 4 string ESC
    for c in input.chars() {
        match mode {
            1 => {
                mode = match c {
                    '[' => 2,
                    ']' | 'P' | '^' | '_' => 3,
                    _ if (0x40..=0x7e).contains(&(c as u32)) => 0,
                    _ => 1,
                }
            }
            2 => {
                if (0x40..=0x7e).contains(&(c as u32)) {
                    mode = 0
                }
            }
            3 => {
                if c == '\x07' {
                    mode = 0
                } else if c == '\x1b' {
                    mode = 4
                }
            }
            4 => mode = if c == '\\' { 0 } else { 3 },
            _ => {
                if c == '\x1b' {
                    mode = 1
                } else if c == '\n' || !c.is_control() {
                    out.push(c)
                }
            }
        }
    }
    out
}

fn clean_single(input: &str) -> String {
    clean_text(input)
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub fn render_frame(
    state: &TuiState,
    input: &str,
    model_name: &str,
    cols: u16,
    rows: u16,
) -> String {
    let width = cols as usize;
    let height = rows as usize;
    if width == 0 || height == 0 {
        return String::new();
    }

    let mut body_lines = Vec::new();

    if !state.thinking_buffer.is_empty() {
        body_lines.push("[thinking]".to_string());
        for line in clean_text(&state.thinking_buffer).lines() {
            body_lines.push(format!("  {line}"));
        }
    }

    if !state.text_buffer.is_empty() {
        for line in clean_text(&state.text_buffer).lines() {
            body_lines.push(line.to_string());
        }
    }

    let status_str = match &state.status {
        AgentStatus::Idle => "idle".to_string(),
        AgentStatus::Running => "running...".to_string(),
        AgentStatus::Thinking => "thinking...".to_string(),
        AgentStatus::ToolExecuting(t) => format!("executing {t}..."),
        AgentStatus::Error(e) => format!("error: {e}"),
    };

    let status_line = clean_single(&format!(
        "status: {status_str} | model: {}",
        clean_single(model_name)
    ));
    let input_line = clean_single(&format!("> {}", clean_single(input)));

    // Height 1 cannot contain status and prompt together.
    if height == 1 {
        return input_line.chars().take(width).collect();
    }
    let max_body_rows = height - 2;
    let visible_body = if body_lines.len() > max_body_rows {
        &body_lines[body_lines.len() - max_body_rows..]
    } else {
        &body_lines[..]
    };

    let mut out = String::new();
    for line in visible_body {
        let truncated: String = line.chars().take(width).collect();
        out.push_str(&truncated);
        out.push('\n');
    }

    // Fill remaining body height if any
    let filled = visible_body.len();
    for _ in filled..max_body_rows {
        out.push('\n');
    }

    out.push_str(&status_line.chars().take(width).collect::<String>());
    out.push('\n');
    out.push_str(&input_line.chars().take(width).collect::<String>());

    out
}

pub fn redraw_frame<W: Write>(
    writer: &mut W,
    state: &TuiState,
    input: &str,
    model_name: &str,
    cols: u16,
    rows: u16,
) -> std::io::Result<()> {
    let frame = render_frame(state, input, model_name, cols, rows);
    write!(writer, "\x1b[H\x1b[J{frame}")?;
    writer.flush()
}

pub enum InputResult {
    Submit(String),
    CancelLine,
    Exit,
    Continue,
}

#[derive(Debug, Clone, Default)]
pub struct LineBuffer {
    pub content: String,
    utf8: Vec<u8>,
    escape: EscapeState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EscapeState {
    #[default]
    Normal,
    Esc,
    Csi,
    String {
        esc: bool,
    },
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_byte(&mut self, b: u8) -> InputResult {
        // Controls must remain effective even while truncated escape sequence is buffered.
        match b {
            0x03 => {
                self.content.clear();
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                return InputResult::CancelLine;
            }
            0x04 => {
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                return if self.content.is_empty() {
                    InputResult::Exit
                } else {
                    InputResult::Continue
                };
            }
            b'\r' | b'\n' => {
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                return InputResult::Submit(std::mem::take(&mut self.content));
            }
            0x7f | 0x08 => {
                self.content.pop();
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                return InputResult::Continue;
            }
            _ => {}
        }
        if !matches!(self.escape, EscapeState::Normal) {
            match self.escape {
                EscapeState::Esc => {
                    self.escape = match b {
                        b'[' => EscapeState::Csi,
                        b']' | b'P' | b'^' | b'_' => EscapeState::String { esc: false },
                        _ => EscapeState::Normal,
                    }
                }
                EscapeState::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.escape = EscapeState::Normal
                    }
                }
                EscapeState::String { mut esc } => {
                    if b == 7 || (esc && b == b'\\') {
                        self.escape = EscapeState::Normal
                    } else {
                        esc = b == 0x1b;
                        self.escape = EscapeState::String { esc };
                    }
                }
                EscapeState::Normal => unreachable!(),
            }
            return InputResult::Continue;
        }
        if b == 0x1b {
            self.utf8.clear();
            self.escape = EscapeState::Esc;
            return InputResult::Continue;
        }
        match b {
            b'\r' | b'\n' => {
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                InputResult::Submit(std::mem::take(&mut self.content))
            }
            0x03 => {
                self.content.clear();
                self.utf8.clear();
                self.escape = EscapeState::Normal;
                InputResult::CancelLine
            }
            0x04 => {
                self.utf8.clear();
                if self.content.is_empty() {
                    InputResult::Exit
                } else {
                    InputResult::Continue
                }
            }
            0x7f | 0x08 => {
                self.content.pop();
                self.utf8.clear();
                InputResult::Continue
            }
            b if b.is_ascii() => {
                // ASCII boundary invalidates incomplete multibyte sequence.
                self.utf8.clear();
                if b.is_ascii_graphic() || b == b' ' {
                    self.content.push(b as char);
                }
                InputResult::Continue
            }
            b => {
                self.utf8.push(b);
                match std::str::from_utf8(&self.utf8) {
                    Ok(s) => {
                        self.content.push_str(s);
                        self.utf8.clear();
                    }
                    Err(e) if e.error_len().is_some() => self.utf8.clear(),
                    Err(_) => {}
                }
                InputResult::Continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_guard_does_not_touch_non_tty_stdin() {
        let stdin_is_tty = unsafe { libc::isatty(std::io::stdin().as_raw_fd()) != 0 };
        if stdin_is_tty {
            // A real terminal may be shared with other tests; never change its mode here.
            return;
        }

        let mut guard = TerminalGuard::enter_raw_mode().unwrap();
        assert!(!guard.is_raw());
        guard.restore().unwrap();
        guard.restore().unwrap();
        assert!(!guard.is_raw());
    }

    #[test]
    fn test_tui_state_reducer_flow() {
        let mut state = TuiState::new();
        assert_eq!(state.status, AgentStatus::Idle);

        assert!(state.apply(&AgentEvent::AgentStart));
        assert_eq!(state.status, AgentStatus::Running);

        assert!(state.apply(&AgentEvent::ThinkingDelta {
            delta: "thinking...".into(),
        }));
        assert_eq!(state.status, AgentStatus::Thinking);
        assert_eq!(state.thinking_buffer, "thinking...");

        assert!(state.apply(&AgentEvent::TextDelta {
            delta: "hello ".into(),
        }));
        assert_eq!(state.status, AgentStatus::Running);
        assert_eq!(state.text_buffer, "hello ");

        // Unhandled event returns false
        assert!(!state.apply(&AgentEvent::TurnStart));

        assert!(state.apply(&AgentEvent::ToolExecutionStart {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            args: Default::default(),
        }));
        assert_eq!(state.status, AgentStatus::ToolExecuting("bash".into()));

        assert!(state.apply(&AgentEvent::ToolExecutionEnd {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            is_error: false,
            content: vec![],
        }));
        assert_eq!(state.status, AgentStatus::Running);

        assert!(state.apply(&AgentEvent::AgentEnd { messages: vec![] }));
        assert_eq!(state.status, AgentStatus::Idle);

        assert!(state.apply(&AgentEvent::PermissionDenied {
            tool_name: "bash".into(),
            reason: "fail".into(),
        }));
        assert_eq!(
            state.status,
            AgentStatus::Error("permission denied: bash: fail".into())
        );
    }

    #[test]
    fn test_clean_text_strips_control_and_ansi() {
        let raw = "Hello \x1b[31mWorld\x1b[0m!\x07\nLine 2\tTabbed";
        let cleaned = clean_text(raw);
        assert_eq!(cleaned, "Hello World!\nLine 2Tabbed");
    }

    #[test]
    fn test_render_frame_structure_and_clipping() {
        let mut state = TuiState::new();
        state.text_buffer = "Line 1\nLine 2\nLine 3".into();
        state.status = AgentStatus::Running;

        // cols = 20, rows = 4 (2 body rows, 1 status row, 1 input row)
        let frame = render_frame(&state, "hello", "claude-3-5", 20, 4);
        let lines: Vec<&str> = frame.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "Line 2");
        assert_eq!(lines[1], "Line 3");
        assert_eq!(lines[2], "status: running... |");
        assert_eq!(lines[3], "> hello");
    }

    #[test]
    fn test_line_buffer_utf8_boundaries_and_escapes() {
        let mut buf = LineBuffer::new();
        for byte in "é界🙂".as_bytes() {
            assert!(matches!(buf.handle_byte(*byte), InputResult::Continue));
        }
        assert_eq!(buf.content, "é界🙂");
        buf.handle_byte(0x08);
        assert_eq!(buf.content, "é界");

        let mut buf = LineBuffer::new();
        buf.handle_byte(0xc3);
        buf.handle_byte(b'a');
        buf.handle_byte(0xa9);
        assert_eq!(buf.content, "a");
        buf.handle_byte(0x1b);
        buf.handle_byte(b'[');
        assert!(matches!(buf.handle_byte(0x03), InputResult::CancelLine));
        buf.handle_byte(0x1b);
        buf.handle_byte(b'[');
        buf.handle_byte(b'1');
        buf.handle_byte(b'~');
        buf.handle_byte(b'x');
        assert_eq!(buf.content, "x");
        buf.handle_byte(0x1b);
        buf.handle_byte(b']');
        buf.handle_byte(b't');
        buf.handle_byte(7);
        buf.handle_byte(b'y');
        assert_eq!(buf.content, "xy");
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write failed"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_redraw_failure_has_no_cursor_hide() {
        let mut writer = FailingWriter;
        let error = redraw_frame(&mut writer, &TuiState::new(), "", "model", 10, 2).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
    }

    #[test]
    fn test_terminal_size_rejects_zero_values() {
        assert_eq!(
            resolve_terminal_size(Some((0, 24)), Some(120), Some(40)),
            (120, 40)
        );
        assert_eq!(resolve_terminal_size(None, Some(0), Some(0)), (80, 24));
        assert_eq!(
            resolve_terminal_size(Some((100, 30)), Some(0), Some(0)),
            (100, 30)
        );
    }

    #[test]
    fn test_body_tabs_and_embedded_string_escape_are_consumed() {
        assert_eq!(clean_text("body\ttext"), "bodytext");
        let mut buf = LineBuffer::new();
        for byte in b"\x1b]title\x1bxpayload\x07ok" {
            buf.handle_byte(*byte);
        }
        assert_eq!(buf.content, "ok");
    }

    #[test]
    fn test_line_buffer_input_handling() {
        let mut buf = LineBuffer::new();
        assert!(matches!(buf.handle_byte(b'a'), InputResult::Continue));
        assert!(matches!(buf.handle_byte(b'b'), InputResult::Continue));
        assert_eq!(buf.content, "ab");

        assert!(matches!(buf.handle_byte(0x08), InputResult::Continue));
        assert_eq!(buf.content, "a");

        assert!(matches!(buf.handle_byte(0x03), InputResult::CancelLine));
        assert_eq!(buf.content, "");

        assert!(matches!(buf.handle_byte(0x04), InputResult::Exit));

        buf.handle_byte(b'x');
        assert!(matches!(buf.handle_byte(0x04), InputResult::Continue));
        assert_eq!(buf.content, "x");

        match buf.handle_byte(b'\n') {
            InputResult::Submit(s) => assert_eq!(s, "x"),
            _ => panic!("expected submit"),
        }
        assert_eq!(buf.content, "");
    }

    #[test]
    fn test_line_buffer_fake_reader_stream() {
        use std::io::Read;
        let input_bytes = b"hello\x08 world\n";
        let mut reader = &input_bytes[..];
        let mut line_buf = LineBuffer::new();
        let mut buf = [0u8; 1];
        let mut result = None;

        while let Ok(1) = reader.read(&mut buf) {
            match line_buf.handle_byte(buf[0]) {
                InputResult::Submit(s) => {
                    result = Some(s);
                    break;
                }
                InputResult::Continue => {}
                _ => panic!("unexpected input result"),
            }
        }
        assert_eq!(result.as_deref(), Some("hell world"));
    }

    #[test]
    fn test_tui_matrix_rendering_states() {
        let mut state = TuiState::new();
        state.status = AgentStatus::Idle;
        let frame = render_frame(&state, "", "model-x", 40, 5);
        assert!(frame.contains("status: idle"));

        state.apply(&AgentEvent::AgentStart);
        let frame = render_frame(&state, "", "model-x", 40, 5);
        assert!(frame.contains("status: running..."));

        state.apply(&AgentEvent::ThinkingDelta {
            delta: "thinking".into(),
        });
        let frame = render_frame(&state, "", "model-x", 40, 5);
        assert!(frame.contains("status: thinking..."));

        state.apply(&AgentEvent::TextDelta {
            delta: "hello world".into(),
        });
        let frame = render_frame(&state, "", "model-x", 40, 5);
        assert!(frame.contains("hello world"));

        state.apply(&AgentEvent::ToolExecutionStart {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            args: Default::default(),
        });
        let frame = render_frame(&state, "", "model-x", 40, 5);
        assert!(frame.contains("status: executing bash..."));

        state.apply(&AgentEvent::PermissionDenied {
            tool_name: "bash".into(),
            reason: "denied".into(),
        });
        let frame = render_frame(&state, "", "model-x", 80, 5);
        assert!(frame.contains("status: error: permission denied: bash: denied"));
    }

    #[test]
    fn test_render_frame_exact_small_heights_and_width() {
        let mut state = TuiState::new();
        state.status = AgentStatus::Running;
        state.text_buffer = "old\nnew".into();
        assert_eq!(render_frame(&state, "x", "m", 10, 0), "");
        assert_eq!(render_frame(&state, "x", "m", 0, 4), "");
        assert_eq!(render_frame(&state, "x", "m", 10, 1), "> x");
        let frame = render_frame(&state, "x", "m", 10, 2);
        assert_eq!(frame.lines().count(), 2);
        assert_eq!(frame.lines().next(), Some("status: ru"));
        assert_eq!(frame.lines().last(), Some("> x"));
    }

    #[test]
    fn test_render_frame_sanitizes_all_dynamic_fields_and_unicode() {
        let mut state = TuiState {
            status: AgentStatus::Error("bad\x1b[31m\x07\r\n\t".into()),
            text_buffer: "body\x1b]x\x1b\\\x07\r\n\t".into(),
            thinking_buffer: "think\x1bPpayload\x1b\\".into(),
        };
        let frame = render_frame(&state, "🙂界\x1b[2J", "模型\x1b[31m", 8, 6);
        assert!(!frame.contains('\x1b'));
        assert!(!frame.contains('\x07'));
        assert!(!frame.contains('\r'));
        assert!(!frame.contains('\t'));
        assert!(frame.lines().count() <= 6);
        assert!(std::str::from_utf8(frame.as_bytes()).is_ok());
        state.text_buffer = "é界🙂".into();
        for width in 1..=4 {
            let frame = render_frame(&state, "", "m", width, 4);
            assert!(std::str::from_utf8(frame.as_bytes()).is_ok());
        }
    }

    #[test]
    fn test_reducer_settlement_and_next_turn_reset() {
        let mut state = TuiState::new();
        state.apply(&AgentEvent::AgentStart);
        state.apply(&AgentEvent::ThinkingDelta {
            delta: "think".into(),
        });
        state.apply(&AgentEvent::TextDelta {
            delta: "text".into(),
        });
        state.apply(&AgentEvent::ToolExecutionEnd {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            is_error: true,
            content: vec![],
        });
        assert!(matches!(state.status, AgentStatus::Error(_)));
        state.apply(&AgentEvent::Settlement {
            messages: vec![],
            cancelled: false,
        });
        assert_eq!(state.status, AgentStatus::Idle);
        assert_eq!(state.text_buffer, "text");
        assert_eq!(state.thinking_buffer, "think");
        state.apply(&AgentEvent::AgentStart);
        assert_eq!(state.text_buffer, "");
        assert_eq!(state.thinking_buffer, "");
        assert_eq!(state.status, AgentStatus::Running);
    }
}
