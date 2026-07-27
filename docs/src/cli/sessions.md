# Sessions

Every interactive turn is saved to disk under
`$XDG_CONFIG_HOME/pi/sessions/<id>.json` (typically
`~/.config/pi/sessions/` on Linux/macOS; candidate until Pi fixtures pass). Sessions contain the full
message transcript and can be reloaded later.

## Subcommands

```bash
pi-rs sessions list           # list saved sessions (id, model, last updated)
pi-rs show <id>      # print a session's transcript
pi-rs delete <id>    # delete a saved session
```

## Resume

To continue a previous conversation, pass `--resume <id>`:

```bash
pi-rs --resume 0193abcd-...
```

Inside the REPL you can also use `/resume <id>` to swap to a saved
session, or `/session` to print the current session id.
