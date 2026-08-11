# U6 TUI Checkpoint: Renderer & Raw Mode Implementation

## Status: IN_PROGRESS

### Included in Slice
- Executable minimal terminal renderer (`render_frame`, `redraw_frame`, `get_terminal_size`) in `tui.rs`.
- Minimal raw-mode line input (`LineBuffer`, `InputResult`, `TerminalGuard`) supporting incremental UTF-8, Enter, Backspace, Ctrl-C, Ctrl-D, and consumed CSI/OSC escapes.
- Interactive startup and event flow wiring in `interactive.rs`:
  - `isatty` check selecting TUI vs line-REPL fallback.
  - Event loop applying `AgentEvent` to `TuiState` with state-change-driven redraws.
  - Raw mode restored before slash commands and agent execution to maintain canonical permission prompt behavior.
  - Clean terminal restoration on all exit paths.
- Tokio `SIGWINCH` window resize listener handling on Unix targets.
- Focused unit tests in `tui.rs` verifying reducer flow, control sequence cleaning, frame layout/clipping, UTF-8 boundaries, Unicode backspace, and line buffer inputs.

### Exclusions & Ceiling
- Upstream differential main-screen renderer, `ratatui` components, multiline editor, overlays, themes, mouse/image support, Kitty keyboard protocol, session selector UI, and running agent cancellation are explicitly excluded.

### Validation Status
- U6 reducer, UTF-8 boundary handling, escape/control reset including embedded-ESC strings, body-tab sanitization, row limits, cursor-safe redraw, zero-size fallback, and running-task cleanup fixes applied.
- `cargo test -p pi-coding-agent tui`, `cargo test -p pi-coding-agent interactive`, and `cargo check -p pi-coding-agent` passed in current worktree.
- Added pure terminal-size fallback tests and bounded failing-writer redraw test; interactive fake-reader/raw-termios tests remain unimplemented.
- `cargo clippy -p pi-coding-agent --all-targets -- -D warnings` remains not run in this override; prior review recorded unrelated dirty U4/U5/U7 diagnostics (`resources.rs`, `rpc/server.rs`, `extension.rs`).
- `./scripts/check-doc-consistency` and `sh ./scripts/verify-termux` remain to be run after this override.
- Manual PTY checks for termios restoration, cursor visibility, resize, permission prompts, and forced I/O errors remain pending.
