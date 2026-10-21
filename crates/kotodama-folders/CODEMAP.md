# kotodama-folders code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `settings.rs` (`Settings`, the `DEFAULT_…` commands, `fill`; `FolderError` is why a `folders` entry is left out), with its unit tests in `settings/tests.rs`
- `projects.rs` (git repos, tms names), with its unit tests in `projects/tests.rs`
- `scripts.rs` (executables and header options), with its unit tests in `scripts/tests.rs`
- `scan.rs` (`Scanner` thread, settling changes from a thread-less `Watcher::new`)
- `items.rs` (the Items and their Action ids)
- `main.rs`: the protocol loop; a script without a terminal runs as a Job. `RunError` is why an Action failed, the failed Outcome's message.
- `../tests/folders.rs`: the Extension over stdio, on a temporary home.
- `../tests/projects.rs`: finding projects and naming them as `tms` does.
- `../tests/daemon.rs`: the Extension under the real Daemon, with the shell example.
- `../tests/support/git.rs`: making git repositories, shared by the three tests.
