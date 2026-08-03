# Compatibility Matrix

This document defines the implementation milestone and compatibility status of each documented Pi/OMP interface.

## Compatibility Statuses

* **candidate**: Inherited or planned behavior awaiting its required fixture on the declared target; it makes no Pi compatibility claim.
* **supported**: Required fixture passes on the declared target.
* **read-only**: Input is consumed but `pi-rs` never mutates/writes it.
* **unsupported**: Stable diagnostic is emitted before intentional API use where detectable. Trusted legacy module top-level code can still execute before Node reports an unsupported runtime dependency.
* **deferred**: Planned for a future milestone; no compatibility claim is active.

---

## Contracts Catalog

### Milestone M1a (Termux Headless Slice)

| ID | Surface | Description / Contract | Status | Target Fixture |
| --- | --- | --- | --- | --- |
| `cli.print` | CLI | `--print` headless prompt execution | **candidate** | `cli.print.basic` |
| `agent.serial-tools` | Agent | Serial tool call validation, execution and cancellation | **candidate** | `agent.serial-tool-loop` |
| `provider.openai-chat` | Provider | OpenAI Chat Completions compatible SSE with local mock | **candidate** | `provider.openai-chat.fragmented-sse` |
| `provider.google` | Provider | Google gemini-2.0-flash explicit model routing | **supported** | Local verification |
| `tool.read` | Built-in Tool | Bounded text read with 1-indexed offsets | **candidate** | `tool.read.bounds` |
| `tool.bash` | Built-in Tool | Shell execution, process-group cancellation and reaping | **candidate** | `tool.bash.cancel-descendants` |
| `resources.context` | Resources | Global/current `AGENTS.md` or `CLAUDE.md` context discovery | **candidate** | `resource.context-precedence` |
| `resources.trust` | Resources | Trust decision data model; no project executable resource loading | **candidate** | `resource.untrusted-project` |
| `session.native-write` | Session | Fork-native versioned JSONL append/recovery format; legacy single JSON is accepted only as migration input | **candidate** | `session.native-append-recover` |
| `session.pi-import` | Session | Read-only import of original Pi v1/v2/v3 sessions; imported files are never mutated | **read-only** | `session.pi-import-checksum` |
| `session.pi-cow` | Session | COW fork-native session created on first mutation of an imported Pi session | **candidate** | `session.pi-cow-provenance` |
| `extension.none-required` | Extensions | Core functionality operates normally when Node is absent | **candidate** | `extension.node-absent` |

### Milestone M1 (Expanded Headless Pi)

| ID | Surface | Description / Contract | Status | Target Fixture |
| --- | --- | --- | --- | --- |
| `cli.json-events` | CLI | Structured JSON event output | **deferred** | None |
| `agent.retry-auto-compaction` | Agent | Serial retry and one automatic context-overflow compaction retry | **deferred** | None |
| `tool.write` | Built-in Tool | Atomic write semantics | **deferred** | None |
| `tool.edit` | Built-in Tool | Exact multi-edit semantics | **deferred** | None |
| `tool.grep-find-ls` | Built-in Tool | Ignored-path discovery and bounded search | **deferred** | None |
| `resources.skills-prompts-themes` | Resources | Skills, prompt templates and themes | **deferred** | None |

### Milestone M2 (Pi Interactive & Public API Parity)

| ID | Surface | Description / Contract | Status | Target Fixture |
| --- | --- | --- | --- | --- |
| `cli.interactive` | CLI | Interactive terminal mode | **deferred** | None |
| `cli.rpc` | CLI | Public Pi JSONL RPC | **deferred** | None |
| `agent.parallel-tools` | Agent | Parallel batch ordering and cancellation | **deferred** | None |
| `agent.manual-compaction-scheduling` | Agent | Manual compaction and scheduling interaction with queue/steer/follow-up | **deferred** | None |
| `provider.anthropic` | Provider | Anthropic Messages | **deferred** | None |
| `provider.openai-responses` | Provider | OpenAI Responses | **deferred** | None |
| `provider.google` | Provider | Google Generative AI | **deferred** | None |

### Milestone M3 (Legacy Extension Host)

| ID | Surface | Description / Contract | Status | Target Fixture |
| --- | --- | --- | --- | --- |
| `resources.remote-packages` | Resources | `npm:` and `git:` package source installation | **unsupported** | `resource.remote-package-diagnostic` |
| `session.pi-inplace-write` | Session | In-place mutation of Pi session files | **unsupported** | `session.pi-inplace-write-diagnostic` |
| `extension.pi-tier-a` | Extensions | Tools, commands, cancelable hooks, persisted state and basic UI via Node host | **deferred** | None |
| `extension.bun-private-native-custom-ui` | Extensions | Bun APIs, private imports, native addons and custom Pi TUI components | **unsupported** | `extension.unsupported-capability` |
| `extension.omp` | Extensions | Oh My Pi public API intersection | **deferred** | None |
