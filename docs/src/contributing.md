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

## Termux Verification

Run authoritative Termux verification locally on Termux:

```sh
sh ./scripts/verify-termux
```

GitHub Actions does not execute this local device. Hosted workflows verify
Linux/macOS portability only. The GitHub `termux.yml` entry remains available
for an optional self-hosted runner, but release workflow does not claim that
external runner as release evidence.

### Repository Variable Format

Configure `vars.TERMUX_RUNNER_LABELS` as a JSON array of strings:

```json
["self-hosted", "termux"]
```

### Runner Security

- `persist-credentials: false` MUST be set on checkout steps.
- Self-hosted runners execute untrusted workflow code; ensure runner isolation.
- Runner must be ephemeral or fully reset after each job. Workflow cleanup removes only verifier temp directories; it does not clean arbitrary files or build output.

### Deferred GitHub Release Controls

These GitHub release controls are deferred until repository settings are configured:

- `release` environment has required reviewers and tag restrictions;
- OIDC and artifact attestations are enabled by repository/organization policy;
- `main` requires `ci-required` and release tags are protected from force updates.

These settings cannot be proven or created by repository files. They remain
release controls, not code blockers. Local Termux verification is the source of
Termux evidence until a separately registered self-hosted runner exists.
