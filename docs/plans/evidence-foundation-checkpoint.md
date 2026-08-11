# Evidence Foundation Execution Checkpoint

## Status: IN_PROGRESS (Resumed & Tasks 1-8 Completed)

- Resume attempt: Validated Tasks 1-5, cleaned trailing whitespace, verified full test suite.

## Scope

- `scripts/contract_fixture_lib.py`
- `scripts/validate-fixture-manifest`
- `scripts/compare-contract-fixtures`
- `scripts/capture-upstream-fixtures`
- `fixtures/upstream-pi/manifest.json`
- `fixtures/upstream-pi/*.raw-hash.txt`
- `tests/contract/test_validator.py`
- `tests/contract/test_manifest.py`
- `tests/contract/test_comparator.py`
- `tests/contract/test_capture.py` (new)
- `docs/plans/evidence-foundation-checkpoint.md`

## Tasks & Checkpoints

- [x] Task 1: Define schema-v2 canonical evidence model in `scripts/contract_fixture_lib.py`
- [x] Task 2: Migrate current manifest evidence metadata without relabeling fixtures
- [x] Task 3: Make validator enforce complete committed corpus
- [x] Task 4: Make comparator consume shared corpus validation
- [x] Task 5: Add explicit capture modes and exact-reference preflight to `scripts/capture-upstream-fixtures`
- [x] Task 6: Stage complete corpus and generate deterministic capture result
- [x] Task 7: Make explicit write transaction fail closed
- [x] Task 8: Freeze interface and run test suite validation

## Rules & Constraints

- Do NOT relabel 0.82.1.
- Do NOT modify Rust implementation.
- Do NOT delete agent leftovers.
- Preserve existing dirty state.
- Keep standard expected payloads unchanged; only update compact `.raw-hash.txt` sidecars matching canonical JSON formula.
