# U8 ACP Checkpoint: Scope, Protocol Identity & Deferred Implementation

## Status: Deferred / identity unresolved

U8 has no implementation, workspace crate, codec dependency, fixture, or runtime transport. This checkpoint records roadmap facts and separates them from proposed implementation policy. It does not claim compatibility.

## What roadmap currently says

`docs/plans/2026-08-03-current-main-completion-roadmap.md` lists U8 as **unsupported**, with an initial release-train target of **deferred** and a later goal of a supported local protocol-v2 subset. Roadmap description says that target uses strict CBOR, a 4-byte big-endian framing prefix, and local stdio/Unix-socket operation distinct from U5 JSONL RPC.

Roadmap also lists these eventual requirements:

- bounded CBOR, framing, and schemas in a future `crates/pi-protocol` crate;
- local stdio/Unix-socket server and client modes;
- list/create/attach/detach/prompt/steer/abort/model/thinking operations and revisioned snapshots;
- ownership release on disconnect;
- cross-language vectors plus fragmented, multiple, truncated, oversized, handshake, lifecycle, lock, revision-order, and disconnect-cleanup tests.

These are roadmap requirements, not implemented behavior or verified compatibility.

## Protocol identity remains unresolved

“ACP” is ambiguous across external protocols. Current repository evidence does not identify a canonical upstream ACP specification, message schema, version, or authoritative TypeScript/Rust fixture set. The roadmap's phrase “pinned upstream binary protocol v2” is not enough to establish protocol identity.

Therefore this document does **not** claim that the described CBOR framing is ACP-compatible, nor that `protocol.acp` has a compatibility-matrix status. `docs/compatibility-matrix.md` currently has no verified ACP contract entry. Do not add `supported`, `candidate`, or `deferred` ACP compatibility status until protocol identity and evidence are established.

## U5 separation

U5 owns public Pi JSONL RPC under `src/rpc`; U8 must not introduce a JSONL duplicate or relabel U5 messages as ACP. Any future U8 wire format requires separately identified protocol schemas and fixtures. This separation is a scope boundary, not evidence that either protocol is compatible with the other.

## Proposed policy for any future U8 work

The following are gates proposed by this checkpoint, not current roadmap facts:

1. Record canonical protocol identity first: specification URL or repository/version, message schema, framing rules, and ownership of compatibility claims.
2. Capture authoritative cross-language encode/decode vectors before adding codec or transport code. Include every declared message variant and canonical error behavior.
3. Add malformed-input vectors for unknown fields, invalid types, truncated and oversized frames, invalid lengths, and invalid session/revision data. Reject failures without allocating unbounded memory or executing operations.
4. Only after vectors pass, add the smallest bounded codec/framing crate and tests. Then add local stdio/Unix-socket lifecycle code with stdout reserved for protocol bytes and diagnostics on stderr.
5. Update compatibility documentation only from passing, reproducible fixtures; never infer compatibility from type names, framing resemblance, or test counts.

## Current blockers

- Canonical ACP identity, version, schema, and authority are unspecified.
- No authoritative CBOR vectors or framing fixtures are present.
- No bounded frame-size value or error contract is approved.
- No `crates/pi-protocol`, CBOR dependency, parser, server, client, handshake, or lifecycle implementation exists.
- Ownership, session-lock, revision, disconnect, and operation semantics lack verified wire schemas.
- Compatibility-matrix entry and status cannot be truthfully assigned.

## Deferred scope

No code, dependency, fixture, compatibility claim, Unix-socket or stdio execution loop, TCP binding, authentication, or remote-access behavior belongs in this documentation-only slice. TCP and authentication remain outside local U8 scope unless separate threat-model approval exists.

## Validation

Run:

```text
scripts/check-doc-consistency
```

Workspace tests are not an ACP validation gate because U8 has no implementation or fixtures.
