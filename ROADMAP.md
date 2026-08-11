# Roadmap

> **Status:** Pi-only denominator is upstream `pi-mono` 0.83.0 at pinned commit
> `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`. ACP v1 external standard and Node
> extension runtime are separate appendices, never denominator. Full parity plan:
> `docs/plans/full-parity-roadmap.md`. Status vocabulary: supported, partial,
> candidate, read-only, unsupported, deferred. Unchecked items make no parity claim.
>
> **Evidence boundary:** committed M1a comparator corpus is invalid: direct
> `python3 scripts/compare-contract-fixtures --milestone M1a --actual fixtures/upstream-pi`
> reports 0 passed / 41 failures/errors. Local `sh ./scripts/verify-termux` uses
> transient generated outputs and does not validate committed actual fixtures.

## Milestone 1 — Provider parity for streaming ✅ (historical 1.0.0)

- [x] **SSE parsing for Anthropic Messages** (`stream: true`) — emit
      `text_delta` / `thinking_delta` / `toolcall_delta` as they arrive.
- [x] **SSE parsing for OpenAI Chat Completions** (`stream: true`,
      `stream_options.include_usage: true`).
- [x] Surface `Usage` deltas; aggregate the final usage into the
      `AssistantMessage`.
- [x] Cancellation: thread a `CancellationToken` through the stream so
      callers can cancel mid-response.
- [x] Retry policy with exponential back-off and `Retry-After` honoring.

## Milestone 2 — Coding-agent UX ✅ (historical 1.0.0)

- [x] Streaming render in the REPL.
- [x] **Session persistence** under `$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl`;
      `pi-rs --resume <id>` and `pi-rs sessions list / show / delete`.
- [x] **`AGENTS.md` / project-prompt loading**.
- [x] Slash commands: `/clear` (as `/reset`), `/cost`, `/tools`, `/sessions`,
      `/resume`, `/session`, `/help`, `/model`, `/quit`, `/exit`.
- [x] **Print-mode JSON output** (`-p --json`) — emit structured events for
      scripting.
- [x] **Config file** at `$XDG_CONFIG_HOME/pi-rs/config.toml`.
- [x] `/compact` (auto-summarize context to free room).

## Milestone 3 — Tool ecosystem

- [x] **Per-call permission prompts** with allow / allow-session / deny.
- [x] **New tools**: `web_fetch`, `todo`.
- [x] **`bash` improvements**: streamed stdout/stderr, process-group cancellation, persistent cwd.
- [x] **`edit` polish**: unified-diff preview before write.
- [x] **`grep` upgrade**: regex mode, context lines.
- [ ] **MCP (Model Context Protocol) client**.

## Milestone 4 — More providers

- [x] **Google Generative AI / Vertex AI** (Gemini via
      `streamGenerateContent?alt=sse`).
- [x] **OpenAI-compatible passthrough** — `Model::openai_compat(...)` or
      `StreamOptions::base_url` covers OpenRouter, Together, Groq, Cerebras,
      DeepSeek, Fireworks, xAI, etc.
- [x] **OpenAI Responses API** (`openai-responses`).
- [ ] **AWS Bedrock Converse Stream**.
- [x] **Prompt cache markers** — Anthropic `cache_control`.
- [ ] **OAuth flows** for Copilot, Codex.

## Milestone 5 — Full Parity & Protocol Standards

- [x] **CI**: GitHub Actions matrix and documented workspace checks.
- [x] **MSRV**: declared as `1.80` in workspace and CI.
- [x] **Release pipeline**: pre-built binaries for macOS, Linux, and Termux per tag via `release.yml`.
- [ ] **Pi JSONL RPC (`--mode rpc`)**: Rust recognizes exact 11/32 upstream RPC commands (`prompt`, `steer`, `follow_up`, `abort`, `new_session`, `get_state`, queue-mode controls, and thinking-level controls); thinking controls are a source-attested implemented subset, with `max` collapsed to `xhigh`, not upstream-captured parity evidence.
- [ ] **Pi public surface catalog**: SDK, model/runtime, settings/auth, compaction, event bus, sessions, resources, tools, TUI, and package manager remain inventoried as partial/candidate/deferred.
- [ ] **Agent Client Protocol v1 (`--mode acp`)**: External ACP appendix only; generic transport groundwork exists, coding-agent mode and conformance remain deferred.
- [ ] **Node extension track**: Discovery exists; dynamic JS/TS execution hook, full API ABI, and packaging remain deferred. Node stays separate from Pi denominator.

## Non-goals

- One-to-one type compatibility with TS internal code (idiomatic Rust).
- Bug-for-bug compat with TS provider quirks when inconsistent with specs.

## Contributing

See [docs/plans/full-parity-roadmap.md](docs/plans/full-parity-roadmap.md) for full execution plan details and [CHANGELOG.md](./CHANGELOG.md) for shipped changes.
