# kotodama-extension code map

The runtime every Rust Extension is built on: the Bundled extensions, and any installed separately. The Daemon uses its watcher. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: `main` (lean memory, log name, exit code), `connect` (handshake and the `read` thread) returning `Connection`; `Event<T>`, `Writer`, `lean_memory`, `log!`, `STACK_SIZE`. The crate docs hold a whole example Extension.
- `items.rs`: `SentItems::publish` (send only the Items added, changed or gone).
- `config.rs`: `command`, `string`, `boolean`, `strings`, `warn` for an Extension's config table.
- `dirs.rs`: `state`, `cache` (XDG directories for an Extension).
- `url.rs`: `fill` (a `{query}` template, percent-encoded), `opener` (`xdg-open`), `percent_encode`; shared by the Daemon's Quicklinks and web search.
- `launch.rs`: `launch` (detached), `background` (reports `Ended` with the last output line); both pass `WAYLAND_DISPLAY` on (`runtime::pass_wayland_display`).
- `watch.rs` (feature `watch`): `Watcher::start` (settles on its own thread), `Watcher::new` (no thread), `Watcher::watch`; missing directories are watched through the deepest directory on their path that exists, removed or renamed ones are forgotten. `links_to` finds the symlinks on the way to each directory (the profiles a NixOS or home-manager switch replaces); `watch_locked` watches the directory each sits in, and `Watched::forget_relinked` has the directories behind one replaced watched again where it leads now.
- `runtime.rs` (Linux): for compositors that start after the Daemon (see its docs). `dir` (`$XDG_RUNTIME_DIR`), `wait_for` (retries until something turns up in watched directories: an inotify watch, else a doubling backoff, reset by a change; used by `wayland::Display` and `kotodama-hyprland`), `wayland_sockets`, `pass_wayland_display` (gives launched programs and Extensions the `WAYLAND_DISPLAY` the Daemon lacks).
- `wayland.rs` (feature `wayland`, Linux; the Daemon and the clipboard Extension): `wayland/display.rs:Display` (`WAYLAND_DISPLAY`, else the `wayland-N` sockets in the runtime dir; `connect`, `wait`), `wayland/data_control.rs:DataControl` (binds ext-data-control, else wlr; `set_selection`), `DataControlHandler` (`selected`, `finished`), `delegate_data_control!`.
- `testing.rs` (feature `testing`, dev-dependencies only): the Bundled extensions' shared test harness. `Spawned` runs an Extension over its stdio as the Daemon does (`added`, `removed`, `quiet`, `start_run`, `outcome`; killed on drop); `install`, `bundled` and `connect` set up the in-process Daemon and a Front end on its socket.
- `tests/watch.rs`: the watcher against the real file system.
