# Interactive

Running `pi-rs` with no arguments drops you into the REPL. Assistant text
streams in as it arrives; tool calls show a confirmation prompt by default
(see [Permissions](./permissions.md)). After every turn the session is
persisted to disk (see [Sessions](./sessions.md)).

```bash
pi-rs
```

## Slash commands

```text
/help                show command list
/quit  /exit         quit pi-rs
/reset               start a fresh session
/model               print the active model
/tools               list builtin tools
/cost                show accumulated token usage
/sessions            list saved sessions
/resume <id>         load a saved session by id
/session             print current session id
```

The REPL also loads `AGENTS.md`, `CLAUDE.md`, and `.pi/instructions.md`
from the current directory upward and concatenates them into the system
prompt.
