# Code map

Where things are, so you can open the right file without searching. This map is
maintained by hand: if a line here is wrong, fix it in the PR where you notice.
Terms (Item, Tier, Frecency, View, ...) are defined in [GLOSSARY.md](GLOSSARY.md);
decisions are in [docs/adr/](docs/adr/). References are `path:symbol`.

## Layout

- `crates/kotodamad/`: the Daemon. Index, ranking, History, sessions, Extension host, socket server.
- `crates/kotodama-protocol/`: messages, framing, handshake and Wire formats shared by everything.
- `crates/kotodama/`: the `kotodama` CLI (`toggle`, `open`) that a compositor keybind runs.
- `crates/kotodama-qml/`: the Quickshell Front end, a Rust cdylib QML module (`Kotodama`) plus `qml/KotodamaMenu.qml`.
- `crates/kotodama-apps/`: Bundled extension, desktop apps from `.desktop` files.
- `crates/kotodama-calculator/`: Bundled extension, math, units, currency, dates.
- `crates/kotodama-clipboard/`: Bundled extension, clipboard history.
- `crates/kotodama-folders/`: Bundled extension, git projects and scripts from configured folders.
- `e2e/`: end-to-end tests against headless sway (`e2e/run`, `e2e/lib.sh`, one dir per test).
- `nix/`: `package.nix` (the package and Bundled extension layout), `home-manager.nix`, `tests/` (NixOS VM and QML module checks).
- `scripts/`: dev environment (`dev-env`, `dev-image`), bench runners (`bench-run`, `bench-compare`), `has-crates`/`has-benches`, `qml-module`, `tests/` (script tests).
- `docker/`: the dev image for machines without Nix (`just image`).
- `examples/shell-extension/`: a minimal Extension in shell, also run by `kotodama-protocol/tests/shell_extension.rs`.
- `docs/adr/`: decisions. `docs/agents/`: how agents use issues, labels and domain docs.
- Root: `flake.nix` (dev shell, packages, `checks`), `justfile`, `release-please-config.json`, `GLOSSARY.md`.

## kotodamad (the Daemon)

- `lib.rs:run`: wires paths, config, discovery, watchers, server and the main loop.
- `main.rs`: binary entry; calls `run`.
- `daemon.rs`: the state machine. `Daemon::handle(Event)` is the one entry point; `Event`, `Output`, `Launcher`/`Desktop` traits.
  - `Candidate` and `Candidate::rank`: the full Result order (Tier, then Learned choices/Frecency, then kind order; ADR 0006, 0011).
  - `Session`: per-Front-end state (query, shown Results, open Views); `FromFrontEnd::Viewport` handling keeps visible rows stable (ADR 0003).
  - `Daemon::query`, `Daemon::empty_query`: searching, and what shows before typing.
  - Own Results under `OWN = "kotodama"` (ADR 0005): `FORGET`, `CLEAR_HISTORY`, `UNDO` (ADR 0010). Limits `MAX_RESULTS`, `MAX_LIVE_RESULTS`.
- `rank.rs`: match quality. `Tier`, `Query` (folding, `searches_subtitles`), `Haystack`, `fold_text`.
- `index.rs`: `Index`, every Indexed item in memory, searched with nucleo; `keyed` (keywords), `identified` (shared identities, ADR 0008).
- `history.rs`: `History` on disk (`$XDG_STATE_HOME/kotodama/history.json`); `Habit::frecency`, `History::learned` (Learned choices), `Settings::from_config`.
- `clock.rs`: `Clock`, `When` (time of day for Frecency, ADR 0009).
- `kind_order.rs`: `KindOrder::place`, configurable kind order for equal matches.
- `empty_query.rs`: `EmptyQuery::from_config`, the empty-query menu.
- `keywords.rs`: Aliases and Quicklinks from config (`Keywords`, `Quicklink::command`, `percent_encode`).
- `arguments.rs`: text after a keyword filling an Action's Arguments, and the form for missing ones.
- `config.rs`: `load` (merge every `*.toml` in config dirs), `watch`, `extension_table` (`[extensions.<name>]`).
- `discovery.rs`: `discover` (`<dir>/*/manifest.toml`), `watch` for Extension dirs; `Found`.
- `host.rs`: `Processes`, spawning Extension processes with reader/writer threads.
- `link.rs:pump`: post-handshake connection threads (Extension or Front end).
- `server.rs`: `listen`, `serve`, the Front end Unix socket.
- `desktop.rs`: paste/type worker over Wayland (virtual keyboard, `Keymap`, `Wayland`, `Worker`).
- `notify.rs:send`: desktop notifications over D-Bus.
- `paths.rs:Paths::from_env`: socket, config and Extension dirs from XDG.
- `usage.rs`: opt-in Usage log (`Writer`, `forget`).

