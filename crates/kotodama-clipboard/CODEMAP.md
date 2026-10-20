# kotodama-clipboard code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`; History lives in `dirs::state("clipboard")`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `wayland.rs` (`Clipboard::watch`: waits for the compositor, then watches through `kotodama_extension::wayland`; `set`, used by `examples/copy.rs`; Copy to Clipboard returns the `copy` Outcome instead)
- `copied.rs` (MIME choice, `Copied`)
- `history.rs` (`History` on disk, `Limits`)
- `items.rs` (Items, list and detail Views)
