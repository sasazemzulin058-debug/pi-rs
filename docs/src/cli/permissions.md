# Permissions

`pi-agent` ships a `PermissionPolicy` trait with three decisions: `Allow`,
`AllowSession`, and `Deny`. Tools that advertise `requires_permission()`
(by default `bash`, `write`, and `edit`) are routed through the policy
before they run. Read-only tools (`read`, `ls`, `grep`, `glob`,
`web_fetch`) bypass it.

In the CLI this surfaces as an interactive prompt:

```text
Tool: bash
Args: { "command": "cargo test" }
[a]llow once, [s]ession-allow, [d]eny ?
```

Choosing **session-allow** caches the decision in memory for the
remainder of the run; the next invocation of the same tool kind skips
the prompt.

## `--yolo`

Pass `--yolo` to install a permissive policy that auto-allows everything,
including `bash`, `write`, and `edit`. Use with care; this hands the
agent a loaded gun.

```bash
pi-rs --yolo -p "Run the test suite and fix any failures"
```
