# kotodama-apps code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`, including its `Watcher` for the application dirs. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `scan.rs` (`scan`, `application_dirs`)
- `entry.rs` (`.desktop` parsing)
- `exec.rs` (`Exec` field codes)
- `items.rs` (`Settings`, Items)
