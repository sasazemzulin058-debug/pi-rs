# Project Status

Updated: 2026-08-10

## Target

- Upstream: `@earendil-works/pi-coding-agent@0.83.0`
- Commit: `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`
- Oracle lock SHA-256: `cc00fd4bfdc43f30ac7892de8dd46e5108c8b966b4941683bac42ee0587771d7`
- Branch: `feature/rb1-fail-closed`

## Status

Full upstream Pi parity is **not complete**.

Current evidence proves local Rust behavior and selected source-attested slices. It does not prove full upstream parity. Do not use test count as parity percentage.

## Verified local gates

```text
cargo test --locked --workspace --all-targets       PASS
cargo clippy --locked --workspace --all-targets -- -D warnings  PASS
sh ./scripts/verify-termux                            PASS 13/13
python3 scripts/validate-fixture-manifest            PASS
./scripts/check-acp-schema                            PASS
```

Hosted GitHub operations remain parent-owned. Local Termux is authoritative device evidence.

## Pi port

| Surface | Current state |
|---|---|
| Session v3/tree/import/COW | implemented; selected fixtures pass |
| Agent queues/cancel/reuse | implemented; focused tests pass |
| Providers | OpenAI loopback; Anthropic malformed/truncated SSE fail-closed tests; full provider parity absent |
| Tools read/bash | local M1a fixtures pass |
| Tool edit | multi-edit, overlap rejection, all-or-nothing, BOM/CRLF; upstream expected artifact absent; not captured |
| Resources/trust | implemented subset; focused tests pass; full package/resource parity absent |
| Pi JSONL RPC | partial; 11 recognized of 32 upstream command variants; source-attested queue/thinking slices; upstream capture absent |
| JSON events | local fixture checks; production CLI differential fixture absent |
| TUI/interactive | reducer/input/terminal safety subset; full upstream behavior absent; PTY evidence absent |
| Extensions | trusted discovery plus Node host handshake and narrow JS `tool_call` hook; no full manager/agent-loop integration |
| Node Termux runtime | explicit resolution/install/check groundwork; no bundled sidecar artifact |
| Full Pi catalog | planned; no full executable denominator gate |

## External ACP track

ACP is not part of Pi upstream parity denominator.

Current state:

- official schema vendored and SHA checked;
- bounded JSON-RPC/NDJSON transport;
- `--mode acp` initialization-only mode;
- malformed input and initialization CLI tests.

Absent:

- `session/new`, `session/prompt`, `session/cancel`, `session/update`;
- client filesystem/terminal/permission callbacks;
- schema-generated complete types;
- cross-language vectors and conformance gate.

## OMP

OMP is not being rewritten in Rust. OMP compatibility remains a separate optional Node/extension track.

## Remaining blockers

1. Capture upstream fixtures for uncaptured Pi surfaces.
2. Build executable Pi catalog and strict comparator denominator.
3. Complete RPC commands and response/event shapes.
4. Integrate Node extension manager into agent loop.
5. Finish provider, tools, resources, session, and TUI differential slices.
6. Implement ACP session/callback/conformance track if ACP remains required.
7. Run fresh reviews after each lane.
8. Run final local and GitHub gates from same source SHA.

Canonical plan: `docs/plans/full-parity-roadmap.md`.
Compatibility table: `docs/compatibility-matrix.md`.
