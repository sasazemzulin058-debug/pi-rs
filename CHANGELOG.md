# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] — 2026-05-12

Ten focused features merged in from 10 parallel sub-agent worktrees.

### Added

- **`pi-ai`**
  - **Anthropic prompt caching** — new `CacheRetention { None, Short, Long }`
    on `StreamOptions`. When set, the Anthropic provider stamps
    `cache_control` markers on the system prompt, the last tool definition,
    and the last text block of the last user message. `Long` adds `ttl: 1h`.
  - **OpenAI Responses API provider** (`openai-responses`) — covers
    `o-series` / `gpt-5*` reasoning models. SSE handling for
    `response.output_text.delta`, `response.output_item.added`,
    `response.function_call_arguments.delta`, `response.completed`.
    `Model::openai_gpt_5()` constructor.
  - **Per-model pricing** — new `ModelPricing` on `Model` and `Cost` on
    `Usage`. After each stream completes, the provider computes USD cost
    from token usage × per-million-token rates. Realistic 2026 prices wired
    for Sonnet, Opus, GPT-4o/4o-mini, Gemini 2.0 Flash. New `Cost` /
    `ModelPricing` types in the public API.

- **`pi-agent`**
  - **`bash` streaming + persistent cwd** — `Stdio::piped()` collector
    interleaves stdout/stderr by arrival. Intercepts `cd <path>` to update a
    per-tool `Mutex<PathBuf>`; subsequent commands run with `.current_dir()`.
    New `BashTool::new()` constructor (existing call sites updated).
  - **`edit` unified-diff preview** — tool result now has two text blocks:
    `"edited PATH: N replacement(s)"` plus a unified diff with `a/PATH`,
    `b/PATH` headers from the `similar` crate. CRLF line endings preserved.
  - **`grep` regex + context** — pattern is regex by default; new
    `fixed_string`, `before`, `after`. Matches use `path:lineno:line`,
    context lines use `path-lineno-line` (rg convention).

- **`pi-coding-agent`**
  - **`pi-rs/config.toml`** — `$XDG_CONFIG_HOME/pi-rs/config.toml` loaded before
    argv. Keys: `model`, `max_turns`, `thinking_level`, `yolo`, `json`.
    CLI flags and env still win; the file fills holes.
  - **`/compact` slash command** — summarizes older turns (anything before
    the last 4) into a single synthetic user message via a one-shot
    summarization call. Keeps the persistent session compact across long
    sessions.

- **Tooling**
  - **mdBook docs scaffold** under `docs/` with chapters for the CLI,
    SDK, providers, and contributing. New `.github/workflows/docs.yml`
    builds the book and (on `main`) deploys to GitHub Pages via
    `actions/deploy-pages@v4`. Repo Pages setting is left to maintainers.
  - **Supply-chain CI** — new `supply-chain` job in `ci.yml` runs
    `cargo deny check` and `cargo audit`. `deny.toml` allows the common
    permissive SPDX set (including `CDLA-Permissive-2.0` for
    `webpki-roots`).

### Changed

- Workspace bumped to **1.2.0**. Internal path-dep version specifiers
  updated to match.

### Engineering notes

This release was assembled by dispatching 10 sub-agents in parallel git
worktrees (`agent-1` .. `agent-10`), each producing a focused commit.
Merging was sequential with three real content conflicts: `pi-agent/Cargo.toml`
(deps from agent-4 + agent-5), `pi-ai/src/lib.rs` (re-exports from agent-7 +
agent-10), and `pi-ai/src/types.rs` `Model::openai_gpt_5` (missing
`pricing` field after agent-10 added it). All resolved by hand; full `cargo
test` (22 tests), `cargo fmt --check`, and `cargo clippy --all-targets
-- -D warnings` clean on the merged tree.

## [1.1.0] — 2026-05-12

### Added

- **`pi -p ... --json`** — emit JSON-lines on stdout instead of human text.
  Stable event types: `agent_start`, `turn_start`, `turn_end`,
  `user_message`, `assistant_message`, `text_delta`, `thinking_delta`,
  `tool_start`, `tool_end`, `permission_denied`, `agent_end`. `agent_end`
  is emitted by the CLI after the agent loop returns and includes
  `stopped_at_turn_limit` and `message_count`.
