# pi-coding-agent

Interactive coding agent CLI. Part of the
[`pi-rs`](https://github.com/nktkt/pi) agent harness — a Rust port of
[`@earendil-works/pi-coding-agent`](https://github.com/earendil-works/pi).

The crate name is `pi-coding-agent`; the installed binary is `pi-rs`.

## Install

```bash
cargo install pi-coding-agent
```

## Usage

```bash
export ANTHROPIC_API_KEY=sk-ant-...
# or: OPENAI_API_KEY=..., GOOGLE_API_KEY=..., GEMINI_API_KEY=...

# Interactive REPL
pi-rs

# One-shot prompt
pi-rs -p "List the .rs files in this directory and summarize them"

# JSON-lines event stream for scripting
pi-rs -p "..." --json

# Resume a saved session
pi-rs --resume <id>

# Skip permission prompts (DANGEROUS — bash/write/edit run unconfirmed)
pi-rs --yolo -p "Run the test suite and fix any failures"

# Session management
pi-rs sessions list
pi-rs sessions show <id>
pi-rs sessions delete <id>
```

Pick the model explicitly:

```bash
PI_MODEL=claude-opus-4-7   pi-rs -p "..."
PI_MODEL=gpt-4o            pi-rs -p "..."
PI_MODEL=gemini-2.0-flash  pi-rs -p "..."
```

## Slash commands (interactive)

```
/help                show command list
/quit  /exit         quit pi
/reset               start a fresh session
/model               print the active model
/tools               list builtin tools
/cost                show accumulated token usage
/sessions            list saved sessions
/resume <id>         load a saved session by id
/session             print current session id
```

## What's included

- Streaming text — assistant output prints as it arrives.
- **Session persistence** under `$XDG_CONFIG_HOME/pi/sessions/<id>.json`,
  saved after every turn.
- **AGENTS.md / CLAUDE.md / `.pi/instructions.md`** loader walks up from the
  current directory and concatenates each file into the system prompt.
- **Per-tool permission prompts** — `bash`, `write`, `edit` require
  confirmation by default. `--yolo` skips all prompts.
- Built on [`pi-ai`](https://crates.io/crates/pi-ai) and
  [`pi-agent`](https://crates.io/crates/pi-agent), so any code consuming
  these crates speaks the same protocol as the CLI.

## License

MIT
