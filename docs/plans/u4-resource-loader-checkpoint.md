# U4 Resource Loader Checkpoint

Status: BLOCKED
Updated: 2026-08-10

## Narrow U4 Changed Files
- `crates/pi-coding-agent/src/project.rs` - Upward `.git` search, strict `gitdir: <path>` parsing, common-git identity verification, nested linked-worktree shadow handling.
- `crates/pi-coding-agent/src/system_prompt.rs` - Nested path-based private module registration (`#[path = "resources.rs"] mod resources;`).
- `crates/pi-coding-agent/src/resources.rs` - Memory-only resource catalog, component-aware ancestry, and metadata resolution.
- `crates/pi-coding-agent/tests/resource_loader.rs` - Focused regression tests for prompt sources, metadata fields/kinds, stale metadata reset, component boundaries, longest ancestor selection, nested roots, and linked-worktree fail-open behavior.
- `docs/plans/u4-resource-loader-checkpoint.md` - Narrow U4 verification checkpoint.

`crates/pi-coding-agent/src/main.rs` was not modified for U4. No GitHub action, commit, or push performed.

## Validation Results

| Command | Result | Summary |
|---|---|---|
| `cargo fmt --all -- --check` | PASSED (0) | Workspace formatting clean. |
| `cargo check --locked -p pi-coding-agent` | BLOCKED (101) | Blocked by parallel U6 dirty state (`tui.rs` missing `AgentEvent::PhaseChange` / `Settlement` match arms). |
| `cargo test --locked -p pi-coding-agent --test resource_loader` | BLOCKED (101) | Attempted after fix2; parallel `interactive.rs:202` compile error (`handle` not mutable) prevented test execution. Includes different-filename retention and six fail-open fixtures by source inspection. |
| `cargo test --locked -p pi-coding-agent --test resource_trust` | NOT RUN | Bounded run not reached after resource_loader compile block; prior run passed (10/10 tests). |
| `cargo test --locked --workspace` | BLOCKED (101) | Blocked by parallel U6 dirty state. |
| `sh ./scripts/verify-termux` | PASSED (0) | Termux verification passed, 13/13 invariants matched. |

## Scope & Parity Notes

- Full upstream 0.83 parity remains unproven; shared parity status remains 0%.
- Resource catalog remains staging seam only; no package parsing, runtime discovery, extension execution, or CLI wiring added.
- Current focused coverage:
  - Truly nested worktree fixture inside main repository tree asserts worktree `AGENTS.md` retention, main repo `AGENTS.md` shadowing, main repo `CLAUDE.md` retention, and nested `.pi/instructions.md` loading.
  - `linked_worktree_fail_open_matrix` asserts local and ancestor context retention for ordinary repository, sibling worktree, bare-like layout, missing gitdir, invalid `commondir`, and malformed gitfile; existing nested test also covers absent worktree context and submodule-like layout.
  - Metadata boundary test proves component-aware `Path::starts_with` (`/pkg/foobar/deep.js` does NOT match `/pkg/foo`) and longest-ancestor selection (`/pkg/foo/child` wins over `/pkg/foo` for descendant).
  - Metadata matrix asserts all fields (`source`, `scope`, `origin`, `base_dir`) across all three kinds (`Skill`, `Prompt`, `Theme`), and proves stale extension metadata is cleared on `reload()`.

