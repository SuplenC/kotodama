# kotodama-calculator code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension` and reads `Settings`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs` (`Calculator::evaluate`)
- `math.rs` (fend)
- `dates.rs` (dates, time zones)
- `rates.rs` (ECB rates fetch and cache, in `dirs::cache("calculator")`)
