# Pi Compatibility Matrix

## Denominator and scope

Pi-only upstream denominator is `pi-mono` 0.83.0 at commit
`f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee` (upstream checkout:
`/data/data/com.termux/files/home/pi-mono-oracle-083`). This matrix inventories
public coding-agent surfaces exported by
`packages/coding-agent/src/index.ts` and `packages/coding-agent/src/core/sdk.ts`,
plus Pi RPC commands from `packages/coding-agent/src/modes/rpc/rpc-types.ts`.

ACP is external and appears only in separate appendix. ACP rows never enter Pi
denominator or parity percentages. Node runtime/sidecar is separate from both
Pi denominator and ACP.

## Status vocabulary

- **supported**: behavior implemented and covered by declared evidence.
- **partial**: behavior exists for subset; upstream-compatible coverage incomplete.
- **candidate**: identified surface with no compatibility claim yet.
- **read-only**: input can be consumed without mutation.
- **unsupported**: intentional diagnostic or explicit non-support boundary.
- **deferred**: planned, not implemented; no compatibility claim.

## Evidence ledger

| Evidence | Result | Boundary |
| --- | --- | --- |
| `python3 scripts/compare-contract-fixtures --milestone M1a --actual fixtures/upstream-pi` | **invalid actual corpus: 0 passed / 41 failures/errors** | Committed `*.actual.json` files are incomplete/stale and do not prove parity. |
| `sh ./scripts/verify-termux` | passed locally | Generates transient actual corpus; does not repair committed comparator corpus or prove hosted attestation. |
| `python3 scripts/validate-fixture-manifest` | passed | Manifest structure only. |
| `./scripts/check-doc-consistency` | passed | Existing checker paths preserved; checker is not full denominator gate. |
| `python3 -m unittest discover -s tests/contract -p 'test_*.py' -v` | 52 passed | Contract harness tests; not full upstream surface parity. |
| `./scripts/check-acp-schema` | passed | ACP schema only; excluded from Pi denominator. |
| Rust RPC tests | passed | Internal Rust behavior only; not upstream differential evidence. |

## Upstream public-surface inventory

Paths below are upstream source paths relative to pinned checkout. Rust paths
identify current implementation where present. Status describes current Rust
coverage, not planned work.

| Surface | Upstream path | Rust path | Status |
| --- | --- | --- | --- |
| CLI args and parsing | `packages/coding-agent/src/cli/args.ts`; `src/index.ts` | `crates/pi-coding-agent/src/main.rs` | partial |
| Config paths/version | `packages/coding-agent/src/config.ts`; `src/index.ts` | `crates/pi-coding-agent/src/file_config.rs` | partial |
| AgentSession and events | `packages/coding-agent/src/core/agent-session.ts`; `src/index.ts` | `crates/pi-coding-agent/src/session.rs` | partial |
| Auth storage | `packages/coding-agent/src/core/auth-storage.ts`; `src/index.ts` | `crates/pi-coding-agent/src/config.rs` | candidate |
| Compaction API | `packages/coding-agent/src/core/compaction/index.ts`; `src/index.ts` | `crates/pi-coding-agent/src/session.rs` | partial |
| Event bus | `packages/coding-agent/src/core/event-bus.ts`; `src/index.ts` | `crates/pi-coding-agent/src/` | candidate |
| Extension types/runtime | `packages/coding-agent/src/core/extensions/index.ts`; `src/index.ts` | `crates/pi-coding-agent/src/extension.rs`, `extension_host.rs` | partial |
| Footer data provider | `packages/coding-agent/src/core/footer-data-provider.ts`; `src/index.ts` | `crates/pi-coding-agent/src/tui.rs` | candidate |
| Message conversion | `packages/coding-agent/src/core/messages.ts`; `src/index.ts` | `crates/pi-coding-agent/src/` | candidate |
| Model registry | `packages/coding-agent/src/core/model-registry.ts`; `src/index.ts` | `crates/pi-ai/src/` | partial |
| Model resolver/runtime | `packages/coding-agent/src/core/model-resolver.ts`, `model-runtime.ts`; `src/index.ts` | `crates/pi-ai/src/`, `crates/pi-coding-agent/src/config.rs` | partial |
| Package manager | `packages/coding-agent/src/core/package-manager.ts`; `src/index.ts` | `crates/pi-coding-agent/src/resources.rs` | candidate |
| Resource loader/context | `packages/coding-agent/src/core/resource-loader.ts`; `src/index.ts` | `crates/pi-coding-agent/src/resources.rs`, `system_prompt.rs` | partial |
| Session manager/tree | `packages/coding-agent/src/core/session-manager.ts`; `src/index.ts` | `crates/pi-coding-agent/src/session.rs` | partial |
| Settings manager | `packages/coding-agent/src/core/settings-manager.ts`; `src/index.ts` | `crates/pi-coding-agent/src/file_config.rs` | partial |
| Skills | `packages/coding-agent/src/core/skills.ts`; `src/index.ts` | `crates/pi-coding-agent/src/resources.rs` | candidate |
| Edit diff | `packages/coding-agent/src/core/tools/edit-diff.ts`; `src/index.ts` | `crates/pi-coding-agent/src/` | candidate |
| Built-in tools: bash/edit/find/grep/ls/read/write | `packages/coding-agent/src/core/tools/*`; `src/index.ts` | `crates/pi-agent/src/tools/*` | partial |
| SDK session factories | `packages/coding-agent/src/core/sdk.ts`; `src/index.ts` | `crates/pi-coding-agent/src/` | candidate |
| SDK tool factories | `packages/coding-agent/src/core/sdk.ts`; `src/index.ts` | `crates/pi-agent/src/tools/*` | partial |
| Interactive terminal/TUI | `packages/coding-agent/src/modes/interactive/*` | `crates/pi-coding-agent/src/tui.rs` | partial |
| Pi JSONL RPC | `packages/coding-agent/src/modes/rpc/rpc-types.ts` | `crates/pi-coding-agent/src/rpc/types.rs`, `rpc/server.rs` | partial (11/32 recognized commands; thinking controls source-attested subset, `max` collapsed to `xhigh`, no upstream capture) |

