# Introduction

`pi-rs` is a Rust port of [`earendil-works/pi`](https://github.com/earendil-works/pi),
a coding-agent harness originally written in TypeScript. This port covers the
agent runtime, the multi-provider streaming LLM API, and the CLI end-to-end,
and ships a working `pi-rs` binary that talks to real Anthropic, OpenAI, Google,
and any OpenAI-compatible endpoint.

The project is split into three crates published on crates.io:

- [`pi-ai`](https://crates.io/crates/pi-ai) — provider-agnostic SSE streaming
  for Anthropic Messages, OpenAI Chat Completions, and Google Generative AI.
- [`pi-agent`](https://crates.io/crates/pi-agent) — agent loop, permission
  policy, and built-in tools (`read`, `write`, `edit`, `bash`, `ls`, `grep`,
  `glob`, `web_fetch`, `todo`).
- [`pi-coding-agent`](https://crates.io/crates/pi-coding-agent) — the `pi-rs`
  CLI: print mode, interactive REPL, session persistence, slash commands.

Source: <https://github.com/nktkt/pi>. Upstream:
<https://github.com/earendil-works/pi>.
