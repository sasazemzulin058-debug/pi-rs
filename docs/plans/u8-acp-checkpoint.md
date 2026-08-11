# U8 ACP Checkpoint: Scope, Protocol Identity & Standard ACP Alignment

## Status: Standard ACP v1 Partial Transport Active / Conformance Deferred

U8 implementation targets the official Agent Client Protocol v1 standard (JSON-RPC 2.0 over stdio with schema-defined capabilities and callbacks), not legacy CBOR framing or custom binary protocols.

## Protocol Identity & Scope Boundaries

“ACP” refers exclusively to the official Agent Client Protocol specification (https://agentclientprotocol.com/protocol/v1/overview).

1. **Schema Authority**: Vendored JSON Schema (`fixtures/acp/v1/schema.json`) with strict SHA-256 provenance (`fixtures/acp/v1/provenance.json`). Official immutable release identity metadata was unavailable at pin time; provenance records exact mutable source URL and exact bytes hash.
2. **Transport**: JSON-RPC 2.0 over stdio with stdout reserved for protocol messages and diagnostics on stderr. Recoverable malformed JSON/envelopes receive parse-error responses; oversized or broken transport fails.
3. **Separation from Pi RPC**: U5 owns public Pi JSONL RPC (`--mode rpc`), which uses native Pi event schemas. U8 owns standard ACP v1 (`--mode acp`). Wire formats, capability negotiation, and message structures are kept completely separate.
4. **Stale CBOR Claims Deleted**: Prior references to `crates/pi-protocol`, CBOR encoding, 4-byte big-endian frame headers, protocol-v2 Unix sockets, or revision snapshots when described as ACP are obsolete and removed.

## Implementation Architecture & Phases

1. **Vendor Schema & Provenance**: Pinned official ACP v1 JSON schema and verification scripts (`scripts/check-acp-schema`).
2. **Codec & Framing (`crates/pi-acp`)**: Bounded handwritten JSON-RPC 2.0 stdio framing crate. Schema-generated types and conformance remain deferred.
3. **Initialization & Capabilities**: Negotiation of client/agent capabilities (`initialize` handshake, tool/terminal/filesystem capabilities).
4. **Session Lifecycle & Prompting**: Session creation, prompt streaming, tool updates, cancellation, stop reasons.
5. **Client Callbacks**: Routing client-side filesystem, terminal, and permission operations back through negotiated client callbacks.
6. **Conformance Suite**: Automated verification script (`scripts/verify-acp-v1`) executing official/canonical test vectors.

## Deferred Scope & Security Boundaries

- TCP transport and network authentication are out of scope for local U8; stdio framing is standard.
- Unnegotiated client callbacks must fail closed.
- Handwritten subset types do not establish schema conformance.
- File system and terminal operations requested via ACP callbacks are subject to boundary check and permission confirmation.

## Validation

Run:

```text
scripts/check-acp-schema
scripts/check-doc-consistency
```
