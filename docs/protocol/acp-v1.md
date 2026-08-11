# ACP v1

Pi Rust ACP support starts with isolated JSON-RPC 2.0 over NDJSON transport. Wire messages require `jsonrpc: "2.0"`; one JSON object occupies one line. Messages are bounded to 1 MiB. Blank input lines are ignored. stdout remains caller-owned when integrating this crate.

Schema bytes are pinned at [`fixtures/acp/v1/schema.json`](../../fixtures/acp/v1/schema.json), with SHA-256 and retrieval metadata in [`fixtures/acp/v1/provenance.json`](../../fixtures/acp/v1/provenance.json). ACP official release metadata exposing immutable artifact identity was not available at pin time; `source` therefore records mutable official `releases/latest` URL, while SHA-256 enforces exact local bytes.

## Phase 1–3 boundary

Implemented: schema byte pin, provenance check, inventory generation, standalone transport, strict JSON-RPC envelope validation, bounded framing tests, and `pi-rs --mode acp` initialization-only server.

ACP mode accepts only `initialize`; it advertises `loadSession: false`, false optional prompt/MCP capabilities, and empty session/auth capabilities and methods. Session lifecycle (`session/new`, `session/prompt`, `session/cancel`, `session/update`), callbacks, authentication, MCP, U7 extension sidecar, existing Pi RPC integration, and conformance harness remain unimplemented.

Run checks:

```sh
./scripts/check-acp-schema
./scripts/generate-acp-inventory
cargo test -p pi-acp
cargo test -p pi-coding-agent --test acp_cli
```
