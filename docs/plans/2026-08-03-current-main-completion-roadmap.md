# Implementation Plan

## Goal

Завершить существующий `main` репозитория `/data/data/com.termux/files/home/pi-rs-main-audit` от baseline `fb9be6796b9e1d99bd0fd79cf24e0d754d163541`: сначала закрыть security/release blockers, затем сделать M1a compatibility и delivery gates воспроизводимыми, после чего по отдельным fixture-gated milestones расширять совместимость с upstream без переписывания уже работающих crates, tests и CI с нуля.

## 1. Baseline, provenance и уже выполненная работа

### 1.1 Authoritative baseline

- **Repository:** `/data/data/com.termux/files/home/pi-rs-main-audit`
- **Baseline commit:** `fb9be6796b9e1d99bd0fd79cf24e0d754d163541`
- **Workspace version:** `1.2.0`
- **Rust MSRV:** `1.80`
- **Workspace crates:**
  - `crates/pi-ai`
  - `crates/pi-agent`
  - `crates/pi-coding-agent`
- **M1a fixture oracle currently pinned in manifest:**
  - package `@earendil-works/pi-coding-agent`
  - version `0.82.1`
  - commit `2efa728d2ee90ef597626e96b1e28ef2b279f07c`
  - lockfile SHA-256 `472f0726dc79f3b38df58d8a8bce96bf56fbf993a134b49aabc54947b8461e59`
  - evidence: `fixtures/upstream-pi/manifest.json`
- **Gap-analysis reference:** local `pi-mono` commit `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`, used only to identify newer upstream surfaces. It must not silently replace the fixture oracle commit. Updating the fixture oracle is a separate reviewed migration.
- Старые планы и аудиты для `jshachm/pi-rs` не являются входом этого roadmap.

### 1.2 Already implemented — do not rebuild

| Surface | Current evidence | Current status |
|---|---|---|
| Workspace and package structure | `Cargo.toml`; three workspace members at version `1.2.0` | **implemented** |
| Anthropic, OpenAI Chat, OpenAI Responses and Google provider modules | `crates/pi-ai/src/providers/` | **partial; provider modules exist, CLI routing/parity incomplete** |
| SSE streaming and retry infrastructure | `crates/pi-ai/src/providers/openai.rs`, `crates/pi-ai/src/retry.rs` | **partial** |
| Serial agent/tool loop | `crates/pi-agent/src/agent_loop.rs` | **implemented serially; queues/events parity unsupported** |
| Built-in tools | `crates/pi-agent/src/tools/` including `bash`, `read`, `edit`, `write`, `grep`, `ls`, glob, web fetch and todo | **implemented subset; exact upstream behavior partial** |
| Permission policy and `--yolo` | `crates/pi-coding-agent/src/permission.rs`, `src/main.rs` | **implemented but release-blocking fail-open defect exists** |
| Print mode and JSON-lines mode | `crates/pi-coding-agent/src/print_mode.rs`, `tests/json_mode.rs` | **implemented; compatibility candidate** |
| Basic interactive REPL and slash commands | `crates/pi-coding-agent/src/interactive.rs` | **implemented basic REPL; full TUI unsupported** |
| Active session save/load/list/resume | `crates/pi-coding-agent/src/session.rs`, `src/main.rs`, `src/interactive.rs` | **implemented as pretty single JSON; claimed v3 JSONL is not active** |
| Pi import/checksum/COW seams | `crates/pi-coding-agent/src/session.rs`, `tests/session_pi_import.rs` | **partially implemented but user-facing wiring absent** |
| Context and basic trust model | `crates/pi-coding-agent/src/project.rs`, `src/trust.rs`, `src/system_prompt.rs` | **partial** |
| Termux helper seams | `crates/pi-coding-agent/src/termux.rs`; unit tests in `src/main.rs` | **partial; bash resolver and authoritative CI gate incomplete** |
| CI matrices | `.github/workflows/ci.yml` runs Ubuntu/macOS on stable and Rust 1.80 | **implemented but not sufficient as a release gate** |
| Contract harness | `scripts/validate-fixture-manifest`, `scripts/compare-contract-fixtures`, `tests/contract/` | **implemented harness; M1a product attestation incomplete** |
| Supply-chain checks | `.github/workflows/ci.yml`, `deny.toml` | **implemented baseline; policy/action pinning incomplete** |
| Release binary matrix | `.github/workflows/release.yml` | **implemented build/upload path; provenance and exact-commit gates absent** |
| Documentation book | `docs/src/`, `.github/workflows/docs.yml` | **implemented; claims contain path/version/session inconsistencies** |

### 1.3 Existing test evidence

Fresh reports record:

- One baseline `cargo test` run passing **58 Rust tests** across the three crates.
- `python3 -m unittest discover -s tests/contract` passing **26 contract-harness tests**.
- Global `./scripts/validate-fixture-manifest` passing structural validation.
- `./scripts/validate-fixture-manifest --milestone M1a` failing because all **13 M1a cases** remain `captured: false`.
- A later `cargo test --workspace --all-targets` attempt failing before test execution because local Termux target artifacts were zero-length/missing. This is not a source-test failure, but means a clean-target all-targets result still needs to be produced.

Therefore:

- Existing unit/integration tests must be retained and extended, not recreated.
- Passing Python harness tests prove the harness implementation only.
- No runtime compatibility status may become `supported` solely from these counts.
- Full parity or “everything is supported” must not be claimed without committed expected/actual fixtures and a passing comparator.

## 2. Compatibility matrix policy

Use these statuses consistently in `docs/compatibility-matrix.md`, the fixture manifest, README and release notes:

- **supported:** required fixture/invariant passes on every declared target and is enforced by required CI.
- **partial:** a useful implementation exists, but one or more declared behaviors, platforms or fixture cases are missing.
- **candidate:** implementation is believed ready for fixture comparison but has not passed the required oracle.
- **read-only:** a user-facing input surface is wired and tests prove the source is never mutated.
- **unsupported:** detectable use returns a stable diagnostic and does not silently degrade.
- **deferred:** explicitly excluded from the current release train; no compatibility claim.
- **unattested:** useful internal description for reports; public matrix should normally use `candidate` or `partial`.

Policy rules:

1. Status is per surface and per target, not per crate.
2. Routing tests alone do not establish provider response parity.
3. Linux evidence does not establish Termux process/shell behavior.
4. Unit tests do not substitute for cross-language fixtures where a Pi-compatible wire contract is claimed.
5. A status can be promoted only in the same change that commits:
   - expected fixture or invariant specification;
   - actual-output generator;
   - comparator rule;
   - required CI invocation.
6. Oracle commit changes require a separate reviewed fixture migration with before/after differences.
7. Unsupported/deferred rows remain visible; they must not disappear from the matrix to make a milestone appear complete.

---

## 3. Release blockers first

### Milestone RB1 — Fail-closed EOF permissions

