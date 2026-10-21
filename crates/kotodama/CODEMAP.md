# kotodama code map

The CLI: the command a compositor keybind runs, and `kotodama extension` for Extension authors. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `src/main.rs`: `parse` reads `kotodama toggle`, `kotodama open [--query TEXT] [--view EXTENSION/ITEM] [--app WORD]` (or `--prefix`, its old name) and `help` (`main` hands `check` to `check.rs` and `config` to `config.rs`); `connect` makes the connection, and `send` sends the one `FromFrontEnd` through the Daemon (ADR 0007), and `answer` reads how many menus it reached (ADR 0029): none is `SendError::NoMenu`, a hint to start one. `--view` names an Item whose primary Action pushes the View (ADR 0014). `ParseError` (printed above the usage, exit 2) and `SendError` (exit 1).
- `src/check.rs`: `kotodama check` prints the problems of the Daemon's first `Status` (`problems`, `print`) and exits 1 when there is any, 2 when it can't ask.
- `src/config.rs`: `kotodama config [--schema]` runs `kotodamad config` (beside the `kotodama` binary, else on PATH), which prints the merged config, or every key with its doc.
- `src/extension.rs`: `kotodama extension`, for Extension authors; `run` from `main` when the first argument is `extension`.
  - `src/extension/check.rs:check`: `kotodama extension check <dir> [--query TEXT] [--run ID]`. Reads the Manifest as `kotodamad/src/discovery.rs` does, starts the program, runs the Daemon's side of the handshake (`Run::start`), sends the config, a Live query and an Action, and hangs up; `Seen::take` says what the Daemon would refuse in each message (`Finding`) and mirrors the Daemon's rules, so a rule added there belongs here too. `Report` writes the plain-words report; a problem exits 1.
  - `src/extension/scaffold.rs:write`: `kotodama extension new <name> [--lang shell|python|rust] [--dir PARENT]`, from the templates in `src/extension/scaffold/` (`{{name}}` and `{{command}}` filled in).
  - Unit tests in `src/extension/tests.rs`, `src/extension/check/tests.rs`, `src/extension/scaffold/tests.rs`.
- `tests/extension.rs`: `check` on the three examples, on broken Extensions, and on each scaffold (the Rust one built against this checkout).
- `src/tests.rs`: `parse`'s unit tests, and `answer`'s over a socket pair. The command runs end to end in the e2e suite (`open-view`, `apps`, `menu`).
