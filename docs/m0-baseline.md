# M0.0 — Untouched fork baseline

> Historical audit evidence captured before the GitHub Actions-only verification policy. This is not a current CI gate; the canonical baseline must be reproduced and attached to a GitHub Actions run before M0 closes.

**Date:** 2026-07-27  
**Fork base:** `nktkt/pi` `v1.2.0`  
**Commit:** `0808c756fa1991940def7f0f9837464417149419`  
**Root commit:** `ed1d18da31d53c34b60238f06473c7b162dc30f8`  
**History:** full (unshallowed from `nktkt` remote before baseline capture)

## Environment

| Variable | Value |
|---|---|
| `rustc` | `rustc 1.96.1 (31fca3adb 2026-06-26) (built from a source tarball)` |
| `cargo` | `cargo 1.96.1 (356927216 2026-06-26) (built from a source tarball)` |
| `uname` | `Linux localhost 6.1.118-android14-11-o-g64180ab070e5 #1 SMP PREEMPT Fri Dec 12 12:32:56 UTC 2025 aarch64 Android` |
| `PREFIX` | `/data/data/com.termux/files/usr` |
| `TMPDIR` | `/data/data/com.termux/files/usr/tmp` |
| `HOME` | `/data/data/com.termux/files/home` |
| Target | `aarch64-linux-android` (Termux-native, bionic) |

## Baseline commands (untouched commit)

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --check` | passed | No formatting issues |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed | No warnings |
| `cargo test --workspace` | passed | 5 tests: 4 json_mode + 1 session_smoke |
| `cargo build --workspace --release` | passed | Historical untouched-fork binary: `target/release/pi`, ELF aarch64, 11.2 MiB |

## Inherited test summary

```
pi-agent tests: 0 (no tests in pi-agent crate)
pi-ai tests: 0 (no tests in pi-ai crate)
pi-coding-agent tests: 5 passed
  json_mode: 4 passed (agent_end_is_null, text_delta_shape, tool_start_shape, tool_end_shape)
  session_smoke: 1 passed (save_and_load_roundtrip)
```

## Shallow history resolution

The initial import used `--depth=1` and produced a shallow clone. Before baseline capture, `git fetch --unshallow nktkt` was run successfully, pulling full history from the `nktkt` remote. The repository now has complete history from root commit `ed1d18da31d53c34b60238f06473c7b162dc30f8` through `v1.2.0`.

## Disposition

All four baseline commands pass on the untouched fork commit. No inherited failures to disposition. The workspace compiles and tests cleanly on Termux-native `aarch64` with Rust 1.96.1.
