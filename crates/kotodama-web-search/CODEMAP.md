# kotodama-web-search code map

A Bundled extension, the fallback. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it a fallback Live query Extension (`[live_query] fallback = true`, debounce 0, triggers for what may be an address); `src/main.rs` runs the protocol loop on `kotodama-extension`, reads the `engines` setting, answers each Live query and opens an Item's URL. Which queries it is asked and where its Results rank is the Daemon's: `kotodamad/src/daemon.rs:Daemon::add_fallbacks` (ADR 0016). See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: `results` (a search per engine, and the address as the answer), `address` (what counts as an address; `GENERIC`, `FILES`), `Engine`, `engines` (the setting), `default_engines`, `command` (what `OPEN` runs). Templates and the opener are `kotodama_extension::url`.
- `tests.rs`: `lib.rs`'s unit tests (addresses, the Manifest's triggers, Results, the `engines` setting).
- `../tests/extension.rs`: the Extension over stdio (config, Live queries, opening).
- The ranking scenarios in `kotodamad/tests/ranking.rs` (V3) run its real Manifest and Results through the Daemon: never above a real match, not asked while real matches fill the rows, an address in the top row.