## Milestone catalog

| ID | Surface | Status | Evidence/target |
| --- | --- | --- | --- |
| `cli.print` | `--print` headless execution; model routing `gemini-2.0-flash` → Google Generative AI | supported | `cli.print.basic` |
| `agent.serial-tools` | serial tool loop | supported | `agent.serial-tool-loop` |
| `tool.read` | bounded read | supported | `tool.read.bounds` |
| `tool.bash` | process execution/cancellation | supported | `tool.bash.cancel-descendants` |
| `session.native-write` | native session append/recovery | supported | `session.native-append-recover` |
| `session.pi-import` | explicit read-only Pi import | read-only | `session.pi-import-checksum` |
| `session.pi-cow` | copy-on-write imported session | supported | `session.pi-cow-provenance` |
| `extension.none-required` | operation without Node | supported | `extension.node-absent` |
| `protocol.pi-jsonl-rpc` | Pi RPC command surface | partial | Rust recognizes 11 of upstream 32; thinking controls are source-attested only, `max` collapses to `xhigh`, and no upstream capture exists |
| `extension.discovery` | discover project/global/explicit `.ts`, `.js`, `package.json` | partial | `crates/pi-coding-agent/src/extension.rs` |
| `extension.execution` | execute discovered dynamic extensions | unsupported | diagnostic says runtime deferred; no agent-loop hook |
| `protocol.acp-v1` | external ACP v1 JSON-RPC stdio | partial (active mode) | envelope validation and initialize only; sessions deferred; `fixtures/acp/v1/schema.json` |

## ACP appendix (external; excluded from denominator)

`protocol.acp-v1` is official ACP v1 JSON-RPC 2.0 over stdio, separate from
Pi JSONL RPC. Current `pi-acp` provides generic envelope/transport groundwork;
no coding-agent ACP mode, schema-derived semantic codec, callbacks, or
conformance harness exists. ACP must not affect Pi parity status.

## Node appendix (separate; excluded from denominator)

`pi-rs` resolves and verifies Node >=18 for trusted sidecar work, with canonical
root checks and handshake IPC. Discovery exists across project/global/explicit
paths. Dynamic TypeScript/JavaScript execution remains unsupported and is not
hooked into agent tool registration. Bun support, full extension API channels,
UI, cancellation, and packaging remain deferred/unsupported.
