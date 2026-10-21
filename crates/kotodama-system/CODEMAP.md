# kotodama-system code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`: it reads `CanHibernate` once, sends the power Items and says it can do something, then a `toplevels` thread waits for the compositor and reports its windows, which go out through a `SentItems::publish` of their own, so a window change sends only that window. The three power Items that end the session carry `confirm` on their Action, so the Front end asks before the run reaches the Extension. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `power.rs`: `Power` (`ALL`, `id`, `from_id`, `title`, `asks_first`, `item`), `items` (Hibernate only when logind can); `KIND`, `RUN`.
- `logind.rs`: `run` (the logind method each `Power` calls, with `interactive` false or the session `auto`), `method`, `can_hibernate`, `Error`; `DESTINATION`, `PATH`, `MANAGER`, `AUTO`. Every call opens the system bus (`kotodama_extension::dbus::Bus::system`) and hangs up.
- `windows.rs`: `Windows` (`update`, which says whether the Item changed, `remove`, `clear`, `items`, `request`), `Window`, `Request`, `RequestError`; `FOCUS`, `CLOSE`, `MINIMIZE`, `MAXIMIZE`, `FULLSCREEN`.
- `toplevels.rs` (Linux): `follow` (the thread: waits on `kotodama_extension::wayland::Display`, binds wlr-foreign-toplevel-management, blocks on the connection, waits again once the compositor exits; ends on Hyprland, spotted by its `hyprland_` globals, or without the protocol), `Change`, `Compositor::request`.
- `../tests/logind.rs`: the Extension over stdio against a fake logind on a private `dbus-daemon`: which method each Item calls, the three that carry `confirm`, a call logind turns down, Hibernate only with `CanHibernate` `yes`, failures without logind.
- `../../../e2e/system/`: on headless sway, two windows found by title, focused, one closed, and the list following.
- `../../../nix/tests/hyprland.nix`: on Hyprland, the windows show once, the Hyprland Extension's.
