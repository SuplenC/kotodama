# Kotodama

言霊, "the spirit of words": the old belief that spoken words make things happen.

Kotodama is a command menu for Wayland desktops. You type, and it finds and does things: launches apps, switches windows, runs scripts, opens projects, does math, pastes from clipboard history, controls the system. It is built to be faster than anything else of its kind, and everything beyond the core comes from Extensions.

- **`kotodamad`**, a Rust daemon, owns the index, the ranking and the Extensions, and stays warm between uses.
- **A Quickshell QML module** written in Rust (cxx-qt) gives QML a native result model. QML only draws the results.
- **Extensions** are separate processes in any language. They speak a protocol whose wire format is negotiated (JSON, MessagePack or postcard), so a crashing Extension never takes down the menu.

Status: early. The protocol, the Daemon, the Quickshell Front end and the apps, clipboard history and calculator Bundled extensions work; Views, the Action panel and the other Bundled extensions are next. See `docs/adr/0001-architecture.md` and the issues.

## Installing

The flake builds Kotodama once and splits it into outputs, each also a package of its own:

| Package | Output | What's in it |
| --- | --- | --- |
| `kotodamad` | `out` | the `kotodama` command, the Daemon, and its systemd user units in `lib/systemd/user/` |
| `kotodama-qml` | `qml` | the `Kotodama` QML module, in `lib/qt-6/qml/` |
| `kotodama-extensions` | `extensions` | the Bundled extensions, in `share/kotodama/extensions/` |

`default` installs all three. With home-manager:

```nix
{
  inputs.kotodama.url = "github:SuplenC/kotodama";

  # in your home-manager configuration
  imports = [ inputs.kotodama.homeModules.default ];
  programs.kotodama = {
    enable = true;
    # Written to ~/.config/kotodama/home-manager.toml; any other *.toml there still applies.
    settings = {
      extensions.apps.terminal = [ "ghostty" "-e" ];
      quicklinks.gh = { title = "GitHub"; url = "https://github.com"; };
    };
  };
}
```

The module installs the package and enables `kotodamad.socket`. On NixOS without home-manager, add the package to `systemd.packages` and `environment.systemPackages`, and set `systemd.user.sockets.kotodamad.wantedBy = [ "sockets.target" ]`.

The Daemon starts the first time something connects to its socket, whether a `kotodama toggle` or the Quickshell Front end, and stays running after that. While it is stopped, systemd holds new connections and starts it again. It runs with the environment of the systemd user manager, not the compositor's:

- **Desktop notifications** (a job finishing) go over the session bus at `DBUS_SESSION_BUS_ADDRESS`, or `$XDG_RUNTIME_DIR/bus` when that isn't set. That is where systemd's per-user bus lives, so this works with no setup.
- **Typing, pasting and copying** need `WAYLAND_DISPLAY`, and the apps it starts need the rest of the graphical session's environment. The compositor must hand these to the user manager before the menu is first used, since the Daemon reads its environment once, when it starts. uwsm does this. In a plain Hyprland session, add `exec-once = dbus-update-activation-environment --systemd --all` (or `systemctl --user import-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP`). If the Daemon started before that, run `systemctl --user restart kotodamad`.

The Nix package's `kotodamad` puts its Bundled extensions first on its own `XDG_DATA_DIRS`, followed by the inherited directories (or `/usr/local/share:/usr/share` when there are none). The rest of the session's `XDG_DATA_DIRS` is left alone.

## The Daemon

`kotodamad` serves Front ends on `$XDG_RUNTIME_DIR/kotodama.sock`. It finds Extensions in `$XDG_DATA_DIRS/kotodama/extensions/*/manifest.toml` and `~/.config/kotodama/extensions/*/manifest.toml`, starts each one, and restarts it with backoff when it stops or hangs. Extensions installed, changed or removed while it runs are picked up without a restart. Its config is every top-level `*.toml` in `$XDG_CONFIG_DIRS/kotodama/`, then `~/.config/kotodama/`, merged in order and reloaded when a file changes. `KOTODAMA_LOG=debug` logs more. Under systemd socket activation (`kotodamad.socket`) it serves the socket systemd hands it instead of binding its own.

