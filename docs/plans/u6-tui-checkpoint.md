# U6 TUI Checkpoint: Partial Slice (Reducer + TerminalGuard)

## Status: In Progress / Partial

### Included in Slice

- `TerminalGuard`: RAII terminal raw-mode restoration via `libc`.
- Fallible `TerminalGuard::restore`; `Drop` remains best-effort.
- `TuiState`: Pure reducer consuming real `pi_agent::AgentEvent`.
- Wiring: `tui` module added to `pi-coding-agent`, without interactive startup wiring.
- Tests: Reducer coverage and non-TTY `TerminalGuard` behavior coverage. TTY tests intentionally skip to avoid mutating shared stdin terminal in parallel test runs.

### Excluded (Future Slices)

- No full renderer or `ratatui` components.
- No signal handlers.
- No REPL / `interactive.rs` behavior changes.
- No interactive startup wiring.
- No multiline input or session navigation UI.
- No U7 / U8 integration.

### Validation

- Focused TUI tests pass: 2 tests.
- `cargo check -p pi-coding-agent` passes.
- `cargo clippy -p pi-coding-agent --all-targets -- -D warnings` passes.
- Contract tests pass: 34 tests.
- `./scripts/check-doc-consistency` passes.
- `sh scripts/verify-termux` passes in current Termux environment; direct execution denied by script mode.
- Raw-mode transition and restoration remain untested because no isolated PTY dependency exists; production RAII behavior remains implemented but not claimed by tests.