## kotodama-protocol

- `lib.rs`: re-exports; `Error`.
- `message.rs`: Daemon and Extension messages, `ToExtension`, `FromExtension`, `LiveResult`, `Outcome`, `Clipboard`, `CatalogEntry`.
- `front_end.rs`: Daemon and Front end messages, `FromFrontEnd` (incl. `Query`, `Viewport`, `Open`, `Toggle`), `ToFrontEnd`, `ShownResult`, `socket_path`.
- `item.rs`: `Item`, `ItemRef`, `ItemKind`, `SHARED_KINDS`, `NamespacedKind`, `KindName`.
- `view.rs`: `View`, `ViewContent`, forms (`FormField`, `FormInput`), `Job`, `JobState`.
- `value.rs`: `Value`, `Table`, `Bytes`, `Fraction`.
- `manifest.rs`: `Manifest::from_toml`, `LiveQueries`, `ContributedAction` (ADR 0002).
- `handshake.rs`: `PROTOCOL_VERSION`, `Hello::negotiate` (ADR 0012).
- `frame.rs`: length-prefixed frames, `MAX_FRAME_LEN` (ADR 0004).
- `wire.rs`: `WireFormat` (postcard, JSON, MessagePack) `encode`/`decode`.
- `channel.rs`: `Channel::handshake`, `send`, `recv`, `split`.
- `launch.rs:launch_command`: detached program start, `KOTODAMA_LAUNCH_PREFIX`.

## kotodama (CLI)

- `src/main.rs:parse`: `kotodama toggle`, `kotodama open [--query TEXT] [--view ID]`, `help`; sends one `FromFrontEnd` through the Daemon (ADR 0007).

## kotodama-qml (Front end)

- `qml/KotodamaMenu.qml`: the menu UI; theme properties at the top (colors, font, sizes, `maxRows`).
- `src/lib.rs`: `SearchRust`, the QObject the QML talks to (cxx-qt bridge).
- `src/connection.rs`: the Daemon connection thread (`spawn`, `Update`, `Writer`).
- `src/rows.rs`: `Row`, `Replace` (list model diffing without reset), Action panel entries.
- `src/views.rs`: `Views` stack, form values, jobs.
- `src/variant.rs`: Views, jobs and actions as QVariant maps for QML.
- `src/shortcut.rs:matches`: Action key bindings like `ctrl+o`.
- `src/plugin.rs`: Qt plugin entry symbols. `build.rs`: cxx-qt build.

## Bundled extensions

Each has `manifest.toml`, `src/main.rs` (the protocol loop) and a settings/items module reading `[extensions.<name>]`.

- apps: `scan.rs` (`scan`, `application_dirs`), `entry.rs` (`.desktop` parsing), `exec.rs` (`Exec` field codes), `items.rs` (`Settings`, `SentItems`), `launch.rs`, `watch.rs`.
- calculator: `lib.rs` (`Calculator::evaluate`), `math.rs` (fend), `dates.rs` (dates, time zones), `rates.rs` (ECB rates fetch and cache).
- clipboard: `wayland.rs` (data-control watch/set), `copied.rs` (MIME choice, `Copied`), `history.rs` (`History` on disk, `Limits`), `items.rs` (Items, list and detail Views).
- folders: `settings.rs`, `projects.rs` (git repos, tms names), `scripts.rs` (executables and header options), `scan.rs` (`Scanner` thread), `items.rs`, `launch.rs`.

## Where to change X

