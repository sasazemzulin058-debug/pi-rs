# U3 Model Runtime Checkpoint

Status: READY_FOR_REVIEW

## Scope
- Implement `ProviderFactory::complete` default collector and public `pi_ai::complete` wrapper.
- Document availability refresh sequencing and OAuth `minOAuthValidityMs` status.

## Applicability Decision
- Availability refresh sequencing: NOT APPLICABLE / BLOCKED. Rust codebase currently lacks `ModelRuntime`, `ModelRegistry`, mutable availability refresh manager, and availability cache snapshot logic.
- OAuth minimum validity: NOT APPLICABLE / BLOCKED. Rust codebase currently lacks OAuth token resolution, OAuth credential storage, token expiry calculation, and refresh handlers. `StreamOptions` only handles direct API key parameter. No synthetic abstractions added.

## Implementation Details
- `crates/pi-ai/src/providers/mod.rs`: Added default `complete` method to `ProviderFactory` trait using `futures::StreamExt`. Collects stream until terminal event (`AssistantMessageEvent::Done` or `AssistantMessageEvent::Error`), returning the terminal `AssistantMessage`. Stream-item errors propagate as Rust `Err`, and missing terminal event returns `Error::InvalidResponse`.
- `crates/pi-ai/src/lib.rs`: Exposed public `complete` function delegating to `DefaultProviderFactory.complete(...)`.
- `crates/pi-ai/tests/complete_wrapper.rs`: Added 5 deterministic unit tests covering success, nonempty signed terminal error message preservation, missing done event, payload-bearing stream-item error propagation, and public wrapper dispatch using `FakeProviderFactory` and local stream-error factory.

## Sources
- Shared context: `/data/data/com.termux/files/home/.pi-subagents/shared-parity-context.md`
- U3/U4 scout: `/data/data/com.termux/files/home/.pi-subagents/u34-scout.md`
- U3 plan: `/data/data/com.termux/files/home/.pi-subagents/u3-plan.md`
- U3 fix plan: `/data/data/com.termux/files/home/.pi-subagents/u3-fix-plan.md`
- U3 fix2 plan: `/data/data/com.termux/files/home/.pi-subagents/u3-fix2-plan.md`
- U3 review 1: `/data/data/com.termux/files/home/.pi-subagents/u3-review.md`
- U3 review 2: `/data/data/com.termux/files/home/.pi-subagents/u3-review2.md`
- Upstream: commit `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee` (`packages/ai/src/compat.ts`, `packages/ai/src/utils/event-stream.ts`, `packages/coding-agent/src/core/model-runtime.ts`)

## Changed Files
- `crates/pi-ai/src/providers/mod.rs`
- `crates/pi-ai/src/lib.rs`
- `crates/pi-ai/tests/complete_wrapper.rs`
- `docs/plans/u3-model-runtime-checkpoint.md`

## Validation Results
- `cargo test --locked -p pi-ai --test complete_wrapper`: 5 passed (0.00s)
- `cargo test --locked -p pi-ai --test complete_wrapper -- --list`: 5 listed
- `cargo fmt --check`: PASSED
- `cargo clippy --locked -p pi-ai --all-targets -- -D warnings`: PASSED
- `git diff --check`: PASSED
