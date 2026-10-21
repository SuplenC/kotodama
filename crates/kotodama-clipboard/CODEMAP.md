# kotodama-clipboard code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension` and fails an Action with `RunError`; while paused it drops what is copied, and it says so in its status (`SentStatus::noted`); History lives in `dirs::state("clipboard")`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `wayland.rs` (`Clipboard::watch`: waits for the compositor, then watches through `kotodama_extension::wayland`; `set`, used by `examples/copy.rs`; Copy to Clipboard returns the `copy` Outcome instead; `Change`, `WatchError`, `SetError`), with the Linux threads in `wayland/linux.rs`
- `copied.rs` (MIME choice, `Copied`, `ReadError`)
- `history.rs` (`History` on disk, `Limits` and their `DEFAULT_…` consts)
- `items.rs` (Items, list and detail Views; the pause Item, `pause_item` and `paused_by`), with its unit tests in `items/tests.rs`
- `pause.rs` (`Pause`: whether a copy is kept, the note the app's intro card shows while paused, counting down the minutes, and the `paused` file in the state directory that keeps a pause through a restart), with its unit tests in `pause/tests.rs`
- `../tests/history.rs`: the history on disk through the library, on a made-up clock.
- `../tests/clipboard.rs`: the Extension over stdio, its history filled through the library; pausing and resuming, through a restart.
- `../benches/search.rs`: searching a 1000-entry clipboard history in the Daemon (p99 under 2 ms), plus the Extension's resident memory.