- **Protocol message or field**: `kotodama-protocol/src/{message,front_end,item,view}.rs`. Bump `handshake.rs:PROTOCOL_VERSION` per ADR 0012, then rerun `tests/layout.rs` with `KOTODAMA_BLESS_LAYOUT=1` to rewrite `tests/layout.txt`. Update both ends: `kotodamad/src/daemon.rs`, `kotodama-qml/src/connection.rs`, the Extensions.
- **Ranking rule**: `kotodamad/src/daemon.rs:Candidate::rank`, `rank.rs` (Tiers), `history.rs` (Frecency, Learned choices), `kind_order.rs`. Add cases to `kotodamad/tests/ranking.rs`; add an ADR if it changes the order of precedence.
- **Bundled extension**: new `crates/kotodama-<name>/` with `manifest.toml`; add `<name>` to the loop in `nix/package.nix`; add the crate to the `Cargo.lock` jsonpath in `release-please-config.json`.
- **e2e test**: new `e2e/<name>/test.sh` (plus `shell.qml`, reference `.png` if visual) using `e2e/lib.sh`; `just e2e <name>`, `just e2e --update` for images.
- **Bench**: `crates/<crate>/benches/*.rs` with a `[[bench]] harness = false` entry in its `Cargo.toml` (criterion). CI compares against the base commit.
- **Config key**: the `from_config` of the owner (`kotodamad/src/{history,empty_query,kind_order,keywords}.rs`), or an Extension's settings module for `[extensions.<name>]`; test in `kotodamad/tests/config.rs`. Home Manager options: `nix/home-manager.nix`.
- **Front end QML**: `kotodama-qml/qml/KotodamaMenu.qml`; Rust-side state in `src/{lib,rows,views,variant}.rs`. Check `e2e/menu`, `e2e/smoke`, `e2e/views` reference images.
- **CLI**: `crates/kotodama/src/main.rs:parse`.

## Tests and checks

- Unit tests: `#[cfg(test)]` modules in each `src/*.rs`.
- Integration: `crates/*/tests/*.rs`. Daemon: `kotodamad/tests/` (`common/` helpers, `fixtures/recorder`, `ranking.rs`, `interaction.rs`, `config.rs`, `forget.rs`). Protocol: `layout.rs`, `conformance.rs`, `shell_extension.rs`.
- e2e: `e2e/*/test.sh` under headless sway. `latency` is skipped on PRs.
- Benches: `kotodamad/benches/{query,daemon}.rs` (+ `corpus/`), `kotodama-protocol/benches/batch.rs`, `kotodama-calculator/benches/answer.rs`, `kotodama-clipboard/benches/search.rs`.
- Nix: `nix/tests/vm.nix` (NixOS VM, `front-end.py`), `nix/tests/qml-module.nix`; `flake.nix` `checks`.
- Script tests: `scripts/tests/*.sh`.

`just` recipes (all run in `scripts/dev-env`):
- `just check`: nixfmt, shellcheck, `just --fmt`, actionlint, `cargo fmt --check`, clippy `-D warnings`.
- `just test`: script tests, `cargo nextest run --workspace`, doc tests.
- `just bench`: `cargo bench --workspace`.
- `just e2e [--update] [test...]`: `e2e/run`.
- `just shell`, `just image`.

CI (`.github/workflows/ci.yml`):
- On PRs and main: `test` (check + test, x86 and arm), `e2e` (both arches, no `latency` on PRs), `bench` (base vs this commit; fails on +50% time, +10% Daemon memory).
- Push only: `nixos-vm` (VM and QML module checks). Main only: `image` (dev image), cargo cache saves.
- `release-please.yml`: release PRs from Conventional Commits.

## ADRs

- 0001 architecture: Daemon, Front ends, Extensions, overall shape.
- 0002 Contributed actions by Item kind (manifests).
- 0003 Late results never reorder visible rows (Session, Viewport).
- 0004 Frames have a text length header (`frame.rs`).
- 0005 The Daemon's own Results use the reserved Extension name `kotodama`.
- 0006 Results rank by Tier before usage (`Candidate::rank`).
- 0007 The `kotodama` command goes through the Daemon (CLI).
- 0008 The same thing from several Extensions shares one History (identities).
- 0009 Frecency learns time of day in parts of the day (`clock.rs`, `history.rs`).
- 0010 Undo puts back what a change replaced (`UNDO`).
- 0011 Item kind breaks ties between equal matches after Frecency (`kind_order.rs`).
- 0012 Protocol version goes up with every layout change (`PROTOCOL_VERSION`, `tests/layout.rs`).
