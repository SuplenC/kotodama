# kotodama-hyprland code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`: an `events` thread follows Hyprland's event socket, and every change goes out through `SentItems::publish`. Outside Hyprland it sends nothing. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `hyprland.rs`: `Hyprland::from_env` (the socket directory), `request` (one `.socket.sock` request, as `hyprctl` writes it), `dispatch`, `events`; `Events::next_change` (`.socket2.sock` lines).
- `events.rs`: `Change::parse` (the events that change an Item: `openwindow`, `closewindow`, `movewindowv2`, `windowtitlev2`, `configreloaded`).
- `windows.rs`: `Windows` (`from_clients` for `j/clients`, `apply`, `items`), `dispatch` (what each window Action runs); `FOCUS`, `CLOSE`, `MOVE`, `FLOAT`, `WORKSPACE`.
- `binds.rs`: `Bind`, `parse` (`j/binds`), `items` (binds with a description), `view` (the Shortcuts View, grouped by modifier), `find`, `shortcuts_item` (what `--view hyprland/shortcuts` names); `RUN`, `OPEN`, `VIEW`, `KIND`.
- `../tests/fixtures/`: `hyprctl binds -j` and `clients -j` output and an event stream, in Hyprland's formats. `../tests/binds.rs`, `../tests/windows.rs`: parsing, grouping and events against them. `../tests/extension.rs`: the Extension over stdio against a fake Hyprland serving both sockets, and without Hyprland.
- `../../../nix/tests/hyprland.nix`: the live tests, in a NixOS VM running Hyprland.