- **Severity:** BLOCKER
- **Current status:** unsafe partial
- **User scenario:** A user runs `pi-rs -p ... </dev/null` without `--yolo`; a provider requests `write`, `edit` or `bash`. No destructive tool may execute without an explicit affirmative answer.
- **Current files:**
  - `crates/pi-coding-agent/src/permission.rs`
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-coding-agent/src/print_mode.rs`
  - `crates/pi-agent/src/agent_loop.rs`
- **Exact changes:**
  - In `permission.rs`, treat `read_line` `Ok(0)`, read errors, whitespace-only input and unknown answers as `PermissionDecision::Deny`.
  - Remove the empty string from affirmative responses.
  - Accept only explicit `y`/`yes` for one call and `a`/documented allow-session aliases for session scope.
  - Keep `--yolo` as the only blanket noninteractive allow path.
  - Introduce a narrow injectable reader/writer seam so EOF and read errors can be tested without a production environment backdoor.
  - Ensure print/JSON mode emits `PermissionDenied` and never emits `ToolExecutionStart` for the denied call.
- **Dependencies:** none; first implementation task.
- **Tests:**
  - Extend unit tests in `crates/pi-coding-agent/src/permission.rs`.
  - Add `crates/pi-coding-agent/tests/permission_cli.rs`.
  - Fake provider requests `write` and `bash`; closed input must leave sentinel files absent.
  - Cover `yes`, allow-session, denial, blank input, EOF, injected I/O error and `--yolo`.
- **Exit gate:**
  - EOF and prompt failures always deny.
  - No filesystem or process side effect occurs after denial.
  - Regression test passes in Linux, macOS and authoritative Termux jobs.
- **Non-goals:**
  - Per-path sandboxing.
  - A new policy language.
  - Implicit allow because stdin is not a TTY.
- **Risks:**
  - A test-only provider injection must not become an undocumented production bypass.
  - Machine-readable stdout must remain clean; prompt diagnostics belong on stderr.

### Milestone RB2 — Session ID containment

- **Severity:** HIGH
- **Current status:** unsafe partial
- **User scenario:** A user supplies an ID to `sessions show`, `sessions delete`, `--resume` or `/resume`; it must be impossible to read or delete a file outside the configured sessions directory.
- **Current files:**
  - `crates/pi-coding-agent/src/session.rs`
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-coding-agent/src/interactive.rs`
  - `crates/pi-coding-agent/tests/session_smoke.rs`
- **Exact changes:**
  - Add one canonical `SessionId` parser/newtype matching the generated UUID-like ID format.
  - Reject empty IDs, `.`, `..`, separators, absolute paths, control characters, noncanonical encodings and excessive length.
  - Add a single session path resolver used by save/load/show/delete/resume/list.
  - Replace `main.rs` direct `join(format!("{id}.json"))` deletion with `session::delete`.
  - Validate `Session.id` before save, so imported/deserialized objects cannot bypass the boundary.
  - Canonicalize the sessions root and existing target; reject symlink targets escaping the root.
  - Document and test whether a symlink itself is rejected or may be unlinked without following it; prefer rejection for all session operations.
- **Dependencies:** precedes RB3 session storage changes.
- **Tests:**
  - Extend `session_smoke.rs` with a table of valid/invalid IDs.
  - Add `crates/pi-coding-agent/tests/session_cli_security.rs`.
  - Exercise relative traversal, absolute paths, backslash variants, symlink escape and normal show/delete/resume.
  - Verify external sentinel files remain byte-for-byte unchanged.
- **Exit gate:**
  - No user-supplied ID is joined outside the resolver.
  - Show/delete/CLI resume/interactive resume reject traversal consistently.
  - External files cannot be read, truncated or deleted.
- **Non-goals:**
  - Arbitrary user filenames as session IDs.
  - General tool-path sandboxing.
- **Risks:**
  - Canonicalize-then-open leaves a TOCTOU window. If the threat model requires a stronger boundary, use directory-relative no-follow opens rather than weakening the claim.

### Milestone RB3 — Truthful active session format and wiring

- **Severity:** HIGH
- **Current status:** active single JSON is partial; v3 JSONL claim is unsupported
- **User scenario:** A user saves, lists, resumes and migrates sessions without being misled about location, format, durability or Pi compatibility.
- **Current files:**
  - `crates/pi-coding-agent/src/session.rs`
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-coding-agent/src/interactive.rs`
  - `crates/pi-coding-agent/tests/session_smoke.rs`
  - `crates/pi-coding-agent/tests/session_pi_import.rs`
  - `docs/compatibility-matrix.md`
  - `docs/src/cli/sessions.md`
  - `README.md`
  - `fixtures/upstream-pi/manifest.json`
- **Exact changes:**
  - First record the current truth: active persistence is `$XDG_CONFIG_HOME/pi-rs/sessions/<id>.json`, not `~/.pi-rs`, not `$XDG_CONFIG_HOME/pi`, and not active JSONL.
  - Select one release contract:
    1. preferred: wire a real append/recover versioned JSONL store compatible with the declared v3 fixture; or
    2. short-term fallback: keep atomic single JSON, rename the M1a claim accordingly and leave v3 as unsupported.
  - Because `session.native-append-recover` is already required for M1a, option 1 is the intended completion path.
  - Split `session.rs` into schema/store/context modules only where needed; do not rewrite unrelated CLI logic.
  - Write the header once, append entries, preserve previous complete records, reject malformed middle records and recover only an incomplete final record.
  - Define locking, flush/fsync, permissions and concurrent-writer behavior.
  - Wire list/show/delete/resume/interactive persistence to the same store.
  - Expose Pi import through a documented CLI/API path. Until then, downgrade `session.pi-import` from `read-only`.
  - On first mutation of an imported source, create a new native session with source checksum and provenance; never mutate the source.
- **Dependencies:** RB2; fixture completion D1.
- **Tests:**
  - Header/version/schema and append-without-rewrite tests.
  - Entry IDs and parent chain tests.
  - Interrupted final-tail recovery and malformed-middle hard error.
  - Concurrent writer lock test.
  - Unix permission test.
  - CLI list/show/resume/delete against the active format.
  - Pi v1/v2/v3 read-only import, checksum mismatch and COW source immutability.
  - If Pi v3 compatibility is claimed, parse Rust output with the pinned upstream parser.
- **Exit gate:**
  - CLI actually uses the documented format and directory.
  - `session.native-append-recover`, `session.pi-import-checksum` and `session.pi-cow-provenance` actual outputs pass their committed fixtures.
  - No documentation calls the format v3-compatible before the upstream parser test passes.
- **Non-goals:**
  - In-place mutation of original Pi files.
  - Distributed locking.
  - Supporting every historical unversioned format.
- **Risks:**
  - Session format is a migration boundary; a silent in-place conversion can destroy recoverability.
  - Cross-platform locking/fsync semantics must be described per target.

### Milestone RB4 — Explicit provider/model routing

- **Severity:** HIGH
- **Current status:** provider implementations partial; CLI routing incorrect for documented Gemini
- **User scenario:** `PI_MODEL=gemini-2.0-flash pi-rs -p ...` selects Google, and an unknown explicit model fails rather than silently selecting Anthropic.
- **Current files:**
  - `crates/pi-coding-agent/src/config.rs`
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-ai/src/lib.rs`
  - `crates/pi-ai/src/types.rs`
  - `crates/pi-ai/src/providers/google.rs`
  - `crates/pi-ai/src/providers/openai_responses.rs`
  - `README.md`
