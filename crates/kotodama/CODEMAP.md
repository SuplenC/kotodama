# kotodama code map

The CLI. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `src/main.rs:parse`: `kotodama toggle`, `kotodama open [--query TEXT] [--view ID]`, `help`; sends one `FromFrontEnd` through the Daemon (ADR 0007).