- Tests pinning the JSON-lines schema.

### Notes

- The `--json` contract follows semver from this release. Future additive
  fields are allowed; renaming existing fields is a breaking change.

## [1.0.0] — 2026-05-12

The first stable release of the Rust port. Everything in milestones 0.2.0
through 1.0.0 of the original ROADMAP has shipped.

### Added

- **`pi-ai`**
  - Server-Sent Events streaming for **Anthropic Messages**: per-block
    `text_delta` / `thinking_delta` / `toolcall_delta` events, `Usage`
    aggregation, and stop-reason mapping.
  - SSE streaming for **OpenAI Chat Completions** with `include_usage`,
    tool-call assembly across multiple deltas, and `[DONE]` handling.
  - **Google Generative AI** provider (`google-generative-ai`) targeting
    Gemini's `streamGenerateContent?alt=sse` endpoint.
  - **OpenAI-compatible passthrough**: any base URL (OpenRouter, Groq,
    Together, Cerebras, DeepSeek, Fireworks, xAI, …) works through the
    OpenAI Chat Completions provider via `Model::openai_compat()` or
    `StreamOptions::base_url`.
  - Shared retry helper with exponential back-off and `Retry-After` parsing;
    classifies 429 / 5xx as retry-worthy.
  - Cancellation through the agent loop and SSE reader via
    `StreamOptions::cancel: CancellationToken`.
  - Custom request headers via `StreamOptions::headers`.

- **`pi-agent`**
  - Typed `AgentError` enum (replaces `String` errors) plus
    `#[tracing::instrument]` spans around each turn.
  - **Permission policy hook** — `PermissionPolicy` trait with
    `Allow` / `AllowSession` / `Deny` decisions; tools advertise
    `requires_permission()`. Defaults: `bash` / `write` / `edit` gated;
    read-only tools open.
  - Streaming `AgentEvent::TextDelta` and `ThinkingDelta` events for
    incremental UI rendering.
  - `run_agent_with_history` entry point for resumed sessions.
  - New tools: **`web_fetch`** (HTTP GET → coarse-text extraction) and
    **`todo`** (in-memory checklist).

- **`pi-coding-agent` (binary `pi-rs`)**
  - Streaming REPL render — assistant text prints as it arrives.
  - **Session persistence** under `$XDG_CONFIG_HOME/pi/sessions/<id>.json`.
    Sessions are saved after every turn and listable / loadable.
    Subcommands: `pi-rs sessions list | show <id> | delete <id>`.
    Flag: `pi-rs --resume <id>` resumes interactively.
  - **AGENTS.md / CLAUDE.md / `.pi/instructions.md`** loader walks up from
    `cwd` and concatenates each file into the system prompt.
  - New slash commands: `/help /reset /model /tools /cost /sessions
    /resume <id> /session /quit /exit`.
  - Interactive per-tool permission prompt; `--yolo` to skip.

- **Tooling**
  - GitHub Actions `ci.yml`: matrix of macOS + Linux × {stable, 1.80}, runs
    `cargo build`, `cargo test`, `cargo fmt --check`, `cargo clippy
    -- -D warnings`.
  - GitHub Actions `release.yml`: builds 4 release binaries
    (`aarch64-apple-darwin`, `x86_64-apple-darwin`,
    `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`) and attaches
    them to a GitHub Release on every `v*` tag.
  - `rust-toolchain.toml` pins the toolchain channel and components.
  - Workspace declares MSRV 1.80.

### Changed

- All HTTP-backed providers now stream by default; the previous
  single-POST/replayed-as-Done behavior is gone.
- Workspace version bumped to **1.0.0**.

### Published

- [`pi-ai`](https://crates.io/crates/pi-ai) and
  [`pi-agent`](https://crates.io/crates/pi-agent) published to crates.io
  alongside the GitHub release.

### Not in 1.0 (future milestones)

- AWS Bedrock and OpenAI Responses API
- Prompt-caching markers and OAuth flows
- MCP (Model Context Protocol) client
- TUI / browser UIs (out of scope)
