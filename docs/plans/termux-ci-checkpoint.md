# Termux CI Wiring Checkpoint

## Status: BLOCKED

- Task: Resume D4 Termux CI wiring (attempt 2/3) from existing state.
- Authoritative job: `.github/workflows/termux.yml` job `termux`.
- Trusted invocation policy:
  - Allow `push` to `main`;
  - Allow release tag workflow through reusable `ci.yml`;
  - Allow same-repository PRs;
  - Allow reviewed `workflow_dispatch` for explicit SHA;
  - Never run fork PR code automatically;
  - Never use `pull_request_target` to execute PR checkout;
  - Fork PR remains blocked from `ci-required` until maintainer imports reviewed commit into same-repository branch or runs approved exact-SHA dispatch.
- Requires protected `termux-attestation` environment for self-hosted job.
- Requires dedicated disposable or snapshot-reset Termux runner, no release secrets, no developer credentials, no shared workspaces, minimal outbound access.
- Local Termux device evidence vs GitHub self-hosted gate:
  - Local Termux device runs `sh ./scripts/verify-termux` automatically and passes all local checks (49 python contract tests + cargo test suite + M1a contract fixtures pass).
  - GitHub self-hosted runner gate remains BLOCKED pending external GitHub environment setup (`termux-attestation` environment + self-hosted runner labeled `["self-hosted", "termux"]` + `vars.TERMUX_RUNNER_LABELS`).
  - Observed CI run `31413191171` (`commit 39a4941`): standard hosted test/docs/supply-chain jobs pass, `runner-config` fails fast with `vars.TERMUX_RUNNER_LABELS empty` as expected for missing external configuration.
- Current evidence boundary: fixture oracle remains `0.82.1`; D4 completion proves CI wiring and current corpus only, not `0.83.0` parity.

## Tasks & Checkpoints

- [x] Task 1: Freeze D4 trust and execution contract (`docs/plans/termux-ci-checkpoint.md`)
- [x] Task 2: Make reusable Termux workflow accept and verify explicit source SHA
- [x] Task 3: Call authoritative Termux workflow from required CI
- [x] Task 4: Prove release remains downstream of Termux without duplicate runner execution
- [x] Task 5: Add structural workflow contract tests with mutation coverage
- [x] Task 6: Validate repository behavior locally before GitHub run
- [ ] Task 7: Configure GitHub trust boundary before enabling required job
- [ ] Task 8: Run authoritative GitHub Actions verification
- [ ] Task 9: Close checkpoint using evidence-only protocol

## Verification Log

- Initialized checkpoint attempt 3/3.
- Verified local Termux environment execution via `sh ./scripts/verify-termux` (all Rust unittests/integration tests passed + M1a fixtures 13/13 passed; 49 contract tests passed).
- Status set to `BLOCKED` awaiting GitHub self-hosted runner environment and GH Actions execution.
