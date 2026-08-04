# pi

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![CI](https://github.com/sasazemzulin058-debug/pi-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/sasazemzulin058-debug/pi-rs/actions/workflows/ci.yml)
[![pi-ai on crates.io](https://img.shields.io/crates/v/pi-ai.svg?label=pi-ai)](https://crates.io/crates/pi-ai)
[![pi-agent on crates.io](https://img.shields.io/crates/v/pi-agent.svg?label=pi-agent)](https://crates.io/crates/pi-agent)

A Rust port of [`earendil-works/pi`](https://github.com/earendil-works/pi) —
the pi agent harness — focused on the core coding-agent loop. Evaluated against main @ `fb9be67` and upstream `pi-mono` @ `f0deb8d`. Pinned old M1a oracle (`2efa728`) remains authoritative for M1a contract tests.

The upstream project is a TypeScript monorepo (~189k LOC). This port covers
the agent runtime, multi-provider LLM API, and CLI end-to-end and ships a
working `pi-rs` binary that talks to real Anthropic, OpenAI, Google, and any
OpenAI-compatible endpoint.

## Layout

```
pi/
├─ Cargo.toml                       # workspace, version 0.83.0, MSRV 1.80
└─ crates/
   ├─ pi-ai/                        # ←→ packages/ai
   ├─ pi-agent/                     # ←→ packages/agent
   └─ pi-coding-agent/              # ←→ packages/coding-agent (binary: `pi-rs`)
```

| TS package | Rust crate | Status |
| ------------ | ----------- | -------- |
| `@earendil-works/pi-ai` | `pi-ai` | **SSE streaming** for Anthropic Messages, OpenAI Chat Completions, Google Generative AI. Retry with `Retry-After`. Cancellation token. Custom headers. OpenAI-compatible passthrough (OpenRouter, Groq, etc.). |
| `@earendil-works/pi-agent-core` | `pi-agent` | Streaming `run_agent` loop with per-tool permission gate, typed `AgentError`, `#[instrument]` spans. Builtin tools: `read`, `write`, `edit`, `bash`, `ls`, `grep`, `glob`, `web_fetch`, `todo`. |
| `@earendil-works/pi-coding-agent` | `pi-coding-agent` | `pi-rs` CLI: print mode (`-p`), interactive REPL with streaming render, **session persistence** + `--resume`, **AGENTS.md / CLAUDE.md loader**, slash commands (`/help /reset /model /tools /cost /sessions /resume /session`), interactive permission prompts (`--yolo` to skip). `pi-rs sessions list/show/delete` subcommand. |
| `@earendil-works/pi-tui` | — | Not ported (TS terminal renderer). |
| `@earendil-works/pi-web-ui` | — | Not ported (browser components). |

## Reusing the runtime

The streaming LLM API and agent runtime are published on crates.io:

```toml
[dependencies]
pi-ai = "1.2"      # provider-agnostic streaming
pi-agent = "1.2"   # agent loop, permission policy, built-in tools
```

Install the CLI via:

```bash
cargo install pi-coding-agent   # installs binary `pi-rs`
```

See the crate-level docs at
[crates.io/crates/pi-ai](https://crates.io/crates/pi-ai) and
[crates.io/crates/pi-agent](https://crates.io/crates/pi-agent).

## Quick start

```bash
git clone https://github.com/sasazemzulin058-debug/pi-rs.git
cd pi-rs
cargo build --release

export ANTHROPIC_API_KEY=sk-ant-...
# or any of: OPENAI_API_KEY, GOOGLE_API_KEY / GEMINI_API_KEY

# One-shot:
./target/release/pi-rs -p "List the files in this directory and summarize them"

# Same prompt, JSON-lines on stdout for scripting:
./target/release/pi-rs -p "..." --json

# Interactive:
./target/release/pi-rs

# Resume a saved session:
./target/release/pi-rs --resume <id>

# Skip permission prompts (bash/write/edit run unconfirmed):
./target/release/pi-rs --yolo -p "Run the test suite"

# List saved sessions:
./target/release/pi-rs sessions list
```

Pick the model explicitly:

```bash
PI_MODEL=claude-opus-4-7   pi-rs -p "..."   # Anthropic
PI_MODEL=gpt-4o            pi-rs -p "..."   # OpenAI
PI_MODEL=gemini-2.0-flash  pi-rs -p "..."   # Google (via GOOGLE_API_KEY)
```

## OpenAI-compatible providers

Any base URL whose API matches OpenAI Chat Completions works through the
`openai-completions` code path. From Rust:

```rust
use pi_ai::Model;
let m = Model::openai_compat(
    "openrouter",
    "anthropic/claude-3.5-sonnet",
    "https://openrouter.ai/api/v1",
    200_000, 8_192,
);
```

Or override `StreamOptions::base_url` at call time. The same code path is
exercised by Groq, Together, Cerebras, DeepSeek, Fireworks, xAI, etc.

## Architecture

```
            ┌────────────────────────────┐
            │   pi-coding-agent (bin)    │
            │ print mode | interactive   │
            │ session persistence        │
            │ permission prompts         │
            │ AGENTS.md loader           │
            └──────────────┬─────────────┘
                           │ AgentConfig + tools + PermissionPolicy
                           ▼
            ┌────────────────────────────┐
            │         pi-agent           │
            │ run_agent / _with_history  │
            │ streaming events           │
            │ permission gate            │
            └──────────────┬─────────────┘
                           │ Context, StreamOptions (incl. CancellationToken)
                           ▼
            ┌────────────────────────────┐
            │           pi-ai            │
            │ stream_simple → Provider   │
            │  ├─ AnthropicProvider      │
            │  ├─ OpenAiProvider         │
            │  └─ GoogleProvider         │
            │  SSE + retry + cancel      │
            └────────────────────────────┘
```

## Agent loop

1. Append the user prompt onto the message transcript.
2. Build a `Context { system_prompt, messages, tools }` and call
   `pi_ai::stream_simple()`.
3. Consume the SSE event stream. Emit `TextDelta` events to the agent
   subscriber as they arrive (the CLI prints them live).
4. When the assistant finishes, append it. For each `Content::ToolCall`,
   consult `PermissionPolicy` if the tool flagged `requires_permission()`,
   then `execute()` and append the `ToolResultMessage`.
5. Repeat until `stop_reason ≠ ToolUse` or `max_turns` is reached.
6. After every turn the CLI persists the session to disk.

## Tests

```bash
cargo test          # 10 passing, no network
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs the same checks on macOS and Linux against stable + MSRV
(`1.80`).

## Roadmap & changelog

- [CHANGELOG.md](./CHANGELOG.md) — what shipped in 1.0.0.
- [ROADMAP.md](./ROADMAP.md) — future 1.x targets: OpenAI Responses API,
  Bedrock, prompt caching, MCP client, `--json` print mode, mdBook docs,
  crates.io publishing.

## License

MIT — same as the upstream project.
