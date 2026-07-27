# Print mode

`pi-rs -p <prompt>` runs the agent once and prints its output to stdout, then
exits. It is the scriptable counterpart to the interactive REPL.

```bash
pi-rs -p "List the .rs files in this directory and summarize them"
```

## JSON mode

Pass `--json` to emit one structured JSON event per line on stdout instead
of human-readable text. The `--json` schema is covered by semver from
1.1.0 onward: new fields may be added, but existing ones will not be
renamed without a major bump.

```bash
pi-rs -p "Run the test suite" --json
```

## Event types

| Type | When |
|------|------|
| `agent_start` | Agent loop begins. |
| `turn_start` | A new turn begins. |
| `turn_end` | A turn finished. |
| `user_message` | A user message was appended. |
| `assistant_message` | The assistant finished a message. |
| `text_delta` | Streaming assistant text chunk. |
| `thinking_delta` | Streaming reasoning chunk (when supported). |
| `tool_start` | A tool call is about to execute. |
| `tool_end` | A tool call finished. |
| `permission_denied` | A gated tool was denied by `PermissionPolicy`. |
| `agent_end` | Loop returned; includes `stopped_at_turn_limit` and `message_count`. |
