# U5 RPC Checkpoint

## Completed

- `crates/pi-coding-agent/src/rpc/mod.rs` defines `transport` and `types` modules.
- `crates/pi-coding-agent/src/rpc/transport.rs` provides bounded JSONL framing through `read_record` and `write_record`. It accepts LF, CRLF, and final records without newline; skips empty lines; rejects invalid UTF-8 and records larger than `MAX_RECORD_SIZE`.
- `crates/pi-coding-agent/src/rpc/types.rs` defines bounded wire types: `RpcRequest`, `RpcResponse`, `RpcState`, `RpcCommand`, and strict `StreamingBehavior` (`steer` or `followUp`). State is carried in response `data`; no duplicate top-level `state` field exists.
- Boundary and rejection tests cover LF, CRLF, EOF, invalid UTF-8, oversized records, request shape, response shape, invalid streaming behavior, and invalid message types. `RpcRequest::command_kind` rejects unknown commands.

## Intentionally unexposed

- `rpc` is declared in `crates/pi-coding-agent/src/main.rs` with `#[allow(dead_code)]`. No RPC runtime server, session control loop, stdin/stdout dispatch, or public CLI behavior is exposed.
- `cli.rpc` remains deferred in `docs/compatibility-matrix.md`.

## Known residual gaps

- No runtime wiring or CLI entry point exists, so RPC commands are not executable through the binary.
- No upstream-compatible scripted RPC command matrix or fixture-backed runtime verification exists.
- Request-level command semantics, session ownership/concurrency, event streaming, error propagation, and permission integration remain deferred with runtime work.

## Scope boundary

This checkpoint contains transport and wire-type groundwork only. Runtime integration belongs to a later U5 slice; no U6-U8 work is included.
