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
- [`crates/kotodama-extension/`](crates/kotodama-extension/CODEMAP.md): the runtime Rust Extensions are built on (handshake, reader thread, `SentItems`, config, launch, XDG dirs, directory watcher, waiting on `$XDG_RUNTIME_DIR`, and with feature `wayland` finding the compositor and data-control; the dev-only `testing` harness the Bundled extensions' tests share). The Daemon uses its watcher and its Wayland module.
- [`crates/kotodama-ai/`](crates/kotodama-ai/CODEMAP.md): Bundled extension, Proposals: catalog Actions a model picks for a request in plain words, run once the user confirms.
- [`crates/kotodama-apps/`](crates/kotodama-apps/CODEMAP.md): Bundled extension, desktop apps from `.desktop` files.
- [`crates/kotodama-calculator/`](crates/kotodama-calculator/CODEMAP.md): Bundled extension, math, units, currency, dates.
- [`crates/kotodama-clipboard/`](crates/kotodama-clipboard/CODEMAP.md): Bundled extension, clipboard history.
- [`crates/kotodama-emoji/`](crates/kotodama-emoji/CODEMAP.md): Bundled extension, emoji (with skin tones) and symbols, typed or copied.
- [`crates/kotodama-folders/`](crates/kotodama-folders/CODEMAP.md): Bundled extension, git projects and scripts from configured folders.
- [`crates/kotodama-hyprland/`](crates/kotodama-hyprland/CODEMAP.md): Bundled extension, Hyprland's windows (from its event socket) and keyboard shortcuts, with the Shortcuts View.
- [`crates/kotodama-web-search/`](crates/kotodama-web-search/CODEMAP.md): Bundled extension, the fallback: web searches for what nothing matches, and addresses opened.
- `e2e/`: end-to-end tests against headless sway (`e2e/run`, `e2e/lib.sh`, one dir per test).
- `nix/`: `package.nix` (the package and Bundled extension layout; crane builds its dependencies as a derivation of their own), `gungraun-runner.nix` (pinned runner for the instruction-counting benches), `home-manager.nix`, `tests/` (NixOS VM, Hyprland VM and QML module checks).
- `scripts/`: dev environment (`dev-env`, `dev-image`), bench runners (`bench-run` for criterion, `bench-instructions` for gungraun, `bench-compare`, `bench-crates`), `path-deps` (a crate's workspace dependencies), `has-crates`, `qml-module`, `tests/` (script tests).
- `docker/`: the dev image for machines without Nix (`just image`).
- `examples/shell-extension/`: a minimal Extension in shell, also run by `kotodama-protocol/tests/shell_extension.rs`.
- `docs/adr/`: decisions, one file per ADR named for the area it governs. `docs/agents/`: how agents use issues, labels and domain docs.
- Root: `flake.nix` (dev shell, packages, `overlays.default`, `checks`), `justfile`, `release-please-config.json`, `GLOSSARY.md`.

## Where to change X

- **Protocol message or field**: `kotodama-protocol/src/{message,front_end,item,view}.rs`. Bump `handshake.rs:PROTOCOL_VERSION` per ADR 0012, then rerun `tests/layout.rs` with `KOTODAMA_BLESS_LAYOUT=1` to rewrite `tests/layout.txt`. Update both ends: `kotodamad/src/daemon.rs`, `kotodama-qml/src/connection.rs`, the Extensions.
- **Ranking rule**: `kotodamad/src/daemon.rs:Candidate::rank`, `rank.rs` (Tiers), `history.rs` (Frecency, Learned choices), `kind_order.rs`. Add cases to `kotodamad/tests/ranking.rs`; add an ADR if it changes the order of precedence.
- **Bundled extension**: new `crates/kotodama-<name>/` with `manifest.toml`, built on `kotodama-extension`; add `<name>` to the loop in `nix/package.nix`; add the crate to the `Cargo.lock` jsonpath in `release-please-config.json`; give it a `CODEMAP.md` and a line in the layout above.
- **e2e test**: new `e2e/<name>/test.sh` (plus `shell.qml`, reference `.png` if visual) using `e2e/lib.sh`; `just e2e <name>`, `just e2e --update` for images.
- **Bench**: `crates/<crate>/benches/*.rs` with a `[[bench]] harness = false` entry in its `Cargo.toml` (criterion). CI compares against the base commit. An instruction-counting one (gungraun, under Valgrind) is a bench target named `instructions`, which `bench-run` leaves out and `bench-instructions` runs.
- **Config key**: the `from_config` of the owner (`kotodamad/src/{history,empty_query,kind_order,keywords}.rs`), or an Extension's settings module for `[extensions.<name>]`; test in `kotodamad/tests/config.rs`. Home Manager options: `nix/home-manager.nix`.
- **Front end QML**: `kotodama-qml/qml/KotodamaMenu.qml`; Rust-side state in `src/{lib,rows,views,variant}.rs`. Check `e2e/menu`, `e2e/smoke`, `e2e/views` reference images.
- **Reserved key**: `kotodama-protocol/src/shortcut.rs:RESERVED_SHORTCUTS`; the Daemon drops them in `kotodamad/src/reserved.rs`; list them for Extension authors in `examples/shell-extension/README.md`.
- **CLI**: `crates/kotodama/src/main.rs:parse`.

## Tests and checks

- Unit tests: `#[cfg(test)]` modules in each `src/*.rs`.
- Integration: `crates/*/tests/*.rs`. Daemon: `kotodamad/tests/` (`common/` helpers, `fixtures/recorder`, `ranking.rs`, `interaction.rs`, `open_view.rs`, `views.rs`, `config.rs`, `forget.rs` (Forget, clearing, undo and its Toast offers), `reads_first.rs`: off-path indexing, hot Items and a property test against a full search). Protocol: `layout.rs`, `conformance.rs`, `shell_extension.rs`. Bundled extensions share `kotodama-extension`'s `testing` feature (`Spawned`, in-process Daemon setup).
- e2e: `e2e/*/test.sh` under headless sway, which `e2e/run` gives a virtual keyboard for the whole run (without one, windows bind no keyboard and lose keys). `e2e/type-and-paste/client.qml` is the window typed into; it logs `ready` once it has the keys. `latency` runs on PRs only when what it builds on changed. `emoji` types an emoji in a configured skin tone, and a symbol, into a test client. `open-view` checks `kotodama open --view` shows the View in the first frame (its `shell.qml` logs the top View as the window maps). `views` checks list View filtering, that typing in a 1000-Item list View keeps the selected Item and never resets the model, and that no View change shows the search between two Views. `late-display` starts the Daemon and the clipboard Extension with no compositor in sight, then links the socket into their runtime dir. `type-and-paste` and `clipboard` wait for the Daemon's debug log `copied to the clipboard` before Ctrl+V. `undo` forgets a picked Result by its key and checks Ctrl+Z on the Toast puts it back in the same place.
- Benches: in each crate's `benches/`, listed in its `CODEMAP.md`. `kotodamad/benches/instructions.rs` is the instruction-count gate CI runs on PRs (`scripts/bench-instructions`); the rest are criterion, run by `scripts/bench-run`.
- Nix: `nix/tests/vm.nix` (NixOS VM, `front-end.py`; its `switched` specialisation, switched to mid-test, installs apps and Extensions through the system and home-manager profiles), `nix/tests/hyprland.nix` (Hyprland in a NixOS VM with a virtio GPU: windows, binds, `--view hyprland/shortcuts`), `nix/tests/qml-module.nix`; `flake.nix` `checks`.
- Script tests: `scripts/tests/*.sh`.

`just` recipes (all run in `scripts/dev-env`):
- `just check`: nixfmt, shellcheck, `just --fmt`, actionlint, `cargo fmt --check`, clippy `-D warnings`.
- `just test`: script tests, `cargo nextest run --workspace`, doc tests.
- `just bench`: `cargo bench --workspace` (with the gungraun benches; Valgrind and `gungraun-runner` are in the dev shell).
- `just e2e [--update] [test...]`: `e2e/run`.
- `just shell`, `just image`.

CI (`.github/workflows/ci.yml`):
- On PRs, `changes` gates the jobs below by path (docs-only runs none; `bench` and `instructions` only for the crates `scripts/bench-crates` lists, lockfiles, scripts and CI; e2e `latency` for the crates the Daemon, `kotodama` and the QML module build on and the e2e harness; `nixos-vm` for crates, lockfiles, the flake and `nix/`); `ci` is the single required check.
- On PRs and main: `test` (check + test, x86 and arm), `e2e` (both arches; `latency` as gated above), `bench` (base vs this commit; fails on +50% time, +10% Daemon memory). On main, `bench` runs both commits on one runner and caches the results as `bench-results-<sha>`; a PR compares against its base's cached results with shorter criterion times, reruns the base on its own runner only when a timing fails, and runs both commits when nothing is cached. `instructions` (`scripts/bench-instructions`: base vs this commit on one runner; fails on +5% instructions). `nixos-vm` (the NixOS VM, Hyprland VM and QML module checks; the package's dependencies come from Cachix).
- Main only: `image` (dev image), cargo cache saves.
- `release-please.yml`: release PRs from Conventional Commits.
