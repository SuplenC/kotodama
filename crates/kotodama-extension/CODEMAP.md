# kotodama-extension code map

The runtime every Rust Extension is built on: the Bundled extensions, and any installed separately. The Daemon uses its watcher. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: `main` (lean memory, log name, exit code), `connect` (handshake and the `read` thread) returning `Connection`; `Event<T>`, `Writer`, `lean_memory`, `log!`, `STACK_SIZE`. The crate docs hold a whole example Extension.
- `items.rs`: `SentItems::publish` (send only the Items added, changed or gone).
- `config.rs`: `command`, `string`, `boolean`, `strings`, `warn` for an Extension's config table.
- `dirs.rs`: `state`, `cache` (XDG directories for an Extension).
- `url.rs`: `fill` (a `{query}` template, percent-encoded), `opener` (`xdg-open`), `percent_encode`; shared by the Daemon's Quicklinks and web search.
- `launch.rs`: `launch` (detached), `background` (reports `Ended` with the last output line).
- `watch.rs` (feature `watch`): `Watcher::start` (settles on its own thread), `Watcher::new` (no thread), `Watcher::watch`; missing directories are watched through their parent, removed or renamed ones are forgotten. #50's symlink fix goes in `watch_locked`.
- `testing.rs` (feature `testing`, dev-dependencies only): the Bundled extensions' shared test harness. `Spawned` runs an Extension over its stdio as the Daemon does (`added`, `removed`, `quiet`, `start_run`, `outcome`; killed on drop); `install`, `bundled` and `connect` set up the in-process Daemon and a Front end on its socket.
- `tests/watch.rs`: the watcher against the real file system.
