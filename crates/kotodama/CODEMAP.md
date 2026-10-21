# kotodama code map

The CLI. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `src/main.rs`: `parse` reads `kotodama toggle`, `kotodama open [--query TEXT] [--view EXTENSION/ITEM] [--prefix PREFIX]` and `help`; `send` sends the one `FromFrontEnd` through the Daemon (ADR 0007). `--view` names an Item whose primary Action pushes the View (ADR 0014). `ParseError` (printed above the usage, exit 2) and `SendError` (exit 1).
- `src/tests.rs`: `parse`'s unit tests. The command runs end to end in the e2e suite (`open-view`, `prefix`, `menu`).
