# kotodama-apps code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`, including its `Watcher` for the application dirs. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `scan.rs` (`scan`, `application_dirs`)
- `entry.rs` (`.desktop` parsing: `parse`, `Locale`), with its unit tests in `entry/tests.rs`
- `exec.rs` (`Exec` field codes: `command`, `Error`), with its unit tests in `exec/tests.rs`
- `items.rs` (`Settings`, Items)
- `main.rs`: `Extension::run` launches with `launch_watched`; an app whose whole launch closes within `withdraw_within`, with no copy of its program running from before, sends `WithdrawPick` (`Own::ExitedSoon`). `RunError` is why an Action failed, the failed Outcome's message.
- `../tests/apps.rs`: the Extension over stdio, on temporary XDG data directories.
