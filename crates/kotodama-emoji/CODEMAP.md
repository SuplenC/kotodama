# kotodama-emoji code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`, sends every Item once at start, and reads the `skin_tone` setting. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs`: `Character` (`all`, `find`, `item`, `toned`), `Tone`, `items`, `run` (the Outcome of an Action); `TYPE`, `COPY`, `KIND`.
- `../data/emoji.tsv`: the characters, built into the binary with `include_str!` and read in place. Generated: run `data/update.sh`, which pins the Unicode Emoji and CLDR versions, to update it.
- `../data/keys.tsv`: keyboard symbols CLDR doesn't name (⌘ ⌥ ⌃) and key names for those it does (⇧ shift). Edit by hand, then run `data/update.sh`.
- `../data/LICENSE-UNICODE`: the license of the Unicode and CLDR data.
- `../tests/extension.rs`: the Extension over stdio (Items, Actions, skin tones). `../tests/search.rs`: searches and Frecency through the real Daemon, with `../tests/menu/mod.rs`, which `../benches/search.rs` (keystroke p99, resident memory) shares.
