# Sessions

Interactive turns are persisted in the application's resolved configuration root.
With the standard XDG configuration root, the active session file is
`$XDG_CONFIG_HOME/pi-rs/sessions/<id>.jsonl` (typically
`~/.config/pi-rs/sessions/<id>.jsonl` on Linux). The active contract is a
fork-native, versioned JSONL append/recovery format; it is not claimed to be
compatible with upstream Pi v3 output.

A save appends new transcript entries while preserving the bytes of the
previously committed prefix. Loading may recover an incomplete final JSONL
record by truncating only that tail. Malformed complete records, including
records in the middle of a file, remain errors.

Legacy `<id>.json` files are accepted as read-only migration input. Upstream Pi
v1, v2, and v3 session files are also imported read-only and are never mutated;
the first mutation is saved as a separate fork-native copy-on-write session.

## Subcommands

```bash
pi-rs sessions list  # list saved sessions (id, model, last updated)
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
