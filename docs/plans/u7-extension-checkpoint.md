# U7 Extension Checkpoint

## Status

IN_PROGRESS — U7 Node host protocol fixes applied; execution remains hook-only and no agent manager wiring.

## Current change

`extension_host.rs` now performs handshake before extension load, uses bounded incremental frames with finite response timeout and kill/reap on startup failure, converts paths with `pathToFileURL`, and tests mutation hook roundtrip. `extension.rs` now enforces trusted project directory equality with `CanonicalProjectRoot`, resolves trusted explicit files and one-level package directories, validates malformed/invalid/missing manifest entries, rejects uppercase executable suffixes, reports missing trust roots, tests global lexical ordering and canonical symlink containment, and removes `extension.json` heuristic. Project discovery remains trust-gated; global discovery remains canonical-root-gated; candidate canonicalization and first-wins dedup remain fail-closed.

## Validation

- Tests cover trusted-root mismatch, missing extension root, directory symlink diagnostics, no `extension.json` heuristic, malformed/invalid/missing manifest entries, uppercase suffixes, explicit file/package resolution, missing trust root, global lexical ordering and symlink containment, and first-wins dedup.
- Validation: `rustfmt --edition 2021 crates/pi-coding-agent/src/extension.rs` passed; `git diff --check -- crates/pi-coding-agent/src/extension.rs docs/plans/u7-extension-checkpoint.md` passed. Focused Cargo test was attempted but blocked by unrelated dirty-lane compile errors in `crates/pi-coding-agent/tests/resource_loader.rs` (`linked_worktree_fail_open_matrix`) and `crates/pi-coding-agent/src/interactive.rs` (`handle` not mutable). Full `cargo fmt --all -- --check`, clippy, and doc consistency remain not-run because workspace is already fmt-dirty outside U7.
- Focused Node host test passed when Node runtime available; full workspace validation remains pending.
- No agent manager/tool registry wiring claim. No GitHub operations or commits performed by worker.

## Scope

U7 files: `crates/pi-coding-agent/src/extension.rs`, `crates/pi-coding-agent/src/extension_host.rs`, `crates/pi-coding-agent/node/extension-host.mjs`, and this checkpoint. Runtime host remains trusted hook boundary only.
