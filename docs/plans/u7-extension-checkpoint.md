# U7 Extension Architecture & Fail-Closed Checkpoint

## Status: Design & Diagnostics (Fail-Closed)

### Context & Goal

Upstream Pi supports loading TypeScript/JavaScript extensions via Node.js/jiti dynamic runtime loading. In `pi-rs`, runtime dynamic execution of TS/JS extensions via Node/jiti sidecars is **deferred**.

This slice establishes:

1. **ADR**: Design decision documenting deferred upstream TS/jiti runtime sidecars and outlining safe Tier-A host capability boundary.
2. **Fail-closed Unsupported Diagnostic**: Explicit helper for caller-supplied extension paths/manifests (`ExtensionDiagnostic`), failing with stable, control-character sanitized diagnostics (`UnsupportedExtension`) rather than silent misbehavior or unhandled dynamic loading.

### ADR: Extension Runtime Model (Deferred Node/jiti Sidecars)

- **Decision**: Rust `pi-rs` core does NOT spawn Node.js/jiti sub-processes or execute untrusted JS/TS extension code directly in U7.
- **Upstream compatibility**: Upstream Pi uses Node.js host environments to dynamically require `.ts`/`.js` modules. `pi-rs` targets single-binary performance and strict resource boundaries.
- **Fail-Closed Strategy**:
  - `ExtensionDiagnostic` provides explicit `check_extension_path` validation for caller-supplied extension candidate paths, returning `UnsupportedExtension` with sanitized paths.
  - Core agent runtime remains fully functional when extensions are absent or disabled.
  - Safe discovery and parsing of metadata (frontmatter/manifests) without execution may be enabled for passive registration inspection in future milestones.

### Included in Slice

- `docs/plans/u7-extension-checkpoint.md` (this checkpoint / ADR).
- `ExtensionDiagnostic` helper module in `pi-coding-agent` (`crates/pi-coding-agent/src/extension.rs`), unexposed until CLI/config extension flags exist.
- Unit and boundary tests verifying sanitized path diagnostics for explicit extension candidate check calls.

### Excluded (Future Slices / Deferred)

- No Node.js subprocess IPC / JSON-RPC sidecar protocol execution.
- No dynamic TS runtime integration (jiti, Bun, Deno).
- No Tier-A tool/hook execution sidecars.
- No Oh My Pi API implementation.

### Validation

- Unit tests for unsupported extension error reporting and path sanitization.
- `cargo check --workspace` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `scripts/check-doc-consistency` passes.