Apps, Quicklinks and every other program started for the user run after the words in `KOTODAMA_LAUNCH_PREFIX`, if it is set. Extensions inherit it from the Daemon and should do the same. The systemd unit sets it to `systemd-run --user --scope --slice=app.slice --collect --quiet`, so each program gets a scope of its own and outlives a stop or restart of the Daemon, whose service would otherwise take every process in its cgroup down with it. A prefix such as `uwsm app --` works too. The words are split on whitespace, so the prefix can't contain a path with spaces.

### Ranking that learns

Results rank by how well they match before anything else, in tiers: the whole title (or one of the Item's keywords) is the query, then the query is the initials of consecutive words (`vsc` for Visual Studio Code) or a word starts with it, then the query is inside a word, then its letters are scattered. Within a tier, Frecency decides, then the kind of Result (an app before a project, a project before a bookmark (a `url` Item), anything else, then a file), then nucleo's score, then the shorter title. Neither case nor accents matter (`zolw` finds Żółw), every word of the query must match in any order, a single letter matches only at the start of a word, and a trailing space makes the last word a whole word. A subtitle is searched once the query has 3 letters, and only at word starts.

Every Action the user runs is recorded in `$XDG_STATE_HOME/kotodama/history.json` (by default `~/.local/state/kotodama/`). A picked Result gets a Frecency boost, which halves every 14 days but never quite reaches zero. Frecency also learns the time of day: picks count in full at the times of day the Result is usually picked, and half as much at a time of day it never is, so something used every morning ranks higher in the morning. A Result used about daily for a week ranks one tier higher, but never above an exact title. When the user picks the same Result 3 times for one query, it becomes that query's Learned choice: it ranks first for that query and for the queries that extend it or that it extends (`fi`, `fir`, `fire`), but only over the Results that ranked above it when it was picked, so a new app with exactly that title still wins. Two picks of another Result since the Learned choice was last picked take over from it. Learning fades with Frecency's half-life; when two Learned choices apply, the one picked more often wins, then the one learned for the closer query. Both apply to Indexed items and Live query results. Items that several Extensions offer with the same Identity (a page from bookmarks and from browser history) share their Frecency and Learned choices: picking one raises the others. Each still shows as a Result of its own, and the one the user picks through most takes the best place any of them ranks in. The "Clear history" Result empties the history.

The Action panel of a Result the history keeps anything for offers "Forget", which drops its Frecency, every Learned choice for it, the picks of the Aliases that name it and, for a Result with an Identity, the history of that thing from every Extension. It also offers "Clear the history of" its Extension, which forgets every Result of that Extension, the Aliases that name them, and the shared history of the Identities its Items carry, though another Extension offers them too: the history doesn't know which Extension a pick of a shared thing came through. Another Extension's Result of such a thing keeps only its own picks, which decide its place among the Results of that thing. For a minute after Forget, clearing an Extension's history or "Clear history", typing `undo` finds a Result that puts back what was forgotten, latest first; anything picked since stays as it is.

These numbers are settings in the config's `[history]` table, which reloads with the config. The values shown are the defaults:

```toml
[history]
picks_to_learn = 3      # picks of one Result for one query that make it the query's Learned choice
picks_to_take_over = 2  # picks of another Result, since the choice was last picked, that replace it
half_life_days = 14     # how long a pick takes to count half as much; above 0
lift_threshold = 5.0    # the Frecency that lifts a Result one tier, about a week of daily use; above 0
time_of_day = 0.5       # how much less picks count at a time of day the Result is never picked; 0 to 1
usage_log = false       # log queries and picks, to tune ranking from real use
```

A setting that doesn't parse is logged and keeps its default. With `usage_log = true`, every query typed and every Action run is appended to `$XDG_STATE_HOME/kotodama/usage.jsonl`, one JSON object per line, which only the user can read and which never leaves the machine:

```json
{"at":1760870400,"kind":"query","query":"fi"}
{"at":1760870401,"kind":"pick","query":"fi","extension":"apps","id":"firefox","action":"open","row":1}
```

`at` is seconds since the Unix epoch; a pick's `extension` and `id` name the Result, `action` the Action run, and `row` where the Result was in the list (from 0), or `null` if it wasn't in it. Emptying the search box isn't logged as a query, but a pick made with nothing typed is, with `"query":""`. When the file would pass 4 MiB it becomes `usage.jsonl.1`, replacing the older one, so the log never takes more than 8 MiB. Turning it off keeps what was written; delete the files to forget it. "Clear history" deletes both files, and clearing one Extension's history drops the picks of its Results from them. Forgetting one Result leaves the log as it is, and Undo puts back the history, never the log.

With nothing typed, the menu shows the 3 Results most used at this time of day, by their picks within a few hours of now, then by all their picks. An Alias's picks count for the Result it names, and a thing that several Extensions offer shows once, as the Result picked through most. A fixed menu follows them, set in the config:

```toml
[empty_query]
most_used = 3  # how many of the most used come first; 0 to 50
# Then these, in this order, each unless it is shown already.
menu = [
    { extension = "apps", item = "firefox" },
    { extension = "kotodama", item = "clear-history" },
]
```

The kind order is a setting too, which reloads with the config:

```toml
[ranking]
# On an equal match, kinds earlier in the list rank first. `*` stands for every
# kind not in it; without it, those rank last.
kinds = ["app", "project", "url", "*", "file"]
```

Aliases and Quicklinks live in the config directory and reload with it:

```toml
[aliases]
ff = { extension = "apps", item = "firefox" }
# An Alias for one of the Item's own Actions makes that Action primary.
term = { extension = "folders", item = "/home/me/Projects/kotodama", action = "shell" }

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

Typing an Alias puts its Result first, ahead of a Learned choice, and what follows it is the Result's Argument; part of an Alias does nothing. Picks through an Alias count for the Alias, not for the Result's own Frecency. Typing a Quicklink's keyword ranks it as an exact match, with Frecency deciding between equals; its row reads `Search YouTube for “cats”`, the keyword alone opens it with an empty query, and words after a keyword whose template has no `{query}` are ignored. A URL opens with `xdg-open`, and a command runs without a shell. The Daemon's own Results, the Quicklinks, "Clear history" and the Undo Result, belong to the reserved Extension name `kotodama`, and so do the Actions it offers on every Result, `forget` and `clear-extension-history`.

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

The `latency` e2e test types into the real menu with 10k Indexed items and fails if any keystroke takes 8 ms (one frame at 120 Hz) or more from its query to the list model's update. In the dev container on an Apple M-series machine, under software rendering, the worst of 40 keystrokes takes about 5 ms.

## Bundled extensions

### Apps

`kotodama-apps` (`crates/kotodama-apps`) makes every desktop application an Item of kind `app`. It reads the `.desktop` files in the `applications` directory of `$XDG_DATA_HOME` and each of `$XDG_DATA_DIRS`, and watches them, so an app installed, changed or removed while it runs shows up, changes or goes within a moment. It follows the Desktop Entry Specification: a file in a more important directory replaces one with the same desktop id, `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn` and `TryExec` leave entries out, and names, generic names, comments and keywords are read in the user's language.

Each Item's id is its desktop id without `.desktop` (`firefox`, `org.gnome.Nautilus`), and its Identity is `app:` and the desktop id (`app:firefox.desktop`). It is found by its name, its keywords, its generic name and its desktop id, and shows its generic name, or failing that its comment, below. Its primary Action, "Open", starts the app; its desktop actions, such as Firefox's "New Private Window", follow as more Actions. Apps with `Terminal=true` start in the configured terminal.

```toml
[extensions.apps]
# What starts apps with `Terminal=true`; the app's command follows it.
# The default is `xdg-terminal-exec`.
terminal = ["ghostty", "-e"]
# Apps to leave out, by desktop id, with or without `.desktop`.
hidden = ["qt5ct", "qt6ct"]
# Names to show instead of the app's own; it is still found by its own name too.
names = { nvim = "Neovim" }
```

Its `manifest.toml` runs `./kotodama-apps`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/apps/`.

### Clipboard history

`kotodama-clipboard` (`crates/kotodama-clipboard`) watches the Wayland clipboard through ext-data-control, or wlr-data-control on a compositor that has only that, and keeps what is copied in `$XDG_STATE_HOME/kotodama/clipboard/`, readable only by the user, so it survives a restart. It keeps text, and PNG and JPEG images with a 256-pixel thumbnail; when an offer has both, it keeps the text.

What a password manager copies is never read: an offer carrying the `x-kde-passwordManagerHint` type, which password managers add to mark a copy sensitive, is skipped whole. Entries go after 7 days, or once there are more than 1000, oldest first.

Each clip is an Item of kind `clipboard.clip`, found by the first 60 characters of its text, or titled with an image's size and shown with its thumbnail. Its primary Action, "Paste", pastes it into the window the menu was opened from; "Copy to Clipboard", "Show Details" (the text, or the thumbnail, in a detail View) and "Delete from History" follow. The "Clipboard History" Item lists every clip, newest first, and its second Action, "Clear Clipboard History", clears them all, on disk too.

```toml
[extensions.clipboard]
# Past this many entries, the oldest go.
max_entries = 1000
# Entries older than this many days go.
max_age_days = 7
```

Only an index of the entries (a short preview of each text, the size of each image) stays in memory: about 5 MiB resident with 1000 entries. What was copied is read from disk when it is pasted or shown. Searching 1000 entries takes about 0.1 ms in the Daemon (`benches/search.rs`, which fails above the 2 ms budget).

Its `manifest.toml` runs `./kotodama-clipboard`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/clipboard/`.

### Calculator

`kotodama-calculator` (`crates/kotodama-calculator`) answers math as you type. Its answer takes the top row, over every other Result, and Enter copies it; the second Action, "Copy Expression and Answer", copies `5 km in mi = 3.1068559612 mi`. It answers on every keystroke (its Live query debounce is 0), and a simple sum is on screen within the frame of the keystroke: `just bench` checks that the p99 from query to answer, over the Daemon's socket, stays under 8 ms. In the dev container on an Apple M-series machine it is 0.14 ms, and the calculator holds 3.9 MiB resident.

| Typed | Answer |
| --- | --- |
| `42*2`, `(3 + 4) * 2`, `2^10`, `sqrt 2` | `84`, `14`, `1024`, `1.4142135624` |
| `5 km in mi`, `100 F to C`, `3 weeks in days` | `3.1068559612 mi`, `37.7777777778 °C`, `21 days` |
| `100 usd to eur` | in euros, at the European Central Bank's rate |
| `today + 3 weeks`, `dec 25 - 1 month` | `Tuesday, 10 November 2026` |
| `days until dec 25`, `days since 2026-01-01` | `66 days` |
| `5pm PT in Warsaw`, `now in Tokyo`, `17:00 UTC to Europe/Lisbon` | `02:00 CEST, Wednesday 21 October` |

A query goes to it when it looks like math: it starts with a number (or a sign, bracket or currency symbol and then a number), a function such as `sqrt`, `today`/`tomorrow`/`yesterday` or a month and day followed by `+` or `-`, `now in`, or `days until`/`days since`. Even then it answers only if the query does something: a number or quantity alone (`42`, `5 kg`) and words (`fi`) get no answer, so they don't push the other Results down. After its `=` prefix, anything that evaluates is answered: `= pi`.

Arithmetic, units and currency come from [fend](https://github.com/printfn/fend), with each evaluation cut off after 50 ms. Dates and time zones are its own, as fend has no clock and no time zones: dates are `today`, `tomorrow`, `yesterday`, `2026-12-25`, `dec 25` or `25 december`, with or without a year (without one, the next such day, or for `days since` the last); amounts are days, weeks, months and years. A zone is an abbreviation (`PT`, `ET`, `UTC`, `CET`, `JST` and others, each standing for its region's zone all year), an IANA name, or the city one ends in (`Warsaw`, `New York`), read from the system's time zone database (`$TZDIR`, or `/usr/share/zoneinfo`). A time without a zone is local. Natural-language dates (`next friday`) are out of scope.

Exchange rates are the European Central Bank's, from [frankfurter.app](https://frankfurter.app). Nothing is fetched until the first currency query. Then the rates are fetched with `curl` and cached in `$XDG_CACHE_HOME/kotodama/calculator/rates.json`, and fetched again only once that is more than a day old; the query is answered as soon as they arrive. A failed fetch is tried again after 10 minutes.

```toml
[extensions.calculator]
# Fetch exchange rates. Off, currency uses the cached rates, however old, and
# nothing without them.
network = true
# Where the rates come from: frankfurter's API, or anything answering like it.
rates_url = "https://api.frankfurter.app/latest"
```

### Folder sources

`kotodama-folders` (`crates/kotodama-folders`, Extension name `folders`) turns the contents of the folders named in its config into Items, in two kinds of folder: projects and scripts. It watches them, so a project cloned or a script added, renamed or removed while it runs shows up or goes within a moment. It watches only the folders it searched, each on its own, never the files inside a project, so a large projects folder costs no more than its repositories. Saving files inside a project searches nothing again.

**Projects** are the git repositories found down to the folder's `depth` (3 if not given), with their worktrees and checked-out submodules. A repository stops the search: nothing inside it is searched. Each is an Item of kind `project` with its path, titled with the name `tms` (tmux-sessionizer) gives it, so the menu and `tms` land in the same tmux sessions: the folder's name; `<repository>#<worktree>` for a worktree; `<repository>>`, then the folder's name, for a submodule; and, for a name found more than once, as many parent folders as tell them apart (`work/api`, `home/api`). The tmux session is that name with every `.` made `_`. Its primary Action, "Open", opens the session in a terminal, with the editor in it. "Open Terminal Here", "Open in Lazygit", "Open in File Manager" and "Copy Path" follow, then other Extensions' Contributed actions for `project`.

**Scripts** are every executable file in the folder and its subfolders, down to 4 deep, except hidden ones. Each is an Item of kind `file` with its path, titled with its name without the extension, after its subfolders (`work/backup.sh` shows as "work › backup"). Its primary Action runs it in a terminal that stays open after it exits. A comment among its first 10 lines can change that:

```sh
#!/bin/sh
# kotodama: name=Deploy staging terminal=false args=prompt
```

`name=` is shown instead of the file's name, which still finds it. `terminal=false` runs it in the background as a job, whose end notifies the desktop with the script's last line of output. `args=prompt` asks for its arguments in a form first, split as a shell would split them.

Each Item's id is its path. A project's Identity is `folder:` and its path (`folder:/home/me/Projects/kotodama`), and a script's, `.desktop` files included, is `file:` and its path (`file:/home/me/Scripts/backup.sh`).

```toml
[extensions.folders]
folders = [
  { path = "~/Projects", kind = "projects", depth = 2 },
  { path = "~/Scripts", kind = "scripts" },
]
# What a script runs in; the script and its arguments follow. It should keep
# the window open once the script exits.
terminal = ["ghostty", "--wait-after-command=true", "-e"]
# The commands a project's Actions run, in the project's folder: `{path}` is
# the folder and `{session}` its tmux session. These are the defaults.
open = ["ghostty", "-e", "tmux", "new-session", "-A", "-s", "{session}", "-c", "{path}", "nvim"]
shell = ["ghostty", "--working-directory={path}"]
lazygit = ["ghostty", "-e", "lazygit", "--path", "{path}"]
file_manager = ["xdg-open", "{path}"]
copy_path = ["wl-copy", "{path}"]
```

Its `manifest.toml` runs `./kotodama-folders`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/folders/`.

## The Quickshell Front end

The `Kotodama` QML module is a Rust plugin (`crates/kotodama-qml`) that Quickshell imports. It connects to the Daemon, reconnects whenever the Daemon restarts, and gives QML two types:

- **`KotodamaMenu`**, the menu itself: a layer-shell window with a search box over the Results. Up and Down (or Ctrl+N and Ctrl+P) move the selection, Enter runs the selected Result's primary Action, and Esc closes it. Ctrl+K opens the Action panel: the Item's own Actions, then other Extensions' Contributed actions under each Extension's name. An Action's own key binding, such as Ctrl+L, runs it directly. The Views Extensions push (lists, forms, details and progress) stack on top of the search, and Esc pops one at a time. Closing the menu keeps them: it opens again where it was. In a form, Tab moves between fields, Space ticks a checkbox, Left and Right choose an option, and Enter submits. Running jobs are listed under the search box.
- **`KotodamaSearch`**, a list model of the Results, for a shell that draws its own window. Setting `query` searches; each row has the roles `title`, `subtitle`, `icon`, `extension` and `markup` (the title as StyledText, with its matched characters in `highlightColor`). While a list View is on top, the rows are its Items instead. `run(row)` runs a Result. `actions(row)`, `runAction(row, n)` and `runShortcut(row, key, modifiers)` give the Action panel. `view`, `depth` and `jobs` describe the top View and the running jobs, and `popView()`, `setText(field, text)`, `setChecked(field, checked)` and `submit()` work them. The signals `openRequested`, `toggleRequested` and `closeRequested` carry the `kotodama` command and the Daemon's requests. When it gets `closeRequested`, a window must hide and then call `closed()`, so the Daemon types or pastes into the window underneath and not into the menu.

```qml
import Quickshell
import Kotodama

ShellRoot {
    KotodamaMenu {
        // The look follows the host shell, e.g. from a matugen palette.
        background: "#1e1e2e"
        foreground: "#cdd6f4"
        accent: "#89b4fa"
        fontFamily: "Inter"
    }
}
```

The other look properties are `dimmed`, `selection`, `border`, `error`, `fontSize`, `radius`, `menuWidth`, `rowHeight`, `maxRows`, `top` and `placeholder`.

A compositor keybind opens it through the `kotodama` command, which asks the Daemon to pass the request on to every Front end:

```
# Hyprland
bind = SUPER, Space, exec, kotodama toggle
bind = SUPER, V, exec, kotodama open --view clipboard/history
```

`kotodama open [--view <extension>/<item>] [--query <text>]` opens it with text already typed. `--view` opens it on a View: it names an Item by its Extension and id, and the Daemon runs that Item's primary Action, such as "Show Clipboard History" on the clipboard's `history` Item, and opens the menu once the View it pushes is there, so the menu never shows the search first. Esc pops the View to the search. If the name matches no Item, or the Action fails, the menu opens on the search and says why. If the View hasn't arrived within a second (an Extension that is slow to start, say), the menu opens on the search, and the View still goes on top when it arrives. To try it, build the module and point Quickshell at it:

```sh
QML_IMPORT_PATH=$(scripts/qml-module --release) quickshell --path shell.qml
```

With `KOTODAMA_LOG=debug` in Quickshell's environment, the module logs how long each query took from the keystroke to the list model showing its Results.

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
| `just test` | Runs the tests; extra arguments go to `cargo nextest run` |
| `just bench` | Runs the benchmarks; extra arguments go to `cargo bench` |
| `just e2e` | Runs the e2e tests against headless sway, or only those named (`just e2e menu`); `just e2e --update menu` rewrites a test's reference image |
| `just image` | Rebuilds the dev container image locally, e.g. after editing `docker/` without committing |

Nix only sees files that git tracks, so `git add` new files before running a recipe.

### e2e tests

`e2e/run` starts sway with the headless backend and the pixman renderer on one 640x480 output, then runs every `e2e/*/test.sh` against it. A test starts its clients with `spawn`, types with `wtype`, and checks the screen with `expect_screen reference.png`, which compares a grim screenshot with the stored image. Each test's screenshots, diff images and logs go in `e2e/out/<test>`, which CI uploads as an artifact.

Hyprland can't run here: it needs a DRM device. Its live tests will run in the NixOS VM test (`checks.<system>.nixos-vm`), which already installs Kotodama through the home-manager module and checks socket activation, the settings, and that apps started from the menu outlive the Daemon.

### CI

GitHub Actions runs `check` and `test`, and `e2e` as a separate job, on x86_64 and arm64. Each restores a cargo cache keyed on `Cargo.lock` and `flake.lock`, which only runs on `main` save. On pull requests, `e2e` skips the `latency` test (set `E2E_SKIP` to a space-separated list of tests to leave out a full run), and the NixOS VM job (x86_64) doesn't run: both need a full build that no cache can save, so they run after every merge to `main` instead. The bench job (x86_64 only) compares the benchmarks of the change with those of the base commit, and fails when one is more than 50% slower or the Daemon's resident memory is more than 10% higher. On `main`, it runs both commits on the same runner, one after the other, and saves the new commit's results in the Actions cache. On a pull request whose base has saved results, it runs only the change, with shorter criterion times (`--warm-up-time 1 --measurement-time 2`), and compares it with them. Shared runners differ in speed by up to 2x, so when a timing is over the limit against results from another runner, the job runs the base commit on its own runner and compares again; resident memory doesn't depend on the runner. Without saved results, both commits run on the same runner, as on `main`. The log says which way it went: lines starting with `bench:` name the path, the CPU the saved results came from, and this runner's CPU.

On pull requests, a `changes` job skips the jobs a change can't affect: a change only to Markdown, `docs/` or `LICENSE` runs none of them, and one only to `e2e/` or `examples/` runs only `e2e`. The bench job also skips changes that can't move a benchmark: crates no benchmark builds on (`scripts/bench-crates` lists those it does), the dev image, the NixOS tests and the release config. Any other path runs everything. The `ci` job is the one required check. It passes when every job it needs succeeded or was skipped.

On pushes to `main`, CI publishes:

- the dev container image to **`ghcr.io/suplenc/kotodama-dev`** (linux/amd64 and linux/arm64), tagged `latest` and with git's hash of `docker/`, which is the tag `just` uses;
- Nix build outputs to the **`kotodama`** Cachix cache, using the `CACHIX_AUTH_TOKEN` repository secret.

CI and the dev container read from that cache. To use it with your own Nix, add it to `/etc/nix/nix.conf` (or run `cachix use kotodama`):

```
extra-substituters = https://kotodama.cachix.org
extra-trusted-public-keys = kotodama.cachix.org-1:8/a1e4X26O1hz2m5HFJBfKQ+P8m3GL7Z3Iy+xRIeZWE=
```

### Releases

release-please (`.github/workflows/release-please.yml`) reads the Conventional Commits that reach `main` and keeps a release PR open. For now it only runs when started by hand (Actions → Release → Run workflow), until the setup below is done. The PR bumps the workspace version in `Cargo.toml` and `Cargo.lock` and adds the changelog. Merging it tags the release. Before 1.0, a breaking change bumps the minor version. Two things must be set up first: the repository setting that allows GitHub Actions to create pull requests, and a `RELEASE_PLEASE_TOKEN` secret so that CI runs on the release PR (a PR opened with the default token starts no workflows).
