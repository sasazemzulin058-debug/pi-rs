# U1 Session Checkpoint

Target: `pi-mono 0.83.0` (`f0deb8dd8e9611e89b5bc4145ca92c03ae6ed4ee`)
Status: COMPLETED_PENDING_REVIEW

## Linked reviews & plans
- Review 3 (BLOCKED): `/data/data/com.termux/files/home/.pi-subagents/u1-review3.md`
- Fix4 Plan: `/data/data/com.termux/files/home/.pi-subagents/u1-fix4-plan.md`

## Resolution summary
- Fixed Review3 Blocker 1 & 2: Added `details: Option<serde_json::Value>`, `usage: Option<serde_json::Value>`, and `from_hook: Option<bool>` to `SessionEntry::Compaction` and `SessionEntry::BranchSummary`.
- Lossless direct serde & JSONL import/save/reload preservation of compaction & branch_summary `details`, `usage`, and `fromHook` metadata fields.
- Rejection of malformed `fromHook` non-boolean values in direct serde deserialization and line-numbered errors in JSONL import.
- Added full `serde_json::Value` round-trip tests and JSONL import/save/reload metadata preservation tests in `crates/pi-coding-agent/tests/session_v3_tree.rs`.
- Fixed prior Blockers 1-4: ISO string timestamps, camelCase serde aliases in `pi-ai`, compaction history exclusion without `firstKeptEntryId`, strict parsing rejecting malformed/null fields.

## Evidence
- `cargo test --locked -p pi-coding-agent --test session_v3_tree`: PASS (29/29 passed)
- `cargo test --locked -p pi-coding-agent`: PASS
- `cargo test --locked --workspace --all-targets`: PASS
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS
- `cargo fmt --check`: PASS
- `python3 -m unittest discover -s tests/contract -p 'test_*.py'`: PASS (52/52 passed)
- `sh ./scripts/verify-termux`: PASS (13/13 comparator cases pass)
- `git diff --check`: PASS

## Exit gates status
- Lossless retainedTail wire storage: DONE
- Lossless ordinary message wire storage: DONE
- Strict parsing (no silent `.ok()`, no null retainedTail/message): DONE
- Upstream-shaped JSONL round-trip/negative tests & full Value equality: DONE
- ISO entry timestamp preservation on import: DONE
- camelCase serde aliases for assistant/toolResult wire fields & usage counters: DONE
- `mimeType` image projection: DONE
- `StopReason::Pending` support: DONE
- Compaction history exclusion without firstKeptEntryId: DONE
- Compaction & BranchSummary `details`, `usage`, `fromHook` metadata preservation: DONE
- Termux verify 13/13: DONE


