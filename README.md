# Kotodama

言霊, "the spirit of words": the old belief that spoken words make things happen.

Kotodama is a command menu for Wayland desktops. You type, and it finds and does things: launches apps, switches windows, runs scripts, opens projects, does math, pastes from clipboard history, controls the system. It is built to be faster than anything else of its kind, and everything beyond the core comes from Extensions.

- **`kotodamad`**, a Rust daemon, owns the index, the ranking and the Extensions, and stays warm between uses.
- **A Quickshell QML module** written in Rust (cxx-qt) gives QML a native result model. QML only draws the results.
- **Extensions** are separate processes in any language. They speak a protocol whose wire format is negotiated (JSON, MessagePack or postcard), so a crashing Extension never takes down the menu.

Status: early. The protocol and the Daemon work; the Quickshell Front end is next. See `docs/adr/0001-architecture.md` and the issues.

## The Daemon

`kotodamad` serves Front ends on `$XDG_RUNTIME_DIR/kotodama.sock`. It finds Extensions in `$XDG_DATA_DIRS/kotodama/extensions/*/manifest.toml` and `~/.config/kotodama/extensions/*/manifest.toml`, starts each one, and restarts it with backoff when it stops or hangs. Extensions installed, changed or removed while it runs are picked up without a restart. Its config is every top-level `*.toml` in `$XDG_CONFIG_DIRS/kotodama/`, then `~/.config/kotodama/`, merged in order and reloaded when a file changes. `KOTODAMA_LOG=debug` logs more.

### Ranking that learns

Every Action the user runs is recorded in `$XDG_STATE_HOME/kotodama/history.json` (by default `~/.local/state/kotodama/`). A picked Result gets a Frecency boost, which halves every 14 days. When the user picks the same Result 3 times in a row for one query, it becomes that query's Learned choice and ranks first; 3 picks in a row of another Result replace it. Both apply to Indexed items and Live query results. The "Clear history" Result empties the history.

Aliases and Quicklinks live in the config directory and reload with it:

```toml
[aliases]
ff = { extension = "apps", item = "firefox" }
# An Alias for one of the Item's own Actions makes that Action primary.
code = { extension = "projects", item = "kotodama", action = "open-editor" }

[quicklinks.gh]
title = "GitHub"
url = "https://github.com"

# `{query}` is what's typed after the keyword, percent-encoded in a URL.
[quicklinks.yt]
title = "Search YouTube"
url = "https://www.youtube.com/results?search_query={query}"

[quicklinks.logs]
command = ["foot", "journalctl", "--user", "-fu", "{query}"]
```

Typing an Alias puts its Result first. Typing a Quicklink's keyword, followed by a query if its template takes one, puts the Quicklink first. A URL opens with `xdg-open`, and a command runs without a shell. The Daemon's own Results, the Quicklinks and "Clear history", belong to the reserved Extension name `kotodama`.

### Trying it

To try it from a terminal, run the Daemon, then the bare example Front end:

```sh
cargo run -p kotodamad
cargo run -p kotodamad --example front-end -- firefox        # print the Results
cargo run -p kotodamad --example front-end -- --run firefox  # run the top one
```

### Performance

`just bench` measures two things with 10k Indexed items:

- `query_10k_items`: from a query arriving to its Results encoded for the Front end, in process (criterion).
- `daemon`: the real `kotodamad` with a real Extension process: the round trip of 2,000 queries over the socket, and the Daemon's resident memory (`VmRSS`) afterwards.

| Machine | First keystroke | Next keystroke | Socket round trip p99 | Resident memory |
| --- | --- | --- | --- | --- |
| Ryzen AI 9 HX 370 (target) | to be measured | to be measured | to be measured | to be measured |
| Apple M-series, dev container (arm64 Linux VM) | 309 µs | 42 µs | 0.38 ms | 13.3 MiB |

The target is a p99 under 2 ms from query received to Results sent. CI fails when a change makes resident memory more than 10% higher than its base commit does.

## Development

One environment, defined by the Nix flake's dev shell, runs everywhere. The `just` recipes run in it on every machine:

- **Linux with Nix**: `just` runs the recipe in `nix develop`.
- **macOS, or Linux without Nix**: `just` runs the same recipe in the dev container, with Docker or OrbStack.

From clone to green tests:

```sh
git clone https://github.com/SuplenC/kotodama && cd kotodama
brew install just      # or your package manager, or `nix profile install nixpkgs#just`
just test
```

The first run fetches the toolchain (Rust, Qt 6, Quickshell, sway) into the `kotodama-nix` Docker volume, or into your Nix store. Later runs start in about a second.

| Recipe | What it does |
| --- | --- |
| `just shell` | Opens a shell in the dev environment |
| `just check` | Checks formatting and runs the lints (rustfmt, clippy, nixfmt, shellcheck, actionlint) |
| `just test` | Runs the tests; extra arguments go to `cargo test` |
| `just bench` | Runs the benchmarks; extra arguments go to `cargo bench` |
| `just e2e` | Runs the e2e tests against headless sway; `just e2e --update` rewrites the reference images |
| `just image` | Rebuilds the dev container image locally, e.g. after editing `docker/` without committing |

Nix only sees files that git tracks, so `git add` new files before running a recipe.

### e2e tests

`e2e/run` starts sway with the headless backend and the pixman renderer on one 640x480 output, then runs every `e2e/*/test.sh` against it. A test starts its clients with `spawn` and checks the screen with `expect_screen reference.png`, which compares a grim screenshot with the stored image. When a test fails, the screenshot, a diff image and every log are in `e2e/out`, which CI uploads as an artifact.

Hyprland can't run here: it needs a DRM device. Its live tests run in a NixOS VM (`checks.x86_64-linux.nixos-vm`).

### CI

GitHub Actions runs `check`, `test` and `e2e` on x86_64 and arm64, plus the NixOS VM job on x86_64. The bench job (x86_64 only) runs the benchmarks of the base commit and of the change on the same runner, one after the other, and fails when one is more than 50% slower or the Daemon's resident memory is more than 10% higher. Shared runners differ too much for absolute timings, or a baseline saved by another run, to mean anything, so CI only compares the two runs it made itself.

On pushes to `main`, CI publishes:

- the dev container image to **`ghcr.io/suplenc/kotodama-dev`** (linux/amd64 and linux/arm64), tagged `latest` and with git's hash of `docker/`, which is the tag `just` uses;
- Nix build outputs to the **`kotodama`** Cachix cache, using the `CACHIX_AUTH_TOKEN` repository secret.

CI and the dev container read from that cache. To use it with your own Nix, add it to `/etc/nix/nix.conf` (or run `cachix use kotodama`):

```
extra-substituters = https://kotodama.cachix.org
extra-trusted-public-keys = kotodama.cachix.org-1:8/a1e4X26O1hz2m5HFJBfKQ+P8m3GL7Z3Iy+xRIeZWE=
```
