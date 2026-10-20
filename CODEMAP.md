# Code map

Where things are, so you can open the right file without searching. Read this
index, then the `CODEMAP.md` of the crate you work in, which lists its modules
and key symbols as `path:symbol`. Both are maintained by hand: fix a stale line
when you notice it. Terms are defined in [GLOSSARY.md](GLOSSARY.md); decisions
are in [docs/adr/](docs/adr/).

## Layout

- [`crates/kotodamad/`](crates/kotodamad/CODEMAP.md): the Daemon. Index, ranking, History, sessions, Extension host, socket server.
- [`crates/kotodama-protocol/`](crates/kotodama-protocol/CODEMAP.md): messages, framing, handshake and Wire formats shared by everything.
- [`crates/kotodama/`](crates/kotodama/CODEMAP.md): the `kotodama` CLI (`toggle`, `open`) that a compositor keybind runs.
- [`crates/kotodama-qml/`](crates/kotodama-qml/CODEMAP.md): the Quickshell Front end, a Rust cdylib QML module (`Kotodama`) plus `qml/KotodamaMenu.qml`.
- [`crates/kotodama-extension/`](crates/kotodama-extension/CODEMAP.md): the runtime Rust Extensions are built on (handshake, reader thread, `SentItems`, config, launch, XDG dirs, directory watcher; the dev-only `testing` harness the Bundled extensions' tests share). The Daemon uses its watcher.
- [`crates/kotodama-apps/`](crates/kotodama-apps/CODEMAP.md): Bundled extension, desktop apps from `.desktop` files.
- [`crates/kotodama-calculator/`](crates/kotodama-calculator/CODEMAP.md): Bundled extension, math, units, currency, dates.
- [`crates/kotodama-clipboard/`](crates/kotodama-clipboard/CODEMAP.md): Bundled extension, clipboard history.
- [`crates/kotodama-emoji/`](crates/kotodama-emoji/CODEMAP.md): Bundled extension, emoji (with skin tones) and symbols, typed or copied.
- [`crates/kotodama-folders/`](crates/kotodama-folders/CODEMAP.md): Bundled extension, git projects and scripts from configured folders.
- [`crates/kotodama-web-search/`](crates/kotodama-web-search/CODEMAP.md): Bundled extension, the fallback: web searches for what nothing matches, and addresses opened.
- `e2e/`: end-to-end tests against headless sway (`e2e/run`, `e2e/lib.sh`, one dir per test).
- `nix/`: `package.nix` (the package and Bundled extension layout), `home-manager.nix`, `tests/` (NixOS VM and QML module checks).
- `scripts/`: dev environment (`dev-env`, `dev-image`), bench runners (`bench-run`, `bench-compare`, `bench-crates`), `has-crates`/`has-benches`, `qml-module`, `tests/` (script tests).
- `docker/`: the dev image for machines without Nix (`just image`).
- `examples/shell-extension/`: a minimal Extension in shell, also run by `kotodama-protocol/tests/shell_extension.rs`.
- `docs/adr/`: decisions, one file per ADR named for the area it governs. `docs/agents/`: how agents use issues, labels and domain docs.
- Root: `flake.nix` (dev shell, packages, `checks`), `justfile`, `release-please-config.json`, `GLOSSARY.md`.

## Where to change X

- **Protocol message or field**: `kotodama-protocol/src/{message,front_end,item,view}.rs`. Bump `handshake.rs:PROTOCOL_VERSION` per ADR 0012, then rerun `tests/layout.rs` with `KOTODAMA_BLESS_LAYOUT=1` to rewrite `tests/layout.txt`. Update both ends: `kotodamad/src/daemon.rs`, `kotodama-qml/src/connection.rs`, the Extensions.
- **Ranking rule**: `kotodamad/src/daemon.rs:Candidate::rank`, `rank.rs` (Tiers), `history.rs` (Frecency, Learned choices), `kind_order.rs`. Add cases to `kotodamad/tests/ranking.rs`; add an ADR if it changes the order of precedence.
- **Bundled extension**: new `crates/kotodama-<name>/` with `manifest.toml`, built on `kotodama-extension`; add `<name>` to the loop in `nix/package.nix`; add the crate to the `Cargo.lock` jsonpath in `release-please-config.json`; give it a `CODEMAP.md` and a line in the layout above.
- **e2e test**: new `e2e/<name>/test.sh` (plus `shell.qml`, reference `.png` if visual) using `e2e/lib.sh`; `just e2e <name>`, `just e2e --update` for images.
- **Bench**: `crates/<crate>/benches/*.rs` with a `[[bench]] harness = false` entry in its `Cargo.toml` (criterion). CI compares against the base commit.
- **Config key**: the `from_config` of the owner (`kotodamad/src/{history,empty_query,kind_order,keywords}.rs`), or an Extension's settings module for `[extensions.<name>]`; test in `kotodamad/tests/config.rs`. Home Manager options: `nix/home-manager.nix`.
- **Front end QML**: `kotodama-qml/qml/KotodamaMenu.qml`; Rust-side state in `src/{lib,rows,views,variant}.rs`. Check `e2e/menu`, `e2e/smoke`, `e2e/views` reference images.
- **CLI**: `crates/kotodama/src/main.rs:parse`.

## Tests and checks

- Unit tests: `#[cfg(test)]` modules in each `src/*.rs`.
- Integration: `crates/*/tests/*.rs`. Daemon: `kotodamad/tests/` (`common/` helpers, `fixtures/recorder`, `ranking.rs`, `interaction.rs`, `open_view.rs`, `views.rs`, `config.rs`, `forget.rs`). Protocol: `layout.rs`, `conformance.rs`, `shell_extension.rs`. Bundled extensions share `kotodama-extension`'s `testing` feature (`Spawned`, in-process Daemon setup).
- e2e: `e2e/*/test.sh` under headless sway, which `e2e/run` gives a virtual keyboard for the whole run (without one, windows bind no keyboard and lose keys). `e2e/type-and-paste/client.qml` is the window typed into; it logs `ready` once it has the keys. `latency` is skipped on PRs. `emoji` types an emoji in a configured skin tone, and a symbol, into a test client. `open-view` checks `kotodama open --view` shows the View in the first frame (its `shell.qml` logs the top View as the window maps). `views` checks list View filtering and that no View change shows the search between two Views.
- Benches: `kotodamad/benches/{query,daemon}.rs` (+ `corpus/`), `kotodama-protocol/benches/batch.rs`, `kotodama-calculator/benches/answer.rs`, `kotodama-clipboard/benches/search.rs`, `kotodama-emoji/benches/search.rs`.
- Nix: `nix/tests/vm.nix` (NixOS VM, `front-end.py`), `nix/tests/qml-module.nix`; `flake.nix` `checks`.
- Script tests: `scripts/tests/*.sh`.

`just` recipes (all run in `scripts/dev-env`):
- `just check`: nixfmt, shellcheck, `just --fmt`, actionlint, `cargo fmt --check`, clippy `-D warnings`.
- `just test`: script tests, `cargo nextest run --workspace`, doc tests.
- `just bench`: `cargo bench --workspace`.
- `just e2e [--update] [test...]`: `e2e/run`.
- `just shell`, `just image`.

CI (`.github/workflows/ci.yml`):
- On PRs, `changes` gates the jobs below by path (docs-only runs none; `bench` only for the crates `scripts/bench-crates` lists, lockfiles, scripts and CI); `ci` is the single required check.
- On PRs and main: `test` (check + test, x86 and arm), `e2e` (both arches, no `latency` on PRs), `bench` (base vs this commit; fails on +50% time, +10% Daemon memory). On main, `bench` runs both commits on one runner and caches the results as `bench-results-<sha>`; a PR compares against its base's cached results with shorter criterion times, reruns the base on its own runner only when a timing fails, and runs both commits when nothing is cached.
- Push only: `nixos-vm` (VM and QML module checks). Main only: `image` (dev image), cargo cache saves.
- `release-please.yml`: release PRs from Conventional Commits.
