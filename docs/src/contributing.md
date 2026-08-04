# Contributing

Issues and pull requests are welcome at
<https://github.com/sasazemzulin058-debug/pi-rs>.

- See [`ROADMAP.md`](https://github.com/sasazemzulin058-debug/pi-rs/blob/main/ROADMAP.md)
  for what is targeted at upcoming 1.x and 2.0 releases. Unchecked items
  are good candidates for first contributions; open an issue with the
  milestone tag before you start.
- See [`CHANGELOG.md`](https://github.com/sasazemzulin058-debug/pi-rs/blob/main/CHANGELOG.md)
  for what shipped in each release.

Before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same checks on macOS and Linux against stable Rust and the
declared MSRV (`1.80`).

## Termux Verification Runner

Termux CI requires external runner attestation (`D4`).

### Repository Variable Format

Configure `vars.TERMUX_RUNNER_LABELS` as a JSON array of strings:

```json
["self-hosted", "termux"]
```

### Runner Security

- `persist-credentials: false` MUST be set on checkout steps.
- Self-hosted runners execute untrusted workflow code; ensure runner isolation.
- Runner must be ephemeral or fully reset after each job. Workflow cleanup removes only verifier temp directories; it does not clean arbitrary files or build output.

### D4 Blocker Status

Full automated Termux verification depends on external runner infrastructure
registration (D4 blocker). Workflows validate configuration preflight on hosted
runners before attempting execution.