- **Exact changes:**
  - Replace `default_model_from_env() -> Model` silent fallback with a fallible resolver.
  - Separate explicit CLI/env/file selection from key-based default selection.
  - Resolve every documented alias to a provider and API:
    - Anthropic aliases to Anthropic Messages;
    - `gpt-4o` and `gpt-4o-mini` to OpenAI Chat;
    - documented Responses models to OpenAI Responses;
    - `gemini-2.0-flash` to Google Generative AI.
  - Recognize documented Google key variables and define deterministic key precedence.
  - Reject an unknown explicit model with a stable nonzero diagnostic.
  - Test request construction through loopback endpoints without real credentials or external network.
- **Dependencies:** independent of RB2/RB3; required before final CLI fixtures.
- **Tests:**
  - Add `crates/pi-coding-agent/tests/model_routing.rs`.
  - Add `crates/pi-ai/tests/provider_routing_loopback.rs`.
  - Assert endpoint, HTTP method, auth source and essential request fields.
  - Cover CLI > env > file config precedence, unknown IDs, no keys and multiple keys.
- **Exit gate:**
  - Every documented model reaches the expected dispatcher.
  - Unknown explicit models never fall back.
  - Routing tests are network-free and secret-free.
- **Non-goals:**
  - Dynamic support for every provider catalog model.
  - Live-provider golden tests.
- **Risks:**
  - Routing proves request selection, not response parity; provider rows remain `partial` or `candidate` until stream fixtures pass.
  - Google query credentials must not appear in diagnostics.

### Milestone RB5 — Persistent allow-session semantics

- **Severity:** MEDIUM, release-required
- **Current status:** broken across interactive turns
- **User scenario:** A user selects allow-session for `bash` once; later turns in the same interactive session do not prompt for `bash`, while another tool still prompts.
- **Current files:**
  - `crates/pi-coding-agent/src/permission.rs`
  - `crates/pi-coding-agent/src/interactive.rs`
  - `crates/pi-agent/src/agent_loop.rs`
- **Exact changes:**
  - Insert the selected tool name into `CliPermission.allowed_session`.
  - Remove the clone-and-merge logic that currently merges only the preexisting snapshot.
  - Keep the `pi-agent` per-run set as an optimization, not the sole session state.
  - Define reset semantics. Recommended: `/reset` starts a new permission session and clears cached allowances.
  - Keep cache process-local and nonpersistent.
- **Dependencies:** RB1 permission seam.
- **Tests:**
  - Add `crates/pi-coding-agent/tests/permission_session.rs`.
  - Two turns using one policy instance.
  - Different tool, ordinary one-shot allow, denial, reset and concurrent checks.
- **Exit gate:**
  - Allow-session survives separate `run_agent_with_history` calls in one session.
  - It does not survive reset/new process.
  - EOF remains deny regardless of cache history for uncached tools.
- **Non-goals:**
  - Persistent permissions on disk.
  - Argument-sensitive rules.
- **Risks:**
  - Reset and concurrent checks must not deadlock or lose inserts.

### Milestone RB6 — Malformed SSE and tool arguments fail closed

- **Severity:** MEDIUM, release-required
- **Current status:** unsafe partial
- **User scenario:** An OpenAI-compatible server emits malformed SSE JSON or malformed assembled tool arguments; the agent reports a provider error and does not run a tool with substituted `{}` arguments.
- **Current files:**
  - `crates/pi-ai/src/providers/openai.rs`
  - `crates/pi-ai/src/error.rs`
  - `crates/pi-ai/tests/openai_sse_loopback.rs`
  - `crates/pi-agent/src/agent_loop.rs`
- **Exact changes:**
  - Treat malformed nonempty SSE `data` other than `[DONE]` as `InvalidResponse`; remove silent `continue`.
  - Preserve support for arbitrary TCP fragmentation handled by the SSE parser.
  - Treat nonempty malformed completed tool arguments as terminal errors; do not replace them with `{}`.
  - Validate missing tool IDs/names, unfinished calls and conflicting indices.
  - Bound and sanitize diagnostics.
  - Audit equivalent permissive paths in `openai_responses.rs` and `google.rs`; either fix them in the same contract or list them as unresolved provider-specific blockers.
- **Dependencies:** required before D1 fragmented-SSE fixture.
- **Tests:**
  - Retain and extend the positive fragmented loopback test.
  - Add `crates/pi-ai/tests/openai_malformed_sse.rs`.
  - Add `crates/pi-agent/tests/malformed_provider_no_tool.rs`.
  - Test malformed event, malformed assembled arguments, truncation at `[DONE]`, invalid metadata and bounded oversized input.
- **Exit gate:**
  - Valid fragmentation succeeds.
  - Every malformed case terminates with a provider error.
  - No `ToolExecutionStart`, tool side effect or apparently successful `Done` follows corruption.
- **Non-goals:**
  - Replacing the SSE library without evidence it is necessary.
  - Retrying protocol-corrupt successful responses.
- **Risks:**
  - Event ordering must settle exactly once; error paths must not emit both error and success terminal events.

---

## 4. Delivery and compatibility

### Milestone D1 — Deterministic M1a expected and actual fixtures

- **Current status:** harness exists; all 13 required cases unattested
- **User scenario:** A maintainer can reproduce the pinned reference output and compare current Rust behavior in a clean checkout without API keys or external provider traffic.
- **Current files:**
  - `fixtures/upstream-pi/manifest.json`
  - `scripts/capture-upstream-fixtures`
  - `scripts/capture_adapters/m1a_adapters.py`
  - `scripts/validate-fixture-manifest`
  - `scripts/contract_fixture_lib.py`
  - `tests/contract/`
  - `.github/workflows/capture-reference.yml`
