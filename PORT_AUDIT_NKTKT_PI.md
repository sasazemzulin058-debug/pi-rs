# Audit: `nktkt/pi` as the `pi-rs` base

**Audited checkout:** `/data/data/com.termux/files/usr/tmp/nktkt-pi-audit`  
**Repository:** <https://github.com/nktkt/pi>  
**Pinned candidate:** tag `v1.2.0`, commit `0808c756fa1991940def7f0f9837464417149419`  
**Method:** source and release metadata review only. No build, test, binary download, install, or source mutation was performed.

## Verdict

**ADOPT AS A FORK, NOT AS A DROP-IN REPLACEMENT.**

`nktkt/pi` eliminates most greenfield M1a work: Rust provider adapters, a streaming agent loop, built-in tools, print mode, JSON events, an interactive REPL, sessions, project instruction loading, CI, release workflow, and MIT licensing already exist.

It is not Pi `0.82.1` compatible by contract. Its own roadmap explicitly rejects one-to-one types and bug-for-bug compatibility. It stores a different session format and location, lacks documented Pi RPC/extensions/resources/trust behavior, and is not tested or packaged for Termux.

The correct product is a maintained fork with a Pi-compatibility layer and fixtures—not a rewrite and not an unmodified upstream install.

```text
Adopt:   core Rust workspace, provider codecs, agent loop, basic CLI/tool implementations.
Extend:  Termux packaging, Pi session importer/COW, Pi resource/trust behavior,
         fixture comparator, contract-compatible CLI/RPC, optional Node host.
Do not adopt unchanged: session storage, shell execution, exact tool contracts,
                       config location/schema, extension/security claims, release targets.
```

## Snapshot

| Item | Evidence | Assessment |
|---|---|---|
| License | MIT root `LICENSE` | Compatible with a fork; retain attribution. |
| Latest release | GitHub `v1.2.0`, 2026-05-12 | Usable source baseline, but stale relative to Pi 0.82.1 reference. |
| Published CLI crate | crates.io `pi-coding-agent` `1.0.0` only | Do not use `cargo install` as the base: it is behind `v1.2.0`. |
| Release assets | macOS + `*-unknown-linux-gnu` | No Android/Termux/bionic artifact. Linux aarch64 asset is unsuitable for Termux. |
| Workspace | `pi-ai`, `pi-agent`, `pi-coding-agent`; Rust 2021; MSRV 1.80 | Small and readable base. Fork can raise/pin toolchain deliberately. |
| CI | Linux/macOS × stable/1.80; fmt/clippy/test; cargo-deny/audit | Good base CI, no Android/Termux, reference fixture, Node bridge, or packaging test. |
| Tests | 22 claimed in changelog; narrow smoke tests in source | Insufficient proof of Pi behavior. |
| Dependencies | `reqwest 0.12` with `rustls-tls`, Tokio, `ignore`, `regex` | No OpenSSL/native-tls requirement apparent from workspace config. |

## What can be adopted

### `pi-ai`

- Anthropic Messages streaming.
- OpenAI Chat Completions SSE.
- OpenAI Responses SSE.
- Google Generative AI SSE.
- OpenAI-compatible base URLs.
- Retry classification / `Retry-After` support.
- Cancellation token, typed messages/tools/usage and pricing.

**Caveat:** this is a useful provider implementation, not Pi wire parity. Example gaps seen in `openai.rs` include dropped non-text user content, silently ignored malformed SSE JSON, default `reqwest::Client` transport policy, and no documented resource limits.

### `pi-agent`

- Serial streaming loop.
- Typed tool trait and tool results.
- Unknown tool errors.
- Per-tool permission prompt abstraction.
- Tools: `read`, `write`, `edit`, `bash`, `ls`, `grep`, `glob`, `web_fetch`, `todo`.

**Caveat:** agent events and tool semantics differ from Pi. No extension hooks, steering/follow-up queues, parallel execution semantics, contract schema validation, Pi session integration, or fixture suite exists.

### `pi-coding-agent`

- One-shot `-p` print mode and JSON output.
- Interactive REPL and slash commands.
- Simple config file and model selection.
- Ancestor `AGENTS.md`, `CLAUDE.md`, `.pi/instructions.md` loader.
- Session list/show/delete/resume UI.

**Caveat:** CLI flags, JSON event protocol, instruction precedence, trust policy, config location and session model do not equal Pi 0.82.1.

## Material compatibility gaps

| Pi-rs goal | `nktkt/pi` state | Required fork work |
|---|---|---|
| Pi session v1/v2/v3 JSONL | Uses `$XDG_CONFIG_HOME/pi/sessions/<id>.json`, one serialized transcript | Implement read-only Pi importer plus native COW; do not overwrite Pi files. |
| Native session durability | Plain create/write; no mode/lock/fsync/recovery policy; invalid sessions are skipped from list | `0700`/`0600`, interprocess lock, bounded input, atomic append/flush, partial-final-line recovery, degraded diagnostics. |
| Pi config/resource model | `config.toml`; only AGENTS/CLAUDE/`.pi/instructions.md` | Read Pi settings/resources with trust gate; skills/prompts/themes/package discovery staged after M1a. |
| Project trust | None | Canonical-root trust store, noninteractive deny, symlink escape checks. |
| Pi CLI / JSON / RPC | Different small flag set; custom JSON event names; no RPC | Treat existing modes as internal base; add Pi fixture-driven compatibility commands/events. |
| Pi tools | Similar names but divergent arguments/output/limits | Match Pi contracts case by case. |
| `read` | No 400-line/50 KiB default or byte limit | Add limits, continuation behavior, binary handling. |
| `bash` | Hardcodes `bash`; independent stdout/stderr readers; `child.kill()` only | Resolve Termux shell; process group; `SIGTERM`/grace/`SIGKILL`/reap; bounded output/temp file; deterministic merged stream policy. |
| `write` / `edit` | Direct writes; edit single string replace | Add atomicity/conflict/symlink policy and Pi multi-edit exact matching. |
| `grep` / `ls` / find | Basic `grep`, unsorted unlimited `ls`, `glob` instead of Pi `find` | Add Pi output/ignore/limit contracts. |
| Provider parity | Four API families exist | Add fixture-driven request/response semantics, transport limits, timeout/redirect/proxy policy, error redaction. |
| Extensions | None | M3 optional Node 22 host; no Bun. |
| OMP | None | Later allowlist after Pi Tier A extensions. |
| Termux release | Not targeted; release builds GNU Linux binaries only | Build natively in Termux; install to `$PREFIX`; clean-device smoke. |

