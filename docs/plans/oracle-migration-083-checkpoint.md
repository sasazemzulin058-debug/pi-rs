# Oracle Migration 0.83 Checkpoint

## Recovery checkpoint (2026-08-10)

- Worker resumed after 3 configured-model failures. No fixture or manifest publication performed.
- Current lane: isolate exact 0.83 capture blockers using detached staging/worktree; preserve 0.82.1 oracle.
- Scope: oracle/capture files only. No GitHub operations, commit, push, or upstream checkout mutation.

- Target version: `0.83.0`
- Target commit: `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`
- Status: IN_PROGRESS at OM-8 (0.83 oracle published locally; comparator and local Termux pass; hosted CI publication gate pending)
- Start state: repository has no tracked implementation diff; checkpoint and `.pi-subagents/` are untracked. Upstream checkout has pre-existing untracked temporary directories; left unchanged.

## Steps & Progress

- [x] OM-1: Confirm target commit `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee` as 0.83.0 oracle reference (commit exists; package reports 0.83.0).
- [x] OM-2: Diff semantic changes between 2efa728d and f0deb8dd in upstream pi-mono (diff inspected; changes span `packages/ai` and `packages/coding-agent`).
- [x] OM-3: Verify pi-mono checkout at f0deb8dd reports version 0.83.0.
- [x] OM-4: Recapture upstream oracle fixtures at f0deb8dd (runs `31418670026` and `31418780296` passed; staged manifest validates; digest deterministic `sha256:5d2bb11548020fbe246fbd7aa78ed76f94644a4babe8faca67c7a04c2613022a`).
- [x] OM-5: Review semantic fixture diff; M1a expected/hash payloads unchanged, target manifest metadata/digest changed, and obsolete actual sidecars absent from staged artifact.
- [x] OM-6: Update manifest.json to 0.83.0 target, lock SHA, and staged M1a corpus digest.
- [x] OM-7: Update `.github/workflows/capture-reference.yml` pinned commit and lock SHA to exact 0.83.0 target; parent GitHub run `31414297025` verified checkout, npm hydration, and generated model JSON.
- [x] OM-8: Re-run M1a comparisons; local `sh ./scripts/verify-termux` passes all 13/13 M1a cases against published 0.83 manifest.
- [x] Capture safety gates: explicit nonexistent `--fixtures-dir` fails closed; non-empty `--staging-dir` rejected; staging capture report declares `mode: staging`; focused negative/report tests added.
- [ ] OM-9: Ensure version-independent case-set validation in `contract_fixture_lib.py` (defer until post-publication review).

## Capture attempt

Command:

```text
cd /data/data/com.termux/files/home/pi-rs-main-audit && PI_UPSTREAM_ROOT=/data/data/com.termux/files/home/pi-mono python3 scripts/capture-upstream-fixtures --milestone M1a
```

Result: exit 1. Capture failed before staging/publishing fixture or manifest updates. Exact errors included repeated extension-load failures because `/data/data/com.termux/files/home/pi-mono/packages/ai/src/providers/data/amazon-bedrock.json` starts with `var bedroc` and is not valid JSON, followed by `Unknown provider "faux". Use --list-models to see available providers/models.`

## Residual Blockers

- Original checkout blocker confirmed: `/data/data/com.termux/files/home/pi-mono/packages/ai/src/providers/data/amazon-bedrock.json` starts with `var bedroc...`; this checkout remains untouched.
- Isolated worktree created at `/data/data/com.termux/files/home/pi-mono-oracle-083`, exact target `f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`, clean tracked tree before hydration. Target lock SHA: `cc00fd4bfdc43f30ac7892de8dd46e5108c8b966b4941683bac42ee0587771d7`; remote `https://github.com/earendil-works/pi.git`; package versions `0.83.0` for `packages/ai` and `packages/coding-agent`.
- Isolated hydration command `npm run hydrate-model-data --workspace=@earendil-works/pi-ai` succeeded on current Termux device and generated valid model JSON. It does not prove executable capture: `npm ci` fails at native `canvas` (`No prebuilt binaries found ... arch=arm64 ... android`; missing `pangocairo`), leaving no usable `node_modules/tsx`.
- Capture preflight passed exact SHA/version/lock/clean-tree/Node checks, then first executable adapter failed: `cli.print.basic` cannot resolve packages (`ERR_MODULE_NOT_FOUND`). No adapter output was published.
- Node dependency isolation in `/data/data/com.termux/files/home/pi-mono-oracle-083` blocks local execution (`cross-spawn` and `tsx` resolution failure without full `npm ci` node_modules).
- Hosted capture workflow `.github/workflows/capture-reference.yml` remains authoritative environment for complete npm hydration and execution.
- Faux-provider registration remains untested after dependency hydration. Do not repair or relabel without runnable dependency environment and successful capture.
- Manifest/version/fixtures remain unchanged. Old 0.82.1 fixtures not relabeled.

## Recovery diagnostics

```text
Isolated worktree: /data/data/com.termux/files/home/pi-mono-oracle-083
HEAD: f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee
npm ci: FAIL (native canvas; Android arm64; pangocairo unavailable)
hydrate-model-data: PASS (model data generated in isolated worktree)
capture preflight: PASS
first adapter: FAIL cli.print.basic, ERR_MODULE_NOT_FOUND: tsx
publication: NONE
```

Capture command used:

```sh
PI_UPSTREAM_ROOT=/data/data/com.termux/files/home/pi-mono-oracle-083 \\
python3 scripts/capture-upstream-fixtures --milestone M1a \\
  --reference-commit f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee \\
  --reference-version 0.83.0 \\
  --reference-lockfile-sha256 cc00fd4bfdc43f30ac7892de8dd46e5108c8b966b4941683bac42ee0587771d7
```

## Validation

- `python3 -m unittest discover -s tests/contract -p 'test_*.py'` → `51/51` passed after staging and isolated-validator tests.
- `python3 scripts/validate-fixture-manifest --milestone M1a` → `Manifest validation PASSED`.
- `python3 scripts/check-doc-consistency` → passed.
- `sh ./scripts/verify-termux` → 13/13 cases passed.
- Hosted capture runs `31418670026` and `31418780296` passed with identical target corpus digest.
- Downloaded staged 0.83 manifest validation passed.
- Local committed oracle remains unchanged at 0.82.1.
- Post-gate validation: contract tests 52/52, manifest validation passed, doc consistency passed, Termux verification passed.
