# U5 RPC Checkpoint

Status: IN PROGRESS (11/32 RPC commands recognized; queue and thinking controls implemented subsets; source-attested fixtures only)

## Completed

- `crates/pi-coding-agent/src/rpc/types.rs` defines `RpcCommand::SetSteeringMode` and `SetFollowUpMode`, typed `RpcQueueMode` (`"all"`, `"one-at-a-time"`), and omitted response `id` when absent (`#[serde(skip_serializing_if = "Option::is_none")]`).
- `crates/pi-coding-agent/src/rpc/server.rs` dispatches `set_steering_mode` and `set_follow_up_mode` to `AgentSession`, exposes updated state via `get_state`, handles error recovery for missing modes, and passes source-attested fixture replay without provider calls.
- `crates/pi-coding-agent/tests/fixtures/rpc-0.83-queue-controls.json` provides exact pinned request/response fixture with `source-attested` identity metadata.
- `crates/pi-coding-agent/tests/rpc_cli.rs` proves queue-control command execution and state visibility via public CLI `--mode rpc`.

## Verification Evidence

- `cargo test --locked -p pi-coding-agent --bin pi-rs rpc::types::tests -- --nocapture`: passed.
- `cargo test --locked -p pi-coding-agent --bin pi-rs rpc::server::tests -- --nocapture`: passed.
- `cargo test --locked -p pi-coding-agent --test rpc_cli -- --nocapture`: passed.
- `cargo fmt --check`: passed.
- `./scripts/check-doc-consistency`: passed.
- `python3 scripts/validate-fixture-manifest`: passed.
- `git diff --check`: passed.

## Residual gaps

- Recognized RPC command count: 11/32 (`prompt`, `steer`, `follow_up`, `abort`, `new_session`, `get_state`, `set_steering_mode`, `set_follow_up_mode`, `set_thinking_level`, `cycle_thinking_level`, `get_available_thinking_levels`). Thinking controls are source-attested only; upstream capture is unavailable.
- `get_state` response shape gaps: full upstream `Model` object projection missing (currently model ID string), `autoCompactionEnabled` field missing, optional `sessionFile` / `sessionName` fields omitted.
- Unimplemented RPC commands: model controls (`set_model`, `cycle_model`, `get_available_models`), compaction, retry, bash execution, session management/fork/tree, message history, slash command discovery. Thinking `max` is collapsed to `xhigh`; full upstream thinking-level semantics remain unverified.
- Host oracle capture: direct 0.83 capture unavailable locally; fixtures remain `source-attested`.

## Scope boundary

U5 files and queue-control fixtures only. U4, U6, U7, and U8 changes remain outside this checkpoint.