## Termux findings

The source is a credible **compile candidate**, but this is not a build result.

Positive:

- `reqwest` is configured with `default-features = false` and `rustls-tls`; workspace does not declare `native-tls` or OpenSSL.
- Uses Rust/Tokio filesystem and process APIs; no platform-specific desktop framework was found.
- Local Termux has Rust, Cargo, `bash`, and Rustls-compatible network stack prerequisites.

Blockers to fix before a Termux claim:

1. `BashTool` executes literal `bash`, while Termux shell selection must support configured shell, `$SHELL`, `PATH` `sh`, and `$PREFIX/bin/sh`.
2. Tests hardcode `/tmp`; Termux contract must use `$TMPDIR`.
3. The GNU release asset cannot run on Android/bionic. Build in Termux; do not download it.
4. Current `bash` kill is single child kill; it does not create/terminate/reap a process group.
5. Native config/session resolution via `dirs::config_dir()` needs an explicit Termux path test and fork-owned state root.
6. No CI or manual test proves TTY restoration, process cleanup, CA behavior, Android suspend, or clean installation.

## Exact source findings

### Agent loop

`crates/pi-agent/src/agent_loop.rs` builds a cloned context each turn and executes tool calls serially. It executes tools only when stop reason is `ToolUse`, which is a useful starting behavior. It has no lifecycle hook registry, no queue/steer/follow-up, no `before_tool_call`/`after_tool_call`, and no explicit tool schema validator.

### Sessions

`crates/pi-coding-agent/src/session.rs` uses pretty JSON overwrite under a port-specific XDG location. It has no Pi JSONL import, session tree, locking, file mode, atomic append, fsync, source provenance, or corruption diagnostics. Its ID is time plus xorshift suffix rather than Pi UUID/session-tree identity.

### Resources

`crates/pi-coding-agent/src/project.rs` walks ancestors starting from CWD and reads `AGENTS.md`, `CLAUDE.md`, `.pi/instructions.md`; it does not implement Pi source ordering, trust, skills, prompt templates, themes, packages, or `SYSTEM.md` behavior.

### Tools

`bash.rs` emits separate pipe lines with `[stderr]` prefix and no output bound. `read.rs` has no Pi default output cap. `edit.rs` performs one replacement rather than Pi’s original-content multi-edit contract. `write.rs` directly overwrites. These must be replaced or wrapped before advertising Pi tool compatibility.

## Proposed adoption sequence

### A0 — Establish fork provenance

1. Replace the empty `/data/data/com.termux/files/home/pi-rs` Git history with a real fork or import `nktkt/pi` at tag `v1.2.0`.
2. Preserve its MIT license and add `UPSTREAM_NKTKT_PI.md` containing repository, tag, commit, license, and audit date.
3. Rename distribution binary to `pi-rs` during development to avoid replacing the installed upstream Pi binary.
4. Add remote names:

```text
origin       user-owned fork
nktkt        https://github.com/nktkt/pi.git
pi-upstream  https://github.com/earendil-works/pi-mono.git
```

### A1 — Contract harness before refactoring

1. Add GitHub Actions reference fixture capture against Pi `0.82.1` commit `2efa728d2ee90ef597626e96b1e28ef2b279f07c`.
2. Capture stable case IDs and implement structural comparison.
3. Add Termux-native manual/device job documentation; no GNU binary reuse.
4. Mark every existing behavior in `compatibility-matrix.json` as `candidate` until its fixture passes.

### A2 — Termux M1a fork

Implement only:

- safe Termux paths and package/install layout;
- fake provider plus loopback OpenAI SSE fixture server;
- print mode on a renamed `pi-rs` binary;
- Pi session importer + native COW store;
- `read` and `bash` Pi semantics;
- strict resource/output/transport limits;
- no Node or Bun dependency.

Do not spend M1a effort on inherited interactive REPL features merely because they exist.

### A3 — Pi compatibility gaps

Add resources/trust, remaining tools, CLI/JSON/RPC, then interactive TUI only with fixtures.

### A4 — Extensions

Only after A3 parity gate: Node 22 sidecar, Pi Tier A examples, then OMP allowlist. Never adopt `metaphorics/pi-rust`: it is archived and requires a Bun sidecar.

## Decision

**Proceed with `nktkt/pi v1.2.0` as the fork base after user confirms replacing the empty `~/pi-rs` repository history/content.**

This decision changes implementation from a greenfield rewrite to a compatibility hardening project. It does not reduce the need for upstream Pi fixtures, Termux checks, or the later Node extension host.
