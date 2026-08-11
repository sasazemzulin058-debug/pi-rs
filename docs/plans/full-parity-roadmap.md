# Full Parity Roadmap

## Frozen denominator and boundaries

Target is Pi-only upstream `pi-mono` 0.83.0, pinned to commit
`f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`. Denominator covers public
coding-agent exports and Pi JSONL RPC, not ACP or Node. Upstream source checkout:
`/data/data/com.termux/files/home/pi-mono-oracle-083`.

ACP v1 is external standard. It stays separate appendix and never denominator.
Node runtime and extension sidecar stay separate implementation track and never
denominator.

Status vocabulary: **supported**, **partial**, **candidate**, **read-only**,
**unsupported**, **deferred**. Use only these terms in parity claims.

## Current evidence boundary

- Committed comparator command
  `python3 scripts/compare-contract-fixtures --milestone M1a --actual fixtures/upstream-pi`
  reports **0 passed / 41 failures/errors**. This is invalid actual corpus evidence:
  committed actual files are incomplete/stale and mismatched.
- `sh ./scripts/verify-termux` passes locally using transient generated outputs;
  this does not make committed corpus valid or prove hosted attestation.
- `python3 scripts/validate-fixture-manifest`,
  `python3 -m unittest discover -s tests/contract -p 'test_*.py' -v`,
  `./scripts/check-doc-consistency`, and `./scripts/check-acp-schema` pass.
- Rust RPC tests pass internal checks only. Upstream defines 36 RPC commands;
  Rust recognizes 11 (`prompt`, `steer`, `follow_up`, `abort`, `new_session`,
  `get_state`, `set_steering_mode`, `set_follow_up_mode`,
  `set_thinking_level`, `cycle_thinking_level`,
  `get_available_thinking_levels`) in `crates/pi-coding-agent/src/rpc/types.rs`.
  Thinking controls are source-attested only; `max` collapses to `xhigh`, and no
  upstream capture establishes parity.

Complete public-surface inventory, upstream paths, Rust paths, and statuses live
in `docs/compatibility-matrix.md`.

## Execution tracks

### Track A — Pi core and public API denominator

1. Freeze manifest/catalog IDs against pinned upstream.
2. Add executable catalog and denominator gate. Bind docs, manifest, generators,
   comparators, and CI without counting ACP or Node.
3. Capture fixtures for public SDK factories/types, model registry/runtime,
   auth/settings storage, compaction, event bus, session manager, package
   manager, resources/skills, tools, interactive mode, export/switch/fork/clone.
4. Replace stale/incomplete committed actual corpus with reproducible generated
   evidence and exact command/output boundary.

### Track B — Pi JSONL RPC

1. Treat upstream `packages/coding-agent/src/modes/rpc/rpc-types.ts` as 36-command
   denominator.
2. Expand Rust RPC beyond current exact 11/32 recognized subset: model controls,
   full thinking-level semantics, compaction/retry, bash, session
   tree/export/fork/clone, messages, commands, and extension UI request/response
   shapes. Current thinking controls remain source-attested only, with `max`
   collapsed to `xhigh`.
3. Add upstream contract fixtures before marking commands supported. Internal Rust
   tests do not establish upstream compatibility.

### Track C — Resources, tools, sessions, providers, TUI

Complete deferred/partial rows in matrix with upstream fixture evidence. Preserve
security boundaries for trust and canonical paths, error handling, cancellation,
atomic writes, and session recovery.

### Track D — Extension discovery and execution (Node separate)

1. Keep discovery status separate: project/global/explicit `.ts`, `.js`, and
   `package.json` candidates are discovered and diagnostics are emitted.
2. Keep execution status separate: dynamic JS/TS runtime currently returns
   `UnsupportedExtension` because no execution hook reaches agent tool registry.
3. Later add versioned Node sidecar ABI, trusted canonical-root checks, handshake,
   capabilities, registration, hooks, cancellation, UI, reproducible packaging.
4. Bun/private imports/native addons/custom TUI remain unsupported unless explicit
   compatibility contract added.

### Track E — ACP appendix (external, separate)

1. Keep vendored ACP schema/provenance and `./scripts/check-acp-schema`.
2. Add schema-derived Rust types and strict codec.
3. Add `--mode acp`, initialization, lifecycle, streaming, callbacks, and
   conformance harness. None count toward Pi denominator.

## Release gates

Release requires pinned upstream identity, executable Pi catalog/denominator,
reproducible committed actual corpus, upstream fixtures for claimed supported
surfaces, exact RPC accounting, and passing allowed documentation/manifest/
contract checks. ACP and Node results report in separate appendices.

Preserve checker paths:
- `./scripts/check-doc-consistency`
- `python3 scripts/validate-fixture-manifest`
- `python3 -m unittest discover -s tests/contract -p 'test_*.py' -v`
- `./scripts/check-acp-schema`
- `git diff --check`

No parity release claim may rely on direct comparator result while it reports
0/41 failures/errors.