- **Exact changes:**
  - Preserve the existing set of 13 M1a IDs.
  - Define one canonical expected/actual JSON envelope and canonical UTF-8/LF serialization.
  - Add per-case expected SHA-256 and verify filename, case ID, oracle and digest.
  - Replace credential-dependent `cli.print.basic` and `agent.serial-tool-loop` captures with scripted providers.
  - Implement the four current `NotImplementedError` adapters:
    - `provider.openai-chat.fragmented-sse`
    - `tool.bash.cancel-descendants`
    - `resource.context-precedence`
    - `resource.untrusted-project`
  - Make capture transactional and run each adapter twice in independent temporary roots to detect nondeterminism.
  - Process `pi-rs-invariant` cases through executable invariant tests rather than permanently skipping them.
  - Add `scripts/generate-pi-rs-fixtures` to generate exactly 13 actual artifacts.
  - Normalize only explicit case-specific volatile fields; do not globally normalize arbitrary paths or semantic IDs.
- **Dependencies:** RB1–RB6, especially RB3 and RB6.
- **Tests:**
  - Add `tests/contract/test_capture.py`.
  - Extend manifest/validator tests for missing, extra, pending and digest-mismatch cases.
  - Verify failed capture leaves committed fixtures unchanged.
  - Run generator twice and compare all actual hashes.
- **Exit gate:**
  - `./scripts/validate-fixture-manifest --milestone M1a`
  - `./scripts/generate-pi-rs-fixtures --milestone M1a --actual target/contract-fixtures/M1a`
  - `./scripts/compare-contract-fixtures --milestone M1a --actual target/contract-fixtures/M1a`
  - All return zero, produce exactly 13 cases, and contain no pending/skipped cases.
- **Non-goals:**
  - Capturing all later upstream milestones.
  - Live-provider fixtures.
  - Automatically committing fixture updates from CI.
- **Risks:**
  - The manifest oracle commit and the newer gap-analysis commit differ; do not mix their outputs.
  - Time, PID, port and temporary path leakage can create false fixture churn.

### Milestone D2 — Invariant adapters and strict comparator

- **Current status:** comparator detects several mutations but does not enforce all completeness/digest invariants
- **User scenario:** A semantic behavior change such as tool order, stop reason or session parent change reliably fails compatibility CI.
- **Current files:**
  - `scripts/compare-contract-fixtures`
  - `scripts/contract_fixture_lib.py`
  - `tests/contract/test_comparator.py`
  - `fixtures/upstream-pi/manifest.json`
- **Exact changes:**
  - Validate manifest and expected digests before comparison.
  - Require the actual directory to contain exactly the required case set.
  - Compare JSON types, key sets, list order and values with JSON Pointer diagnostics.
  - Replace the inaccurate pending/skipped count with exact passed/pending/failed totals.
  - Add per-case normalization allowlists to the manifest.
  - Provide executable invariant adapters for fake cancellation, native recovery, checksum/COW, Node absence and Termux environment.
- **Dependencies:** D1 schema and generator.
- **Tests:**
  - Mutate role, stop reason, tool order, exit code, parent/provenance, missing/extra keys, missing/extra files and digests.
  - Ensure all mutations fail at the expected JSON Pointer.
- **Exit gate:**
  - Unmodified fixture sets pass.
  - Every semantic mutation fails.
  - Comparator can never report success with a pending required case.
- **Non-goals:**
  - Fuzzy comparisons that hide unsupported behavior.
- **Risks:**
  - Overbroad normalization can create false parity.

### Milestone D3 — Required CI checks

- **Current status:** Linux/macOS cargo and supply-chain CI exist; M1a and aggregate required gate do not
- **User scenario:** A pull request cannot merge if security regressions, compatibility drift, docs mismatch or Termux failures remain.
- **Current files:**
  - `.github/workflows/ci.yml`
  - `.github/workflows/docs.yml`
  - `deny.toml`
  - `rust-toolchain.toml`
- **Exact changes:**
  - Add reusable `.github/workflows/verify.yml`.
  - Keep the existing Ubuntu/macOS × stable/MSRV coverage; do not duplicate it from scratch.
  - Use `--locked` consistently.
  - Separate jobs:
    - `rust-test`
    - `lint`
    - `contracts-m1a`
    - `supply-chain`
    - `docs-consistency`
    - `termux`
    - aggregate `ci-required`
  - Run Python contract tests and M1a generation/comparison once in the authoritative Ubuntu contracts job.
  - Add rustdoc warnings gate.
  - Reconcile `cargo audit` ignores with `deny.toml`, including justification and expiry.
  - Pin third-party actions to reviewed immutable commit SHAs.
  - Configure branch protection around the stable aggregate name `ci-required`.
