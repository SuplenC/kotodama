# kotodama-ai code map

A Bundled extension: turns a request in plain words into Proposals, Actions from the Action catalog the user confirms before anything runs. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` reads the Action catalog (`action_catalog = true`) and declares a Live query on the `?` prefix plus a Fallback for the "Ask AI" row. Running a confirmed Proposal is the Daemon's: `kotodamad/src/daemon.rs:Daemon::run_catalog_action` (ADR 0019). See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `main.rs`: the protocol loop: config, the catalog, the `?` debounce, one request in flight at a time (a new query or a cancel kills it), the "Ask AI" row and its View, and confirming a Proposal as `Outcome::RunCatalogAction`.
- `settings.rs`: the `[extensions.ai]` settings (`api_key_command`, `url`, `model`, `effort`, `debounce_ms`) and their `DEFAULT_`s.
- `key.rs`: `Secret` (prints `<redacted>`) and `read_key`, which runs `api_key_command`.
- `request.rs`: the request body (the query and the catalog, nothing else), the curl config on stdin with the key, `post` and `InFlight` (cancel).
- `proposal.rs`: reading the model's `propose_actions` call against the catalog, `Proposal` and its Item, the "Ask AI" row.
- `*/tests.rs`: unit tests per module; `../tests/extension.rs`: the Extension over stdio with a fake `curl` on `PATH` (no key, no network; nothing runs before a confirm; the body; the key off the log and argv; cancelling; the View).