- **Dependencies:** D1–D2 and stable blocker tests.
- **Tests/commands:**
  - `cargo build --locked --workspace --all-targets`
  - `cargo build --locked --workspace --release`
  - `cargo test --locked --workspace --all-targets`
  - `cargo fmt --all -- --check`
  - `cargo clippy --locked --workspace --all-targets -- -D warnings`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps`
  - `python3 -m unittest discover -s tests/contract -p "test_*.py" -v`
  - M1a validator/generator/comparator commands from D1
  - `cargo deny check`
  - `cargo audit --file Cargo.lock`
- **Exit gate:**
  - Any failed job blocks `ci-required`.
  - Branch protection requires `ci-required`.
- **Non-goals:**
  - Replacing all existing workflow structure merely for naming consistency.
- **Risks:**
  - GitHub repository settings are outside the YAML diff and require explicit administrative verification.

### Milestone D4 — Termux portability and attestation

- **Current status:** partial helpers/tests; no authoritative release gate
- **User scenario:** `pi-rs` builds and runs on Termux without `/bin/bash` or `/tmp`, and process cancellation leaves no descendants.
- **Current files:**
  - `crates/pi-agent/src/tools/bash.rs`
  - `crates/pi-agent/tests/tools_smoke.rs`
  - `crates/pi-agent/tests/m1a_limits_and_fake_provider.rs`
  - `crates/pi-agent/tests/m06_fake_no_socket_and_session_cow.rs`
  - `crates/pi-coding-agent/src/termux.rs`
  - session tests using temporary directories
  - `.github/workflows/ci.yml`
- **Exact changes:**
  - Replace operational/test `/tmp` assumptions with isolated `$TMPDIR`/`std::env::temp_dir()`.
  - Centralize shell resolution:
    1. executable absolute `$SHELL`;
    2. `$SHELL` name found through `PATH`;
    3. executable `$PREFIX/bin/sh`;
    4. `sh` found through `PATH`;
    5. explicit failure.
  - Preserve process-group setup; implement bounded `SIGTERM` then `SIGKILL` and guaranteed reaping where required by the fixture.
  - Add `scripts/verify-termux`.
  - Add protected self-hosted Termux workflow/job using an ephemeral or thoroughly cleaned runner without release secrets.
  - Keep Linux `PREFIX` simulation as a fast test, but do not label it authoritative Termux evidence.
- **Dependencies:** D1 actual generator; bash parity milestone U3 may extend the same code.
- **Tests:**
  - Invalid/unset `$SHELL`, Termux `PREFIX`, paths with spaces and nonexecutable candidates.
  - Descendant cancellation with bounded timeout.
  - All filesystem tests under a unique `$TMPDIR`.
  - Full Rust/contract suite on actual Termux exact SHA.
- **Exit gate:**
  - No operational `/tmp` assumption remains.
  - Tests pass without `/bin/bash`.
  - Exact-SHA Termux job is required for release.
- **Non-goals:**
  - Treating Ubuntu simulation as Android proof.
- **Risks:**
  - Self-hosted runners executing untrusted PR code require isolation and no secrets.
  - Process semantics differ across Unix targets and must be tested rather than inferred.

### Milestone D5 — Documentation consistency and migration messaging

- **Current status:** inconsistent versions, paths and capability claims
- **User scenario:** A user follows README/docs and reaches the actual config/session paths, provider and release behavior.
- **Current files:**
  - `README.md`
  - `ROADMAP.md`
  - `CHANGELOG.md`
  - `crates/pi-coding-agent/README.md`
  - `docs/compatibility-matrix.md`
  - `docs/src/install.md`
  - `docs/src/cli/sessions.md`
  - `docs/src/contributing.md`
- **Exact changes:**
  - Add `scripts/check-doc-consistency` and focused contract tests.
  - Derive workspace/crate versions and MSRV from structured Cargo metadata.
  - Remove manually maintained passing-test counts or generate them.
  - Align namespace on actual `pi-rs` paths.
  - Correct stale claims about version `1.0.0`, future/delivered provider features and session format.
  - Tie every supported/candidate/read-only claim to its fixture ID.
  - Document migration behavior from active legacy `.json` sessions to the selected JSONL store before enabling conversion.
- **Dependencies:** RB3/RB4 and D1; final status updates happen only after comparator passes.
- **Tests:**
  - Mutation tests for stale versions, paths and unsupported status promotion.
  - `mdbook build docs`.
- **Exit gate:**
  - Docs consistency and mdBook jobs pass.
  - No claim exceeds fixture evidence.
- **Non-goals:**
  - Marketing expansion of the feature list.
- **Risks:**
  - Naive global text matching may flag historical changelog entries; consistency checks should use bounded headings/markers.

### Milestone D6 — Exact-commit release provenance

- **Current status:** tag builds bypass product and compatibility gates
- **User scenario:** Every downloadable binary can be traced to the exact tested source commit, version, lockfile and signed provenance.
- **Current files:**
  - `.github/workflows/release.yml`
  - `.github/workflows/ci.yml`
  - `Cargo.toml`
  - crate manifests
  - `Cargo.lock`
  - `CHANGELOG.md`
- **Exact changes:**
  - Make release invoke the same reusable verification for the tag SHA.
  - Reject tags not equal to `v<workspace-version>`.
  - Check workspace/crate/internal dependency/changelog version coherence.
  - Require a clean checkout and locked graph.
  - Build with `cargo build --locked --release`.
  - Pin actions to immutable SHAs and minimize permissions per job.
  - Package deterministic archives with README and LICENSE.
  - Smoke-test every released target; remove a target from the declared matrix if it cannot be executed or validly emulated.
  - Generate `SHA256SUMS`, an SPDX/CycloneDX SBOM, release manifest and signed GitHub/Sigstore provenance.
  - Use a protected release environment.
  - Define crate publication as either an automated attested path with `cargo package --locked`, or an explicitly separate process.
- **Dependencies:** RB1–RB6, D1–D5.
- **Tests:**
  - Version-script negative tests.
  - Checksum mutation test.
  - Package twice with fixed `SOURCE_DATE_EPOCH`.
  - Binary `--version`, `--help` and fail-closed smoke tests.
- **Exit gate:**
  - Publish cannot start without exact-SHA `ci-required`, Termux attestation and matching versions.
  - Release includes all declared archives, checksums, SBOM, manifest and verifiable provenance.
- **Non-goals:**
  - Publishing untested targets.
  - Treating workflow YAML alone as proof that branch/environment protections are enabled.
- **Risks:**
  - Reproducible binary hashes may require pinned linker/runner metadata; retain diagnostics rather than dropping the gate silently.

---

## 5. Upstream gaps as explicit milestones

These milestones follow release-blocker and delivery work. They are incremental completion work on the existing crates, not a wholesale rewrite. Unless fixture evidence already exists, the target status remains `candidate` until its milestone gate passes.

### Milestone U1 — Session v3 and tree

- **Current status:** **unsupported** for structural v3/tree; migration seams **partial**
- **Target:** **supported** native v3; **read-only** Pi v1/v2/v3 import
- **User scenario:** Users branch, resume and inspect a session tree while preserving structural events and source provenance.
- **Current files:** RB3 session files.
- **Exact changes:**
  - Add v3 header and typed entries with IDs, parent IDs and timestamps.
  - Support message, model/thinking changes, compaction, branch summary, custom/custom-message, labels and session info.
  - Track the active leaf and build effective context from its ancestor path.
  - Preserve or explicitly reject unknown entry types without silently converting them.
  - Implement v1/v2 migration into an in-memory read-only representation followed by COW v3.
- **Dependencies:** RB2–RB3, D1 fixture framework.
- **Tests:** upstream parse/round-trip, tree traversal, branch switching, effective context, unknown entries and migration.
- **Exit gate:** Rust reads pinned upstream v3 fixtures; pinned upstream parser reads Rust files; parent chains and context match.
- **Non-goals:** in-place writes to upstream sessions.
- **Risks:** schema drift between fixture oracle and newer upstream gap reference.

### Milestone U2 — Agent queues and events

- **Current status:** serial loop **supported implementation/partial parity**; steering/follow-up **unsupported**
- **Target:** **supported** queue/event subset
- **User scenario:** A user steers an active run, queues follow-up input and receives authoritative run-state events.
- **Current files:**
  - `crates/pi-agent/src/agent_loop.rs`
  - `crates/pi-agent/src/types.rs`
  - `crates/pi-coding-agent/src/interactive.rs`
- **Exact changes:**
  - Add a long-lived `AgentSession` runtime around the existing serial loop.
  - Add steering and follow-up queues with explicit all/one-at-a-time modes.
  - Expose phase, queue count, cancellation, retry and compaction state.
  - Guarantee one terminal settlement event per run.
  - Retain the existing serial tool executor as the initial execution strategy.
- **Dependencies:** U1 structural sessions, RB5 permissions, U3 cancellation.
- **Tests:** queue order, steering boundary, follow-up scheduling, abort, settlement and allow-session across turns.
- **Exit gate:** event order and resulting context match committed fixtures.
- **Non-goals:** parallel tools unless separately specified and fixture-backed.
- **Risks:** cancellation races can duplicate terminal events or lose queued messages.

### Milestone U3 — Tool parity

- **Current status:** **partial**
- **Target:** **supported** declared built-in subset
- **User scenario:** `read`, `bash`, `edit`, `write`, `grep`, `find/glob` and `ls` have predictable bounds, cancellation and mutation semantics.
- **Current files:** `crates/pi-agent/src/tools/*.rs`, existing tool tests.
- **Exact changes:**
  - `read`: align default paging and exact 50 KiB behavior with the selected oracle.
  - `bash`: cancellation token, TERM→KILL, reaping, bounded output and deterministic truncation metadata.
  - `edit`: multi-block matching, overlap rejection and CRLF/BOM preservation.
  - `write`: same-directory atomic replace.
  - Serialize concurrent write/edit operations per canonical target.
  - Align grep/find/ls ordering, ignore behavior and output limits.
  - Explicitly classify Rust-only tools instead of implying upstream schema parity.
- **Dependencies:** D4 shell work; U2 cancellation/events for streamed tool updates.
- **Tests:** existing smoke tests plus read bounds, first-line-over-limit, descendant cancellation, truncation, multi-edit, atomic write and concurrent mutation fixtures.
- **Exit gate:** every declared built-in subset row has passing expected/actual evidence.
- **Non-goals:** image read or persistent interactive shell unless independently fixture-backed.
- **Risks:** exact truncation and ignore semantics can vary by platform.

### Milestone U4 — Trust, resources and skills

- **Current status:** trust/context **partial**; skills/templates **unsupported/deferred**
- **Target:** **supported** trusted local subset
- **User scenario:** Trusted project resources load in canonical order; untrusted or symlink-escaped executable resources never load.
- **Current files:**
  - `crates/pi-coding-agent/src/trust.rs`
  - `crates/pi-coding-agent/src/project.rs`
  - `crates/pi-coding-agent/src/system_prompt.rs`
- **Exact changes:**
  - Persist trust against canonical project identity.
  - Separate passive instructions from executable resources.
  - Canonicalize resource paths and reject symlink escape.
  - Implement deterministic global/ancestor/current precedence and worktree deduplication.
  - Add local `SKILL.md` and prompt-template discovery with frontmatter validation, collision handling and typed diagnostics.
  - Never load project skills/extensions before trust.
- **Dependencies:** D1 resource fixtures; U5/U6 may consume resources later.
- **Tests:** context precedence, untrusted project, symlink escape, trust persistence, frontmatter errors and collision order.
- **Exit gate:** resource ordering matches fixtures and untrusted executable resources are absent from prompts/commands.
- **Non-goals:** `npm:`/`git:` installation, private packages or remote registries.
- **Risks:** canonical root identity across worktrees and symlinks requires an explicit policy.

### Milestone U5 — JSONL RPC

- **Current status:** **unsupported**
- **Target:** **supported** documented command subset
- **User scenario:** An external client controls a session over stdin/stdout JSONL without contaminating stdout.
- **Current files:**
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-coding-agent/src/print_mode.rs`
  - U2 runtime
- **Exact changes:**
  - Add `src/rpc/` with types, bounded line transport and server.
  - Support prompt, steer, follow-up, abort, new session, state query, model/thinking setters, queue modes, compaction/retry controls and session tree queries.
  - Preserve optional request IDs in success and error responses.
  - Emit machine output only on stdout; diagnostics on stderr.
  - Return stable errors for unknown commands and recover after malformed lines.
- **Dependencies:** U1, U2, RB1 and RB4.
- **Tests:** lifecycle, event ordering, queue controls, unknown command IDs, malformed/oversized line and stdout cleanliness.
- **Exit gate:** pinned upstream-compatible scripted RPC client passes the declared command matrix.
- **Non-goals:** HTML export until separately implemented.
- **Risks:** prompt acknowledgement and asynchronous event ordering are wire contracts.

### Milestone U6 — Event-driven TUI

- **Current status:** basic REPL **partial**; upstream TUI **unsupported**
- **Target:** **supported behavioral subset**, not byte-for-byte rendering parity
- **User scenario:** Users view streaming output, tool/permission state, queues and session tree while retaining terminal integrity after abort or panic.
- **Current files:**
  - `crates/pi-coding-agent/src/interactive.rs`
  - `crates/pi-coding-agent/src/main.rs`
  - `crates/pi-coding-agent/Cargo.toml`
- **Exact changes:**
  - Introduce a Rust event-driven TUI backend with state reducer separated from renderer.
  - Display streaming text/thinking, tool state, permissions, trust, queues, retry and compaction.
  - Add multiline input, abort, steering/follow-up, model/thinking settings, session and tree selection.
  - Guarantee raw-mode restoration on normal exit, error, panic and supported signals.
- **Dependencies:** U1, U2, U4 and U5 mode separation.
- **Tests:** reducer snapshots, virtual terminal flows, queue display, permission/trust prompts, tree navigation and terminal restoration.
- **Exit gate:** declared behavioral fixture set passes on supported terminals/platforms.
- **Non-goals:** byte-for-byte ANSI parity, every clipboard/image protocol or all upstream visual components.
- **Risks:** headless CI cannot fully prove real-terminal signal behavior; retain a platform attestation.

### Milestone U7 — Optional extensions

- **Current status:** **unsupported**
- **Target:** **supported Tier-A only**; richer capabilities remain unsupported
- **User scenario:** A trusted local extension registers a tool/command through an optional Node sidecar; core operation remains functional when Node is absent.
- **Current files:** U4 resource loader, U1 custom entries, U2 events, U6 UI.
- **Exact changes:**
  - Add a versioned bounded Rust↔Node protocol with handshake, request IDs, deadlines and cancellation.
  - Load sidecars only from trusted canonical local paths.
  - Tier-A scope: tools, slash commands, cancelable hooks, persisted custom state and basic select/confirm/input/editor/notify/status requests.
  - Reject Bun/private/native/custom-component capabilities before execution.
  - Reap crashed/hung sidecars and keep core operational without Node.
- **Dependencies:** U1, U2, U4 and U6.
- **Tests:** Node absent, fake extension lifecycle, cancelable hook, persisted custom entry, unsupported capability and untrusted extension.
- **Exit gate:** Tier-A fixture set passes both with Node present and removed from `PATH`.
- **Non-goals:** full Oh My Pi API, package installation, native addons or arbitrary TUI components.
- **Risks:** extension host materially expands the threat model; keep it optional and capability-scoped.

### Milestone U8 — ACP/binary protocol

- **Current status:** **unsupported**
- **Target:** initially **deferred** for the release-blocker train; later **supported local protocol-v2 subset**
- **User scenario:** A local client connects to a Rust server using strict CBOR frames and manages remote sessions.
- **Current files:** no implementation; workspace addition required.
- **Exact changes:**
  - First confirm terminology: this roadmap uses the pinned upstream binary protocol v2, strict CBOR and 4-byte big-endian framing, distinct from JSONL RPC.
  - Add `crates/pi-protocol` for bounded CBOR, framing and schemas.
  - Add local stdio/Unix-socket server/client modes only after codec fixtures pass.
  - Implement list/create/attach/detach/prompt/steer/abort/model/thinking and revisioned snapshots.
  - Enforce ownership and release it on disconnect.
- **Dependencies:** U1, U2, U6 and completed codec fixtures; independent of U5 transport.
- **Tests:** cross-language CBOR vectors, fragmented/multiple/truncated/oversized frames, handshake, lifecycle, locks, revision order and disconnect cleanup.
- **Exit gate:** TS↔Rust encode/decode passes every declared message variant and local remote lifecycle fixtures pass.
- **Non-goals:** TCP/auth before a separate threat model; external protocols also called “Agent Client Protocol” unless separately specified.
- **Risks:** ACP terminology is ambiguous; protocol scope must be confirmed before implementation.

---

## 6. Dependency graph

```text
RB1 permission EOF ───────────────┐
  └─ RB5 allow-session            │
RB2 session containment ── RB3 truthful active store ── U1 session v3/tree
RB4 provider routing ─────────────┤
RB6 malformed SSE ────────────────┤
                                 ▼
                     D1 deterministic M1a fixtures
                                 │
                     D2 invariant/comparator strictness
                                 │
                ┌────────────────┼────────────────┐
                ▼                ▼                ▼
        D3 required CI   D4 Termux gate   D5 docs consistency
                └────────────────┼────────────────┘
                                 ▼
                     D6 exact-commit release

U1 session v3/tree ─────┬─ U2 queues/events ── U5 JSONL RPC
                        │          │
D4 shell/process ── U3 tools ──────┘
U4 trust/resources/skills ─────── U6 TUI
U1 + U2 + U4 + U6 ────────────── U7 extensions
U1 + U2 + U6 + codec fixtures ── U8 ACP
```

Release of the current headless completion slice depends on RB1–RB6 and D1–D6. U1–U8 must not all be made simultaneous blockers unless the release explicitly claims those surfaces.

---

## 7. Migration strategy

### Sessions

1. Before changing storage, document and test the existing `.json` format and actual `pi-rs` config root.
2. Introduce the new store reader alongside the legacy reader.
3. Legacy native `.json` files remain readable.
4. On first mutation:
   - create a new versioned JSONL file;
   - retain the original file unchanged;
   - record source path/ID and SHA-256 provenance;
   - atomically publish the new file only after complete validation.
5. Pi v1/v2/v3 imports are always read-only; mutation creates a native COW session.
6. Do not bulk-convert all sessions automatically at startup.
7. Provide a clear diagnostic and recovery instructions for malformed or locked sessions.
8. Remove legacy write support only after at least one documented release cycle and migration fixture coverage.

### Config paths

1. Treat `$XDG_CONFIG_HOME/pi-rs` as the current executable namespace.
2. Detect documented legacy `$XDG_CONFIG_HOME/pi` only if a deliberate migration policy is approved.
3. Never silently merge two roots.
4. If import is offered, show source/destination and do not overwrite either without an explicit option.

### Fixtures

1. Keep the current oracle commit pinned during blocker completion.
2. Commit deterministic M1a expected artifacts in a reviewed fixture-only change.
3. Add actual generators and comparator gates.
4. Update the oracle commit only in a separate migration PR that records semantic fixture differences.
5. Never set `captured: true` manually without reproducible capture/invariant evidence.

### Public APIs and protocols

- Add JSONL RPC and binary ACP as additive modes.
- Version extension and binary protocols from their first release.
- Unknown fields/commands must have explicit compatibility behavior.
- Do not reuse the same CLI flag or status row for JSONL RPC and binary ACP.

---

## 8. CI and release plan

### Required branch checks

The stable required check `ci-required` aggregates:

1. Rust stable/MSRV build and tests on Ubuntu/macOS.
2. Formatting, clippy and rustdoc warnings.
3. Permission EOF, session traversal, provider routing and malformed SSE security tests.
4. Python contract harness.
5. M1a validator, actual generator and comparator.
6. Supply-chain policy.
7. Documentation consistency and mdBook build.
8. Authoritative exact-SHA Termux attestation.

### Release sequence

```sh
cargo build --locked --workspace --all-targets
cargo build --locked --workspace --release
cargo test --locked --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps

python3 -m unittest discover -s tests/contract -p "test_*.py" -v
./scripts/validate-fixture-manifest --milestone M1a
rm -rf target/contract-fixtures/M1a
./scripts/generate-pi-rs-fixtures \
  --milestone M1a \
  --actual target/contract-fixtures/M1a
./scripts/compare-contract-fixtures \
  --milestone M1a \
  --actual target/contract-fixtures/M1a

cargo deny check
cargo audit --file Cargo.lock
./scripts/check-doc-consistency
mdbook build docs
```

For a tag release, additionally require:

- tag resolves to the tested SHA;
- tag/workspace/crate/changelog versions match;
- clean locked checkout;
- exact-SHA Termux result;
- deterministic packages;
- runnable binary smoke tests;
- checksum, SBOM, release manifest and signed provenance;
- protected release-environment approval.

---

## 9. Definition of Done

The current-main completion slice is done only when:

1. EOF/read errors and blank permission input deny destructive calls.
2. Session IDs cannot escape the configured root through read/delete/resume paths.
3. Active session format, path and migration documentation match executable wiring.
4. User-facing Pi import is genuinely read-only and COW preserves source bytes.
5. Documented models route to the correct provider; unknown explicit models fail.
6. Allow-session persists across turns of one session and resets at its documented boundary.
7. Malformed SSE/tool arguments terminate before tool execution.
8. All 13 M1a cases have reviewed expected fixtures or executable invariant specs.
9. Actual generation produces exactly 13 deterministic artifacts.
10. Manifest validation and comparator pass with no pending/skipped cases.
11. Linux, macOS and authoritative Termux required checks pass for the same SHA.
12. Docs contain no stronger capability claim than the compatibility matrix evidence.
13. A release tag cannot bypass exact-commit CI, version, package, checksum, SBOM or provenance checks.
14. Existing 58-test/26-test baseline coverage remains intact or is intentionally updated with explained replacements.
15. U1–U8 statuses remain `partial`, `unsupported` or `deferred` until their own fixtures pass; no “full parity” or “everything supported” claim is made.

## 10. Next first task

Implement **RB1 fail-closed EOF permissions** first:

1. Add table-driven prompt parsing tests in `crates/pi-coding-agent/src/permission.rs`.
2. Change `read_line` handling so `Ok(0)`, error, blank and unknown input deny.
3. Add the fake-provider executable regression in `crates/pi-coding-agent/tests/permission_cli.rs`.
4. Prove closed stdin produces `PermissionDenied`, no `ToolExecutionStart`, no sentinel file and no child process.
5. Run the focused test, then the complete locked workspace suite before moving to RB2.

This is the smallest change that closes the only BLOCKER-severity security finding and establishes the test seam reused by RB5.

## Files to Modify

- `crates/pi-coding-agent/src/permission.rs` — fail-closed prompting and persistent allow-session.
- `crates/pi-coding-agent/src/main.rs` — safe session commands, explicit model resolution and future mode wiring.
- `crates/pi-coding-agent/src/print_mode.rs` — testable machine-mode permission/event path.
- `crates/pi-coding-agent/src/interactive.rs` — safe resume, active session store, permission reset and later runtime/TUI integration.
- `crates/pi-coding-agent/src/session.rs` — ID containment, migration and active storage transition.
- `crates/pi-coding-agent/src/config.rs` — fallible model/provider resolution.
- `crates/pi-coding-agent/src/trust.rs` — canonical persisted trust.
- `crates/pi-coding-agent/src/project.rs` — safe resource precedence.
- `crates/pi-coding-agent/src/system_prompt.rs` — typed trusted resources.
- `crates/pi-agent/src/agent_loop.rs` — permission/event integration and later stateful runtime.
- `crates/pi-agent/src/types.rs` — runtime state/events.
- `crates/pi-agent/src/tools/*.rs` — bounded tool parity and Termux shell/mutation behavior.
- `crates/pi-ai/src/providers/openai.rs` — fail-closed SSE/tool-argument parsing.
- `crates/pi-ai/src/providers/openai_responses.rs` — provider protocol consistency.
- `crates/pi-ai/src/providers/google.rs` — routing/error consistency.
- `crates/pi-ai/src/retry.rs` — later provider retry parity.
- `fixtures/upstream-pi/manifest.json` — truthful per-case attestation and digests.
- `scripts/capture-upstream-fixtures` — deterministic transactional capture.
- `scripts/capture_adapters/m1a_adapters.py` — complete offline M1a adapters.
- `scripts/compare-contract-fixtures` — strict completeness and semantic comparison.
- `scripts/contract_fixture_lib.py` — canonical JSON/digest/normalization helpers.
- `scripts/validate-fixture-manifest` — strict M1a gate.
- `tests/contract/*.py` — capture, manifest, comparator and docs mutation tests.
- `.github/workflows/ci.yml` — reusable required verification.
- `.github/workflows/capture-reference.yml` — pinned reproducible capture.
- `.github/workflows/docs.yml` — required documentation consistency.
- `.github/workflows/release.yml` — exact-commit, package and provenance gates.
- `README.md`, `ROADMAP.md`, `CHANGELOG.md` — truthful versions, paths and capability claims.
- `docs/compatibility-matrix.md` — fixture-driven statuses.
- `docs/src/install.md` — actual installation/provider/release verification.
- `docs/src/cli/sessions.md` — active format, location and migration.
- `docs/src/contributing.md` — authoritative validation sequence.
- `Cargo.toml`, crate manifests and `Cargo.lock` — version/dependency coherence only when required.
- `deny.toml` — reconciled advisory policy.

## New Files

- `crates/pi-coding-agent/tests/permission_cli.rs` — EOF destructive-tool regression.
- `crates/pi-coding-agent/tests/permission_session.rs` — allow-session across turns.
- `crates/pi-coding-agent/tests/session_cli_security.rs` — traversal and symlink regressions.
- `crates/pi-coding-agent/tests/model_routing.rs` — CLI/env/file routing.
- `crates/pi-ai/tests/provider_routing_loopback.rs` — endpoint/auth/body routing.
- `crates/pi-ai/tests/openai_malformed_sse.rs` — adversarial stream cases.
- `crates/pi-agent/tests/malformed_provider_no_tool.rs` — no tool after corruption.
- `scripts/generate-pi-rs-fixtures` — exact 13-case actual generator.
- `scripts/check-doc-consistency` — version/path/status verification.
- `scripts/verify-termux` — authoritative Termux checks.
- `scripts/check-release-version` — tag/workspace/crate/changelog verification.
- `scripts/package-release` — deterministic release packaging.
- `scripts/verify-release-artifacts` — checksum/SBOM/manifest/smoke validation.
- `.github/workflows/verify.yml` — reusable required workflow.
- `.github/workflows/termux.yml` — protected Termux entry point if not embedded in `verify.yml`.
- Session schema/store/context modules if RB3/U1 cannot remain maintainable in one file.
- Runtime queue/compaction modules for U2.
- Resource/frontmatter/skills modules for U4.
- `crates/pi-coding-agent/src/rpc/` for U5.
- `crates/pi-coding-agent/src/tui/` for U6.
- `crates/pi-coding-agent/src/extensions/` for U7.
- `crates/pi-protocol/` and remote client/server modules for U8.

## Dependencies

- RB1 precedes RB5.
- RB2 precedes RB3 and U1.
- RB3, RB4 and RB6 must stabilize before M1a fixture finalization.
- D1 precedes D2; both precede required compatibility CI.
- D4 is required before exact-SHA Termux release attestation.
- D3–D5 precede D6.
- U1 precedes queues, RPC, extension state and remote session snapshots.
- U2 and U3 precede complete abort/queue behavior.
- U4 precedes skills, themes and extensions.
- U5 and U8 are distinct transports and must not be conflated.
- U6 precedes extension UI parity.
- No upstream gap milestone may promote itself to `supported` before its fixture/comparator/CI dependency is present.

## Risks

- **BLOCKER:** `crates/pi-coding-agent/src/permission.rs` currently treats EOF and empty input as permission allow.
- **HIGH:** `crates/pi-coding-agent/src/session.rs` and `src/main.rs` accept unchecked session IDs in filesystem paths.
- **HIGH:** `docs/compatibility-matrix.md` claims native v3-compatible JSONL while active CLI persistence is pretty single JSON under the `pi-rs` config root.
- **HIGH:** `crates/pi-coding-agent/src/config.rs` silently falls back for unknown models and does not route documented Gemini correctly.
- **HIGH:** all 13 M1a cases remain uncaptured, so runtime compatibility is unattested.
- **HIGH:** `.github/workflows/release.yml` currently publishes tag artifacts without exact-commit test, M1a, Termux, version, checksum, SBOM or provenance gates.
- **MEDIUM:** allow-session is not retained across separate interactive turns.
- **MEDIUM:** malformed SSE and tool arguments can be silently accepted or changed.
- A clean-target all-targets Rust run is still needed because one fresh Termux attempt failed on corrupt/zero-length build artifacts.
- Authoritative Termux CI requires secure maintained runner capacity.
- Session storage migration and protocol additions are compatibility boundaries; they need fixture-backed incremental rollout rather than a wholesale rewrite.
- Fixture oracle `2efa728...` and gap-analysis upstream `f0deb8...` differ. Updating one from the other is an explicit decision, not an implementation detail.
- ACP terminology is ambiguous and must be fixed to a concrete protocol specification before U8 starts.
- Full parity, “everything supported,” and equivalent claims remain prohibited until every included surface has fixture evidence.