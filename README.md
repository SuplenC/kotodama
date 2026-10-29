# Kotodama

言霊, "the spirit of words": the old belief that spoken words make things happen.

Kotodama is a command menu for Wayland desktops. You type, and it finds and does things: launches apps, switches windows, runs scripts, opens projects, does math, pastes from clipboard history, controls the system. It is built to be faster than anything else of its kind, and everything beyond the core comes from Extensions.

- **`kotodamad`**, a Rust daemon, owns the index, the ranking and the Extensions, and stays warm between uses.
- **A Quickshell QML module** written in Rust (cxx-qt) gives QML a native result model. QML only draws the results.
- **Extensions** are separate processes in any language. They speak a protocol whose wire format is negotiated (JSON, MessagePack, Protocol Buffers or postcard), so a crashing Extension never takes down the menu. From protocol 1.0 on, a newer Daemon keeps talking to an Extension built for an older version. [docs/extensions.md](docs/extensions.md) is the guide to writing one, and `kotodama extension new` starts one.

Status: early, before v1. The protocol, the Daemon, the Quickshell Front end with its Views and Action panel, and the apps, clipboard history, calculator, emoji, folders, Hyprland, system, web search and AI Bundled extensions work. See `docs/adr/0001-architecture.md` and the issues.

## Installing

Kotodama ships as separate packages, each versioned and released on its own (see [Releases](#releases)); the code of all of them lives in this repository. The flake builds each one alone:

| Package | Version of | What's in it |
| --- | --- | --- |
| `kotodama-core` | `kotodama` (the `kotodama` and `kotodamad` crates) | the `kotodama` command, the Daemon, and its systemd user units in `lib/systemd/user/`; no Extensions |
| `kotodama-qml` | `kotodama-qml` | the Front end: the `Kotodama` QML module, in `lib/qt-6/qml/`, and the ready menu, a Quickshell config, in `share/kotodama/quickshell/` |
| `kotodama-<name>` | `kotodama-<name>` | one Bundled extension, in `share/kotodama/extensions/<name>/`: `apps`, `calculator`, `clipboard`, `emoji`, `web-search`, `ai`, `folders`, `hyprland`, `system` |

`default`, also `kotodama` and `kotodama-full`, puts them all together, as one package with three outputs: `out` (the command, a Daemon that finds the Bundled extensions, its units; also the `kotodamad` package), `qml` (the Front end) and `extensions` (every Bundled extension; also the `kotodama-extensions` package). Its `withExtensions` makes the same with only the Extensions you pick: `kotodama.withExtensions (exts: [ exts.apps exts.calculator ])`. The overlay's `pkgs.kotodamaPackages` holds the separate packages.

The Daemon finds an Extension wherever it is installed on `XDG_DATA_DIRS` (below), so `kotodama-core` with `kotodama-<name>` packages in a profile works too, where the Daemon's environment has that profile: on NixOS, the system's and the home-manager profile are there.

Which compositors it works on is in [Compositor support](docs/try-it.md#compositor-support). To package it without Nix, see [docs/packaging.md](docs/packaging.md). To try it on a Hyprland desktop step by step, with the checks to run by hand, see [docs/try-it.md](docs/try-it.md). With home-manager:

```nix
{
  inputs.kotodama.url = "github:SuplenC/kotodama";

  # in your home-manager configuration
  imports = [ inputs.kotodama.homeModules.default ];
  programs.kotodama = {
    enable = true;
    # The menu runs in a Quickshell of its own, with the graphical session
    # (`frontEnd.enable`, on unless home-manager runs a Quickshell of yours).
    # apps, calculator, clipboard, emoji and web-search are on unless turned off.
    extensions.hyprland.enable = true;
    # Written to ~/.config/kotodama/home-manager.toml; any other *.toml there still applies.
    settings = {
      extensions.apps.terminal = [ "ghostty" "-e" ];
      quicklinks.gh = { title = "GitHub"; url = "https://github.com"; };
    };
  };
}
```

`extensions.<name>.enable` picks the Bundled extensions the module installs; one left off is not installed, so the Daemon never starts it, and setting its `[extensions.<name>]` anyway gives a warning:

| Option | Unset |
| --- | --- |
| `extensions.{apps, calculator, clipboard, emoji, web-search}.enable` | on |
| `extensions.{folders, hyprland, ai, system}.enable` | off |
| `extraExtensions` | `[ ]` |
| `strictExtensions` | `false` |

`extraExtensions` installs Extensions that aren't Bundled, a distro's or your own, in any language, each a package holding `share/kotodama/extensions/<name>/` with its manifest: `extraExtensions = [ pkgs.kotodama-notes ];`. They go in beside the enabled Bundled ones, through the package's `withExtensions`, which refuses, by name, one that needs the build-time Qt at run time. Anything else it needs, such as a shell for a script or libstdc++ for C++, it may have. `strictExtensions = true` holds them to the check the Bundled extensions pass: it also refuses one that needs a runpath to GCC's `lib` output (a Rust binary needs only `stdenv.cc.cc.libgcc`'s libgcc_s), a shell or the full curl.

Every config table below also has typed options, so a typo or a value of the wrong type fails the build rather than being logged at runtime. They merge into `settings`, and an option left unset is left out, so the default applies. The Quicklinks and Aliases, keyed by your own names, stay in `settings`.

| Option | Config key |
| --- | --- |
| `history.{picksToLearn, picksToTakeOver, halfLifeDays, liftThreshold, timeOfDay, usageLog, undoSeconds, maxResults, maxQueries}` | `[history]` |
| `paths.{extensionDirs, defaultExtensionDirs, stateDir, cacheDir, socket}` | `[paths]` |
| `search.{rows, nearMisses, fallbacks}` | `[search]` |
| `extensions.{retryMaxSeconds, giveUpAfter}` | `retry_max_seconds` and `give_up_after` in `[extensions]` |
| `desktop.{settleMs, paste}` | `[desktop]` |
| `log.level` | `[log]` |
| `emptyQuery.{mostUsed, menu}` | `[empty_query]` |
| `ranking.kinds` | `[ranking]` |
| `quicklinkDefaults.opener` | `[quicklink_defaults]` |
| `extensions.<name>.{word, inSearch, boost}`, for every Extension above and below, and `system` and `hyprland` (`prefix` is the old name of `word`, still read) | `word`, `in_search` and `boost` in `[extensions.<name>]` (see [Apps](#apps) and [Ranking that learns](#ranking-that-learns)) |
| `extensions.apps.{terminal, hidden, names, withdrawWithin}` | `[extensions.apps]` |
| `extensions.clipboard.{maxEntries, maxAgeDays}` | `[extensions.clipboard]` |
| `extensions.calculator.{network, ratesUrl, decimalSeparator}` | `[extensions.calculator]` |
| `extensions.emoji.skinTone` | `[extensions.emoji]` |
| `extensions.web-search.engines` | `[extensions.web-search]` |
| `extensions.ai.{apiKeyCommand, apiKeyFile, url, model, effort, debounceMs}` | `[extensions.ai]` (see [AI](#ai)) |
| `extensions.folders.{folders, terminal, open, shell, lazygit, fileManager, copyPath}` | `[extensions.folders]` |

Each option is the key in camelCase. `decimalSeparator`, `skinTone` and a folder's `kind` take only the values their Extension knows, a `word` only one word, and numbers are checked against the ranges the tables below give. The Hyprland and system Extensions have no settings of their own.

```nix
programs.kotodama = {
  history.usageLog = true;
  emptyQuery.menu = [ { extension = "apps"; item = "firefox"; } ];
  quicklinkDefaults.opener = [ "firefox" "--new-tab" ];
  extensions = {
    apps.hidden = [ "qt5ct" ];
    calculator.decimalSeparator = "comma";
    emoji.skinTone = "medium";
    folders = {
      enable = true;
      folders = [ { path = "~/Projects"; kind = "projects"; depth = 2; } ];
    };
    web-search.engines = [ { name = "Kagi"; url = "https://kagi.com/search?q={query}"; } ];
  };
};
```

The module installs the package, with the enabled Extensions alone (its read-only `finalPackage`), and enables `kotodamad.socket`. `frontEnd.enable` adds the ready Front end: a systemd user service, `kotodama-menu`, that runs the menu in a Quickshell instance of its own, beside whatever shell the desktop already runs, so a compositor bind running `kotodama toggle` is all that's left to set up. It is on unless home-manager runs a Quickshell of your own (`programs.quickshell.enable`), where you place `KotodamaMenu` yourself.

| Option | What it does |
| --- | --- |
| `frontEnd.enable` | Runs the `kotodama-menu` service. On unless `programs.quickshell.enable`. |
| `frontEnd.quickshellPackage` | The Quickshell that runs it. Unset, the one from the same nixpkgs as `package` (its `quickshell` attribute), which shares the QML module's Qt. |
| `frontEnd.target` | The systemd user target that starts and stops it. Unset, `wayland.systemd.target`, which is `graphical-session.target` unless set. |
| `frontEnd.look.{background, foreground, dimmed, accent, selection, border, error}` | `KotodamaMenu`'s colors, as QML takes them (`"#1e1e2e"`, `"red"`). |
| `frontEnd.look.{fontFamily, fontSize, radius, menuWidth, rowHeight, maxRows, top, placeholder, showHints}` | The rest of its look, in pixels where it's a size. |
| `qmlImportPath` (read-only) | The QML import path that holds the `Kotodama` module, for a Quickshell config of your own (see [The Quickshell Front end](#the-quickshell-front-end)). |

A look property left unset keeps `KotodamaMenu`'s default. The service starts with the target, which needs something to start it once the compositor has handed `WAYLAND_DISPLAY` to the user manager: uwsm does, and so does home-manager's `wayland.windowManager.hyprland` with its systemd integration (on by default). Without either, start it from the compositor: `exec-once = systemctl --user start kotodama-menu`, after the environment import below. Its only extra environment is `QML_IMPORT_PATH`, for this Quickshell alone.

```nix
programs.kotodama.frontEnd = {
  enable = true;
  look = {
    background = "#1d2021";
    accent = "#d79921";
    fontFamily = "Inter";
    fontSize = 14;
  };
};
```

On NixOS without home-manager, add the package to `systemd.packages` and `environment.systemPackages`, and set `systemd.user.sockets.kotodamad.wantedBy = [ "sockets.target" ]`. The ready Front end then runs as `QML_IMPORT_PATH=<qml>/lib/qt-6/qml quickshell --path <qml>/share/kotodama/quickshell`, where `<qml>` is the package's `qml` output and `quickshell` is `kotodama.quickshell`, from the same nixpkgs.

The QML module links against Qt, so it shares Qt with a Quickshell built from the same nixpkgs. With a Quickshell from any other nixpkgs (another channel, or Quickshell's own flake), Kotodama's package brings a second Qt, about 760 MiB. Build Kotodama with your Quickshell's nixpkgs instead, and only the 4 MiB module is new:

- If your Quickshell comes from your own `nixpkgs` input, have Kotodama follow it: `inputs.kotodama.inputs.nixpkgs.follows = "nixpkgs";`.
- Otherwise, apply `inputs.kotodama.overlays.default` to the nixpkgs your Quickshell is built from (for Quickshell's flake, `inputs.quickshell.inputs.nixpkgs`) and use `pkgs.kotodama`. With home-manager, set `programs.kotodama.package = pkgs.kotodama;`.

That nixpkgs must be recent enough to build Kotodama; nixos-25.11 is. Its Quickshell version doesn't matter, since the overlay builds only Kotodama.

The Daemon starts the first time something connects to its socket, whether a `kotodama toggle` or the Quickshell Front end, and stays running after that. While it is stopped, systemd holds new connections and starts it again. It runs with the environment of the systemd user manager, not the compositor's:

- **Desktop notifications** (a job finishing) go over the session bus at `DBUS_SESSION_BUS_ADDRESS`, or `$XDG_RUNTIME_DIR/bus` when that isn't set. That is where systemd's per-user bus lives, so this works with no setup.
- **Typing, pasting and copying** need `WAYLAND_DISPLAY`, and the apps it starts need the rest of the graphical session's environment. The compositor must hand these to the user manager before the menu is first used, since the Daemon reads its environment once, when it starts. uwsm does this. In a plain Hyprland session, add `exec-once = dbus-update-activation-environment --systemd --all` (or `systemctl --user import-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP`). If the Daemon started before that, run `systemctl --user restart kotodamad`.

The `kotodamad` of the `default` package (and of `withExtensions`) puts its Bundled extensions first on its own `XDG_DATA_DIRS`, followed by the inherited directories (or `/usr/local/share:/usr/share` when it isn't set). The rest of the session's `XDG_DATA_DIRS` is left alone.

## The Daemon

`kotodamad` serves Front ends on `$XDG_RUNTIME_DIR/kotodama.sock`, or on `KOTODAMA_SOCKET` when it is set. It finds Extensions in `$XDG_DATA_DIRS/kotodama/extensions/*/manifest.toml` and `~/.config/kotodama/extensions/*/manifest.toml`, starts each one, and restarts it with backoff when it stops or hangs. Extensions installed, changed or removed while it runs are picked up without a restart. Its config is every top-level `*.toml` in `$XDG_CONFIG_DIRS/kotodama/`, then `~/.config/kotodama/`, merged in order and reloaded when a file changes, a symlinked file included when the file it leads to changes; `kotodamad --config <dir>`, or `KOTODAMA_CONFIG_DIR`, reads that one directory instead. `KOTODAMA_LOG=debug` logs more. Under systemd socket activation (`kotodamad.socket`) it serves the socket systemd hands it instead of binding its own.

What it finds wrong that you can fix is a problem: a config file that doesn't parse (the last good config stays), a setting that isn't one or has a bad value, a Manifest it refuses, a keyword two things ask for, an Extension that can't start, speaks a protocol it doesn't, or keeps stopping. The menu's footer counts them, and a click on the count, or the `problems` Result, lists them. `kotodama check` prints them, one a line, and exits with 1 when there is any. An Extension that can't start, or that speaks another protocol, isn't restarted until its program or its Manifest changes.

Apps, Quicklinks and every other program started for the user get a systemd scope of their own in `app.slice`, asked of the user manager over D-Bus as `systemd-run --user --scope` would, so each outlives a stop or restart of the Daemon, whose service would otherwise take every process in its cgroup down with it. The program moves into its scope before it runs, so what it starts at once lands there too. An app's scope is named for its desktop id, as in `app-kotodama-firefox-<pid>.scope`, following the systemd and XDG convention, so tools that group processes by app see the right name; a program with no desktop id, such as a Quicklink's command, gives the scope its file name. Without a user manager, programs start without a scope. `KOTODAMA_LAUNCH_PREFIX`, when set, replaces the scope: programs run after its words instead, such as `uwsm app --`, or start directly when it is empty. Extensions inherit it from the Daemon and should do the same; the Rust ones get both through `kotodama-extension`. When the Daemon has no prefix and there is a user manager, it hands Extensions `systemd-run --user --scope --slice=app.slice --collect --quiet` as their prefix, with `KOTODAMA_LAUNCH_SCOPED=1`, so an Extension in any language that starts programs through the prefix gets the scope too; one that asks for the scope itself, as the Rust ones do, ignores a prefix marked so. The words are split on whitespace, so the prefix can't contain a path with spaces.

### The config

`kotodama config` prints the config the Daemon would run with: every key, the default where nothing sets it, and which file set each one. `kotodama config --schema` prints every key with what it does. A table or key that isn't one, or a value out of range, is a problem, and the default applies.

Every setting reloads with the config, except `state_dir`, `cache_dir` and `socket`, which are read once when the Daemon starts; changing one logs that a restart is needed. The values shown are the defaults:

```toml
[paths]
extension_dirs = []            # more Extension directories, read after the default ones
default_extension_dirs = true  # false reads only extension_dirs
# state_dir = "~/.local/state/kotodama"  # the History and the Usage log; at start
# cache_dir = "~/.cache/kotodama"        # the Items that wait unshown; at start
# socket = "/run/user/1000/kotodama.sock"  # at start; KOTODAMA_SOCKET wins

[search]
rows = 10           # rows a search fills until the Front end says how many it shows; 1 to 50
near_misses = true  # Results a typo away when real matches leave rows empty
fallbacks = true    # ask fallbacks, such as web search, what real matches leave empty

[extensions]
retry_max_seconds = 30  # the longest wait before a crashed Extension restarts; waits double from 100 ms
give_up_after = 0       # crashes in a row after which it stays down until its program or Manifest changes; 0 never gives up

[desktop]
settle_ms = 50    # how long keys wait after the menu closes; 0 to 5000
paste = "ctrl+v"  # or "ctrl+shift+v", which terminals take, or "type", which types the text

[log]
level = "info"  # "error", "warn", "info" or "debug"; KOTODAMA_LOG wins

[files]
# recent_dirs = ["~/Downloads", "~/Documents", "~/Desktop"]  # where a form's file field finds the newest files; the XDG user dirs by default
```

A path in `[paths]` is absolute or starts with `~/`. An Extension in a later directory replaces one of the same name before it, so `extension_dirs` can hold a build of a Bundled extension. `[paths]` wins over the XDG variables, and an environment variable named for one setting, `KOTODAMA_SOCKET`, wins over `[paths]`, so Front ends started with it find a moved socket.

`[extensions.<name>]` holds an Extension's own settings, which it gets, and the Daemon's keys, which it doesn't: `enable`, `command`, `env`, `live_query`, `word`, `prefix`, `in_search` and `boost`.

```toml
[extensions.emoji]
enable = false  # never started, though still installed

[extensions.apps]
command = ["/home/me/src/kotodama/target/debug/kotodama-apps"]  # replaces the Manifest's
env = { RUST_LOG = "debug" }
live_query = { debounce_ms = 0 }  # replaces the Manifest's `prefixes`, `triggers` or `debounce_ms`
boost = 1.5                       # its Results' points count 1.5 times; see Ranking that learns

# No Extension named notes is installed, so this table defines one: the
# Manifest's keys go here too, and the rest are its settings.
[extensions.notes]
command = ["notes-kotodama"]  # an absolute path, or found on PATH
title = "Notes"
word = "note"
folder = "/home/me/notes"     # a setting the Extension gets
```

A bare program name in a `command` from the config is looked up on `PATH`, which a Manifest's never is. A changed `command`, `env` or `live_query` restarts the Extension.

### Ranking that learns

Results rank by how well they match before anything else, in tiers: the whole title (or the Item's own `keyword`) is the query, then the query is the initials of consecutive words (`vsc` for Visual Studio Code), a word starts with it, or it is one of the Item's other keywords, then the query is inside a word, then its letters are scattered. Within a tier, Frecency decides, then the kind of Result (an app before a project, a project before a bookmark (a `url` Item), anything else, then a file), then nucleo's score, then the shorter title. Neither case nor accents matter (`zolw` finds Żółw), every word of the query must match in any order, a single letter matches only at the start of a word, and a trailing space makes the last word a whole word. A subtitle is searched once the query has 3 letters, and only at word starts.

A query with a typo still finds what was meant: `fierfox`, `firfox` and `firefx` find Firefox. Each word of the query may have one typo per 4 letters (a letter missing, extra or wrong, or two letters swapped), but never in its first letter. A Result found only with typos is a near miss, the tier below scattered letters. Use never lifts it, so every real match ranks above it. Near misses are looked for only when the real matches leave rows on screen empty.

Every Action the user runs is recorded in the History, `$XDG_STATE_HOME/kotodama/history.json` (by default `~/.local/state/kotodama/`), except on an answer, such as the calculator's: an answer takes the top row whatever its Frecency, and is a new Result for every expression, so the History keeps nothing of it. A picked Result gets a Frecency boost, which halves every 14 days but never quite reaches zero. Frecency also learns the time of day: picks count in full at the times of day the Result is usually picked, and half as much at a time of day it never is, so something used every morning ranks higher in the morning. A Result used about daily for a week ranks one tier higher, and two tiers after about six times as much use, but a match inside a word or scattered never passes an exact title: only a title that starts with the query does. When the user picks the same Result 3 times for one query, it becomes that query's Learned choice: it ranks first for that query and for the queries that extend it or that it extends (`fi`, `fir`, `fire`), but only over the Results that ranked above it when it was picked, so a new app with exactly that title still wins. Two picks of another Result since the Learned choice was last picked take over from it. An Extension's `boost` in `[extensions.<name>]` (default 1, any number above 0) multiplies its Results' points, the blend of their tier and their Frecency: `1.5` puts its Results above equal matches from other Extensions, `0.5` below them. A boost never takes a Result past an exact title on its own, nor a real match below a near miss. Learning fades with Frecency's half-life; when two Learned choices apply, the one picked more often wins, then the one learned for the closer query. Both apply to Indexed items and Live query results. Items that several Extensions offer with the same Identity (a page from bookmarks and from browser history) share their Frecency and Learned choices: picking one raises the others. Each still shows as a Result of its own, and the one the user picks through most takes the best place any of them ranks in. The "Clear history" Result empties the History.

The Action panel of a Result the History keeps anything for offers "Forget", which drops its Frecency, every Learned choice for it, the picks of the Aliases that name it and, for a Result with an Identity, the History of that thing from every Extension. It also offers "Clear the history of" its Extension, which forgets every Result of that Extension, the Aliases that name them, and the shared History of the Identities its Items carry, though another Extension offers them too: the History doesn't know which Extension a pick of a shared thing came through. Another Extension's Result of such a thing keeps only its own picks, which decide its place among the Results of that thing. Ctrl+Shift+Backspace forgets the selected Result without opening the panel. For a minute (`undo_seconds`) after Forget, clearing an Extension's History or "Clear history", the menu shows a Toast such as "Forgot “Firefox”", and Ctrl+Z puts back what was forgotten; typing `undo` finds a Result that does the same, latest first. Anything picked since stays as it is.

These numbers are settings in the config's `[history]` table, which reloads with the config. The values shown are the defaults:

```toml
[history]
picks_to_learn = 3      # picks of one Result for one query that make it the query's Learned choice
picks_to_take_over = 2  # picks of another Result, since the choice was last picked, that replace it
half_life_days = 14     # how long a pick takes to count half as much; above 0
lift_threshold = 5.0    # the Frecency worth one tier, about a week of daily use; each further tier takes 1 + this times as much; above 0
time_of_day = 0.5       # how much less picks count at a time of day the Result is never picked; 0 to 1
day_of_week = 0.3       # how much less picks count on a weekday or a weekend the Result is never picked on; 0 to 1
usage_log = false       # log queries and picks, to tune ranking from real use
undo_seconds = 60       # how long Forget and Clear history can be undone; 0 to 86400
max_results = 2000      # the most Results kept; past it the least used go
max_queries = 1000      # the most queries whose Learned choice is kept; past it the least recent go
```

A setting that doesn't parse is a problem and keeps its default. With `usage_log = true`, every query typed and every Action run is appended to `$XDG_STATE_HOME/kotodama/usage.jsonl`, one JSON object per line, which only the user can read and which never leaves the machine:

```json
{"at":1760870400,"kind":"query","query":"fi"}
{"at":1760870401,"kind":"pick","query":"fi","extension":"apps","id":"firefox","action":"open","row":1}
```

`at` is seconds since the Unix epoch; a pick's `extension` and `id` name the Result, `action` the Action run, and `row` where the Result was in the list (from 0), or `null` if it wasn't in it. Emptying the search box isn't logged as a query, but a pick made from the Empty query is, with `"query":""`. When the file would pass 4 MiB it becomes `usage.jsonl.1`, replacing the older one, so the log never takes more than 8 MiB. Turning it off keeps what was written; delete the files to forget it. "Clear history" deletes both files, and clearing one Extension's History drops the picks of its Results from them. Forgetting one Result leaves the log as it is, and Undo puts back the History, never the log.

With nothing typed, the menu shows the Empty query: the 3 Results most used at this time of day, by their picks within a few hours of now, then by all their picks. An Alias's picks count for the Result it names, and a thing that several Extensions offer shows once, as the Result picked through most. The Fixed menu follows them, set in the config. Without one, the apps' entry Items follow instead, so a first run, with nothing picked yet, shows what there is to enter; `menu = []` shows nothing after the most used:

```toml
[empty_query]
most_used = 3  # how many of the most used come first; 0 to 50
# Then these, in this order, each unless it is shown already.
# Unset, the apps' entry Items.
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

# The opener: the program, and its first arguments, that a URL opens with.
[quicklinks.mail]
title = "Work mail"
url = "https://mail.example.com"
opener = ["firefox", "-P", "work"]

# The opener of every URL Quicklink that names none.
[quicklink_defaults]
opener = ["firefox", "--new-tab"]
```

Typing an Alias puts its Result first, ahead of a Learned choice, and what follows it is the Result's Argument; part of an Alias does nothing. Picks through an Alias count for the Alias, not for the Result's own Frecency. A Result reached through an Alias carries a badge naming it. An Alias whose Item is gone still shows a row, greyed out, saying so, and one whose Extension is down shows its Item as last seen, greyed out; neither runs.

Typing a Quicklink's keyword ranks it as an exact match, with Frecency deciding between equals; its row reads `Search YouTube for “cats”`, the keyword alone opens it with an empty `{query}`, and words after a keyword whose template has no `{query}` are ignored. A Quicklink is also found by its title, ranked like any other title: `youtube` finds "Search YouTube". A URL opens with the Quicklink's `opener`, the URL its last argument, else with the `opener` of `[quicklink_defaults]`, else with `xdg-open`; a browser and a profile, such as `["chromium", "--profile-directory=Work"]`, or any command will do. A Quicklink with a `command` takes no opener. A command runs without a shell. The Daemon's own Results, the Quicklinks, "Clear history" and the Undo Result, belong to the reserved Extension name `kotodama`, and so do the Actions it offers on every Result, `forget` and `clear-extension-history`.

### Apps

Every Extension is an app: the search over its Items alone. The search shows one entry Item for it, such as "Emoji" or "Projects", found by the app's title and its word (`emoji`, `projects`, `calc`) and ranked like any other Result, though never above a word start: an application such as GNOME Calculator ranks above the Calculator app for `calc` until the entry Item is picked a few times. Tab or Enter on the entry Item enters the app, so `projects` and Tab is enough; with more typed after the word, as in `emoji fire`, the row reads "Search Emoji for “fire”" and entering keeps `fire` typed. The menu shows a chip naming the app in front of the search box, and, until something is typed, an intro card from the Manifest: what the app does, and a few queries to try with what they give. Under it are the app's commands, then its most picked Items, then the rest. Typing searches that Extension's Items alone, and an Extension with Live queries gets every query typed in its app, whether its triggers match or not: the calculator answers anything typed in it. Esc clears what is typed; with nothing typed, it goes back to the whole search. Backspace on the empty box goes back too, with the app's word typed. `kotodama open --app emoji` opens the menu in the app. Learned choices in an app are kept apart from the same words in the search.

Each Item is in the search, or in its app only: an Item with `app_only = true` only its app shows. The search reads one Segment of each Extension's Items and the app reads two, so the choice costs nothing per row on a keystroke. An Extension's Live queries and Fallbacks are asked in its app, and in the search unless it has every Item in the app only (`in_search = false`).

An Extension's Manifest names its app and says what goes in it:

```toml
word = "calc"                    # the app's word; unset, the Extension's name
title = "Calculator"             # the entry Item's title; unset, the name
icon = "accessories-calculator"
about = "Arithmetic, units, currency, dates and time zones. Enter copies the answer."
examples = [
    { query = "15min in sec", result = "900 secs" },
]
commands = ["history"]           # Items of its own shown first in the app
in_search = false                # every Item in the app only, whatever it says
```

The config overrides the word and the visibility, for any Extension:

```toml
[extensions.emoji]
word = "e"          # one word: `e`, then Tab, searches the emoji
in_search = true    # the emoji in the search too

[extensions.folders]
in_search = false   # projects and scripts only in their app
```

`in_search = true` shows every Item of the Extension in the search, `false` none of them; unset, the Manifest's `in_search` or, without one, each Item says. A keyword has one holder, in this order: an Alias, a Quicklink, a word set in `[extensions.<name>]`, a word set in a Manifest, an Extension's name; between two apps, the Extension whose name sorts first. An app that loses its word goes by its Extension's name, or the name and `-2`, `-3`, so it stays reachable, and the Daemon logs a warning for every keyword lost. `docs/adr/0028-one-keyword-table-with-a-fixed-precedence.md` gives the reasons. `docs/adr/0027-every-extension-is-an-app-with-items-in-the-search-or-in-the-app-alone.md` explains the design, and how an app differs from a Hidden item and a View.

### Trying it

To try it from a terminal, run the Daemon, then the bare example Front end:

```sh
cargo run -p kotodamad
cargo run -p kotodamad --example front-end -- firefox        # print the Results
cargo run -p kotodamad --example front-end -- --run firefox  # run the top one
```

### Performance

`just bench` measures three things with 10k Indexed items:

- `query_10k_items`: from a query arriving to its Results encoded for the Front end, in process (criterion).
- `instructions`: the instructions the same keystrokes run, counted under Valgrind by gungraun. The count is the same on every run on one machine, so a change of a few percent shows where the timings can't tell it from noise. CI fails when a keystroke runs more than 5% more instructions than on the base commit.
- `daemon`: the real `kotodamad` with a real Extension process: the round trip of 2,000 queries over the socket, and the Daemon's resident memory (`VmRSS`) afterwards.

| Machine | First keystroke | Next keystroke | Socket round trip p99 | Resident memory |
| --- | --- | --- | --- | --- |
| Ryzen AI 9 HX 370 (target) | to be measured | to be measured | to be measured | to be measured |
| Apple M-series, dev container (arm64 Linux VM) | 309 µs | 42 µs | 0.38 ms | 13.3 MiB |

The target is a p99 under 2 ms from query received to Results sent. CI fails when a change makes resident memory more than 10% higher than its base commit does.

The `latency` e2e test types into the real menu with 10k Indexed items and fails if any keystroke takes 8 ms (one frame at 120 Hz) or more from its query to the list model's update. In the dev container on an Apple M-series machine, under software rendering, the worst of 40 keystrokes takes about 5 ms.

## Bundled extensions

The Bundled extensions are written in Rust on `kotodama-extension` (`crates/kotodama-extension`), which any Rust Extension can use too. It does the handshake on stdio and reads the Daemon's messages on a thread of its own into one channel, which the Extension's own threads send to as well. It keeps resident memory down, sends Indexed items as changes, reads config settings and logs the ones of the wrong shape, starts programs detached from the Extension, each in a systemd scope of its own or through the launch prefix, and, with its `watch` feature, watches directories and acts once they settle after a change. Its crate docs show a whole Extension.

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
# An app that closes within this many seconds of starting was most likely
# picked by mistake: its pick is taken back out of the History. 0 never
# does; at most 30. The default is 5.
withdraw_within = 5
```

An app counts as closed once everything its launch started is gone: its whole scope, or without a user manager its process group. So a launcher script that starts the app and exits keeps the pick. A launch while a copy of the same program already runs keeps it too, since such a launch often only hands off to that copy. The check is still best effort: without a scope, a program that starts the app in a new session or process group and exits looks closed, and so does one that hands off to a copy running under another name, or to a D-Bus-activated service. Set `withdraw_within = 0` if that happens to apps you use.

Its `manifest.toml` runs `./kotodama-apps`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/apps/`.

### Clipboard history

`kotodama-clipboard` (`crates/kotodama-clipboard`) watches the Wayland clipboard through ext-data-control, or wlr-data-control on a compositor that has only that, and keeps what is copied in `$XDG_STATE_HOME/kotodama/clipboard/`, readable only by the user, so it survives a restart. It keeps text, and PNG and JPEG images with a 256-pixel thumbnail; when an offer has both, it keeps the text.

What a password manager copies is never read: an offer carrying the `x-kde-passwordManagerHint` type, which password managers add to mark a copy sensitive, is skipped whole. Entries go after 7 days, or once there are more than 1000, oldest first.

Each clip is an Item of kind `clipboard.clip`, found by the first 60 characters of its text, or titled with an image's size and shown with its thumbnail. Its primary Action, "Paste", pastes it into the window the menu was opened from; "Copy to Clipboard", "Show Details" (the text, or the thumbnail, in a detail View) and "Delete from History" follow. The "Clipboard History" Item lists every clip, newest first, and its second Action, "Clear Clipboard History", clears them all, on disk too. The clips and that Item are in the Clipboard app alone, where "Clipboard History" comes first; `in_search = true` puts them in the search too.

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

Arithmetic, units and currency come from [fend](https://github.com/printfn/fend), with each evaluation cut off after 50 ms. An approximate answer shows at most 12 significant digits, without trailing zeros: `42 km in mi` is `26.097590074 mi`. Dates and time zones are its own, as fend has no clock and no time zones: dates are `today`, `tomorrow`, `yesterday`, `2026-12-25`, `dec 25` or `25 december`, with or without a year (without one, the next such day, or for `days since` the last); amounts are days, weeks, months and years. A zone is an abbreviation (`PT`, `ET`, `UTC`, `CET`, `JST` and others, each standing for its region's zone all year), an IANA name, or the city one ends in (`Warsaw`, `New York`), read from the system's time zone database (`$TZDIR`, or `/usr/share/zoneinfo`). A time without a zone is local. Natural-language dates (`next friday`) are out of scope.

Decimals are written as the locale writes them (`LC_ALL`, `LC_NUMERIC` or `LANG`): in `de_DE` or `pl_PL`, `3,5 * 2` is `7`, `10 / 4` is `2,5`, and `1.000` is a thousand. The other separator only groups thousands, and only in groups of three: in an English locale `1,000 + 1` is `1001`, while `3,5 * 2` gets no answer rather than a wrong one, as does `3.5 * 2` where decimals take a comma. `decimal_separator` overrides the locale.

Exchange rates are the European Central Bank's, from [frankfurter.app](https://frankfurter.app). Nothing is fetched until the first currency query. Then the rates are fetched with `curl` and cached in `$XDG_CACHE_HOME/kotodama/calculator/rates.json`, and fetched again only once that is more than a day old; the query is answered as soon as they arrive. A failed fetch is tried again after 10 minutes.

```toml
[extensions.calculator]
# Fetch exchange rates. Off, currency uses the cached rates, however old, and
# nothing without them.
network = true
# Where the rates come from: frankfurter's API, or anything answering like it.
rates_url = "https://api.frankfurter.app/latest"
# How decimals are written, "dot" (3.5) or "comma" (3,5); the other separator
# groups thousands. Unset, the locale's.
decimal_separator = "comma"
```

### Emoji and symbols

`kotodama-emoji` (`crates/kotodama-emoji`, Extension name `emoji`) makes every emoji and the common symbols Items of kind `emoji.character`: arrows, math, punctuation, shapes and keyboard keys (⌘ ⌥ ⌃ ⇧ ⎋ ⏎ ⌫), every non-ASCII symbol CLDR names. Each is titled with the character and its CLDR name ("👍 Thumbs up") and found by the name and its CLDR keywords. They are in the Emoji app alone (see [Apps](#apps)): in it, `thumbs` and `like` find 👍, `arrow right` finds →, and `command` finds ⌘, while `fire` in the search shows no emoji. `in_search = true` puts them back in the search. Its primary Action, "Type", has the Daemon type the character into the window the menu was opened from; "Copy" puts it on the clipboard. An emoji with skin tones is one Item, with an Action to type each tone ("Type 👍🏽") after those two.

```toml
[extensions.emoji]
# The skin tone "Type" and "Copy" use: light, medium-light, medium, medium-dark
# or dark. Unset, emoji are their default yellow.
skin_tone = "medium"
# The word that names the Emoji app. Unset, emoji.
word = "emoji"
# Whether the search shows the emoji too. Unset, false: only the app does.
in_search = false
```

The characters are Indexed items, sent once at start, so the Daemon searches them as any other Items and ranks them by Frecency, with no round trip on a keystroke. They are Unicode Emoji 17 and CLDR 48's English names and keywords (Unicode License v3, `data/LICENSE-UNICODE`), kept compact in `data/emoji.tsv` (`data/update.sh` regenerates it), built into the binary and read in place.

Its `manifest.toml` runs `./kotodama-emoji`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/emoji/`.

### Web search

`kotodama-web-search` (`crates/kotodama-web-search`, Extension name `web-search`) is the fallback for what nothing else matches. When the real matches leave rows on screen empty (fewer Results, Near misses included, than the menu shows), the bottom rows read "Search DuckDuckGo for …", "Search Google for …" and "Search Wikipedia for …", and Enter opens the search with `xdg-open`, as a Quicklink opens. That is the "no good match" threshold: a query whose real matches fill the rows offers no search, since it would sit below them unseen, and the Daemon doesn't even ask for one. The searches rank below every real match, Near misses included, however often they are picked, and a real match that arrives late goes above those on screen; among themselves, the engine picked most goes first, but picking one never makes it a Learned choice (ADR 0016). They are Items of the shared kind `url`.

A query that looks like an address opens it from the top row, "Open example.com", whatever else matches, as the calculator's answer does. It counts as an address only if it is one word with:

- an `http://` or `https://` scheme and a host, taken as typed;
- a domain name such as `example.com` or `github.com/SuplenC/kotodama`, opened over `https://`, whose last label is a common generic top-level domain (`com`, `org`, `dev`, `app` and so on), a two-letter country code that isn't also a common file extension (`md`, `rs`, `py`, `sh`, `js` and so on are not), or not ASCII;
- `localhost` with a port, such as `localhost:8080`, or an IPv4 address, such as `192.168.1.1:3000`, opened over `http://`.

A port and a path may follow. `foo.bar baz`, `3.14`, `main.rs`, `Cargo.toml`, `localhost` and plain words are not addresses; any of them typed with `https://` is.

```toml
[extensions.web-search]
# Searched in this order. A template takes the query where `{query}` is,
# percent-encoded, as a Quicklink's does, so a Quicklink's URL works here as
# it is. These are the defaults; an empty list leaves only addresses.
engines = [
    { name = "DuckDuckGo", url = "https://duckduckgo.com/?q={query}" },
    { name = "Google", url = "https://www.google.com/search?q={query}" },
    { name = "Wikipedia", url = "https://en.wikipedia.org/w/index.php?search={query}" },
]
```

It is a Live query Extension whose `manifest.toml` sets `fallback = true` under `[live_query]`, with a debounce of 0 so it may answer with an address. Its triggers send it what may be an address on any keystroke; anything else it is sent only when the rows aren't full, so a keystroke whose real matches fill the rows costs nothing. It does no network calls. Its `manifest.toml` runs `./kotodama-web-search`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/web-search/`.

### AI

`kotodama-ai` (`crates/kotodama-ai`, Extension name `ai`) turns a request in plain words into Actions that other Extensions already offer. Type `?` and the request, `?set up a go project`, and once the query has been still for `debounce_ms` the model's proposals show as Results: "Install Go Suite", "New Go project (name: demo)". Each is a Proposal; Enter confirms it, and only then does the Daemon run the Action, as if you had picked it yourself, asking for any Argument the model left out. Nothing runs before you confirm, and the model never runs anything. A query that real matches leave rows empty for also gets an "Ask AI" row at the bottom, which asks nothing until picked; picking it opens a View that lists the proposals.

It is off until configured. With neither `api_key_command` nor `url` set it adds no rows, asks for nothing and makes no network calls.

```toml
[extensions.ai]
# Prints the API key. Its output is kept in memory only: never logged, never
# written to disk, and handed to curl on stdin, never on its command line.
api_key_command = ["pass", "show", "anthropic"]
# The Messages API endpoint. A local server that speaks the same API, such
# as Ollama (`http://localhost:11434/v1/messages`), works too, with no key.
url = "https://api.anthropic.com/v1/messages"
# The default is the fastest Claude model, as you wait on it with the menu open.
model = "claude-haiku-5-5"
# `output_config.effort`; "" leaves it out, for a server that doesn't take it.
effort = "low"
# How long a `?` query must be still before it is sent.
debounce_ms = 600
```

With home-manager, `programs.kotodama.extensions.ai` sets the same keys as typed options; an option left unset is left out, so the Extension's default applies. Keep the key out of the Nix store: point `apiKeyFile` at a secret read at runtime (an absolute path as a string, never a Nix path, which would be copied into the store), or set `apiKeyCommand`.

```nix
programs.kotodama.extensions.ai = {
  apiKeyFile = "/run/agenix/anthropic";   # api_key_command = [ "cat" path ]
  # apiKeyCommand = [ "pass" "show" "anthropic" ];   # or this, not both
  url = "https://api.anthropic.com/v1/messages";
  model = "claude-haiku-5-5";
  effort = "low";
  debounceMs = 600;
};
```

What goes to the model is the query and the Action catalog (each Action's title, Item kind, Arguments and Extension), never Items, the clipboard, file paths or anything else the menu shows. Its Manifest sets `action_catalog = true`, the only way it learns which Actions exist, as for any Extension. A query that changes, or a View left, kills the request in flight. Requests go through `curl`, so the Extension carries no TLS stack; it idles at 2.5 MiB resident. A proposal runs on the one Indexed item of its kind that offers the Action. When several do, the menu lists them to pick from, and the Action runs on the one picked, with the proposal's Arguments; Esc runs nothing. When none does, as for a Contributed action that needs an Item, the menu says to pick one in the search. Its `manifest.toml` runs `./kotodama-ai`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/ai/`.

### Folder sources

`kotodama-folders` (`crates/kotodama-folders`, Extension name `folders`) turns the contents of the folders named in its config into Items, in two kinds of folder: projects and scripts. It watches them, so a project cloned or a script added, renamed or removed while it runs shows up or goes within a moment. It watches only the folders it searched, each on its own, never the files inside a project, so a large projects folder costs no more than its repositories. Saving files inside a project searches nothing again.

**Projects** are the git repositories found down to the folder's `depth` (3 if not given), with their worktrees and checked-out submodules. A repository stops the search: nothing inside it is searched. Each is an Item of kind `project` with its path, titled with the name `tms` (tmux-sessionizer) gives it, so the menu and `tms` land in the same tmux sessions: the folder's name; `<repository>#<worktree>` for a worktree; `<repository>>`, then the folder's name, for a submodule; and, for a name found more than once, as many parent folders as tell them apart (`work/api`, `home/api`). The tmux session is that name with every `.` made `_`, for an `open` command that uses it (the example config below). Its primary Action, "Open", opens a terminal in the project's folder. "Open Terminal Here", "Open in Lazygit", "Open in File Manager" and "Copy Path" follow, then other Extensions' Contributed actions for `project`. "Open in Lazygit" is there only when the config sets `lazygit` or `lazygit` is on the Extension's `PATH`, checked when the config is read.

The terminal commands go through `xdg-terminal-exec`, which starts the user's preferred terminal, as the apps Extension does. A script runs in it under `sh`, which shows its exit status and waits for Enter, so the window stays open in any terminal.

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
# The commands a project's Actions run, in the project's folder: `{path}` is
# the folder and `{session}` its tmux session. These are the defaults.
open = ["xdg-terminal-exec", "--dir={path}"]
shell = ["xdg-terminal-exec", "--dir={path}"]
lazygit = ["xdg-terminal-exec", "lazygit", "--path", "{path}"]
file_manager = ["xdg-open", "{path}"]
copy_path = ["wl-copy", "{path}"]
```

An example config for Ghostty, with each project in its tmux session and Neovim in it, the session `tms` opens:

```toml
[extensions.folders]
folders = [{ path = "~/Projects", kind = "projects" }]
# What a script runs in; the script and its arguments follow. It should keep
# the window open once the script exits.
terminal = ["ghostty", "--wait-after-command=true", "-e"]
open = ["ghostty", "-e", "tmux", "new-session", "-A", "-s", "{session}", "-c", "{path}", "nvim"]
shell = ["ghostty", "--working-directory={path}"]
lazygit = ["ghostty", "-e", "lazygit", "--path", "{path}"]
```

Its `manifest.toml` runs `./kotodama-folders`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/folders/`.

### Hyprland

`kotodama-hyprland` (`crates/kotodama-hyprland`, Extension name `hyprland`) talks to Hyprland over its two sockets in `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`, never through `hyprctl`.

**Windows**: every open window is an Item of the shared kind `window` with its address, titled by its title, with its class and workspace beside. It reads the windows once at start, then follows Hyprland's event socket, so a window opened, closed, moved or retitled is sent again, alone, and focus changes send nothing. Its primary Action, "Focus Window", focuses it; "Close Window", "Move to Workspace" (which asks for the workspace) and "Toggle Floating" follow.

**Shortcuts**: Hyprland's binds come from Hyprland itself, so every bind is there, whether the config or `hyprctl keyword bind` added it; no config file is parsed. Every bind with a description (`bindd`) is an Item of kind `hyprland.shortcut`, titled by the description, with its keys beside, so searching "screenshot" in the Hyprland app shows `SUPER + SHIFT + S` next to it; its Action runs the bind. The binds and the "Keyboard Shortcuts" Item are in the app alone, so the search shows only the windows. The "Keyboard Shortcuts" Item opens a View of every bind, titled by its keys and grouped by modifier: binds without one first, then by how many modifiers and their names, a submap's binds last. Picking one runs it, except a mouse bind, which only the mouse runs. The binds are read again each time the View opens, and on a config reload, so a bind added at runtime is there the next time. To open the menu straight on it:

```
bind = SUPER, F1, exec, kotodama open --view hyprland/shortcuts
```

Outside Hyprland (no `HYPRLAND_INSTANCE_SIGNATURE`) the Extension sends nothing and waits, using no CPU. The Daemon, a systemd user service, finds Hyprland through the user manager's environment, so Hyprland's config should import it: `exec-once = systemctl --user import-environment HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY`. If Hyprland exits, its windows and binds are taken away.

Its `manifest.toml` runs `./kotodama-hyprland`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/hyprland/`.

### System

`kotodama-system` (`crates/kotodama-system`, Extension name `system`) powers the machine and the session through logind, and lists the open windows of any compositor with wlr-foreign-toplevel-management. It knows no compositor and no distro: logind is on every systemd system, and sway, river, labwc, niri, Wayfire and Hyprland all speak the protocol.

**Power and session**: "Power Off", "Reboot", "Suspend", "Hibernate", "Lock Screen" and "Log Out" are Items of kind `system.power`, found by the words people type for them too: `shutdown`, `restart`, `sleep`, `lock`, `logout`, `sign out`. Each calls logind (`org.freedesktop.login1`) on the system bus. "Power Off", "Reboot" and "Log Out" close every app, so Enter on them opens a small form first, and a second Enter runs it; Esc leaves it. "Suspend" and "Lock Screen" lose nothing and run at once. "Hibernate" is listed only when logind's `CanHibernate` says `yes`, read once when the Extension starts. "Log Out" ends the session with logind's `TerminateSession`, whichever compositor runs it. The calls are not interactive: when polkit wants a password, as it does with another user logged in, the menu says it was turned down instead of waiting for a prompt. Without logind the Items are still listed, and running one says why it failed.

"Lock Screen" asks logind to lock the session, and logind tells the session's screen locker. Kotodama ships no locker; connect yours to logind's lock signal:

- **hypridle** runs your locker on it: `general { lock_cmd = pidof hyprlock || hyprlock }` in `hypridle.conf`.
- **swayidle** does too: `swayidle -w lock 'swaylock -f'`.
- **A Quickshell lock screen** (`WlSessionLock`) can lock when logind's session emits `Lock`, for example from a `Process` running `dbus-monitor --system "type='signal',interface='org.freedesktop.login1.Session',member='Lock'"`.

**Windows**: every toplevel the compositor reports is an Item of the shared kind `window`, titled by its title, with its app id beside and as its icon. The compositor pushes every window opened, retitled or closed, so a window's Item is sent again alone, and focus or maximizing sends nothing. Its primary Action, "Focus Window", focuses it; "Close Window", "Minimize", "Toggle Maximized" and "Toggle Fullscreen" follow. What no Wayland protocol offers, such as moving a window to a workspace or floating it, belongs to a compositor's own Extension, as the Hyprland Extension does it. On Hyprland the Extension leaves the windows to that Extension, so a window never shows twice (see `docs/adr/0025-the-generic-windows-step-aside-on-hyprland.md`). On a compositor without the protocol, such as GNOME or KDE, it lists no windows. Either way the windows part then stops, and the Extension uses no CPU.

Idle, with a few windows open, the Extension holds about 2.5 MiB resident and runs no thread but the ones blocked on the Daemon's pipe and the compositor's socket.

Its `manifest.toml` runs `./kotodama-system`, so a package installs the binary next to it in `$XDG_DATA_DIRS/kotodama/extensions/system/`.

## The Quickshell Front end

The `Kotodama` QML module is a Rust plugin (`crates/kotodama-qml`) that Quickshell imports. It connects to the Daemon, reconnects whenever the Daemon restarts, and gives QML two types:

- **`KotodamaMenu`**, the menu itself: a layer-shell window with a search box over the Results. Up and Down (or Ctrl+N and Ctrl+P) move the selection, and Enter runs the selected Result's primary Action. Esc follows one rule everywhere: it clears the text if there is any, else pops the View, else leaves the app, else closes the menu. Ctrl+K opens the Action panel: the Item's own Actions, then other Extensions' Contributed actions under each Extension's name. An Action's own key binding, such as Ctrl+L, runs it directly; Ctrl+Shift+Backspace runs Forget. After Forget or "Clear history", a Toast under the Results says what was forgotten, and Ctrl+Z undoes it while it shows; it never takes the keys or moves the selection. The Views Extensions push (lists, forms, details and progress) stack on top of the search, and Esc pops one at a time. Typing into a list filters it, matched as the search matches. Closing the menu keeps them: it opens again where it was. In a form, Tab moves between fields, Space ticks a checkbox, Left and Right choose an option, and Enter submits. Running jobs are listed under the search box. Tab, or Enter, on an app's entry Item enters the app: a chip names it in front of the search box, and its intro card shows over the rows until something is typed. Backspace on the empty box goes back to the whole search, with the app's word typed.
- **`KotodamaSearch`**, a list model of the Results, for a shell that draws its own window. Setting `query` searches; each row has the roles `title`, `subtitle`, `icon`, `extension` and `markup` (the title as StyledText, with its matched characters in `highlightColor`). While a list View is on top, the rows are its Items instead, and `filter(text)` filters them; the View's `filter` is what was typed. `run(row)` runs a Result. `actions(row)`, `runAction(row, n)` and `runShortcut(row, key, modifiers)` give the Action panel, and `hints(row)` the keys a row takes, such as "↵ Open · Tab Enter app · ⌃K Actions". `toast` is what the Toast says, empty when there is none; `isUndoKey(key, modifiers)` tells its key, and `undo()` undoes what it offers to. `view`, `depth` and `jobs` describe the top View and the running jobs, and `popView()`, `setText(field, text)`, `setChecked(field, checked)` and `submit()` work them. The signals `openRequested`, `toggleRequested` and `closeRequested` carry the `kotodama` command and the Daemon's requests. When it gets `closeRequested`, a window must hide and then call `closed()`, so the Daemon types or pastes into the window underneath and not into the menu. Whenever its window becomes visible, however it opened, a window calls `shown()`, so the Daemon ranks the Empty query again when the time of day has moved on.

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

The other look properties are `dimmed`, `selection`, `border`, `error`, `fontSize`, `radius`, `menuWidth`, `rowHeight`, `maxRows`, `top` and `placeholder`. `showHints: false` turns off the footer that names the keys the selected row takes, such as "↵ Open · Tab Enter app · ⌃K Actions"; a failure or an Extension that is down still shows there.

The home-manager module's `frontEnd.enable` runs such a config, the package's `share/kotodama/quickshell/shell.qml`, as a Quickshell of its own. To put the menu in your own Quickshell config instead, that Quickshell needs the module's directory as `QML_IMPORT_PATH`: `programs.kotodama.qmlImportPath` in home-manager, which home-manager's `programs.quickshell` service (`systemd.enable`) gets on its own. A Quickshell started some other way, such as `exec-once = quickshell` in Hyprland's config, needs it in its environment, for example `exec-once = env QML_IMPORT_PATH=${config.programs.kotodama.qmlImportPath} quickshell` written from Nix, or `$HOME/.nix-profile/lib/qt-6/qml` (`/etc/profiles/per-user/$USER/lib/qt-6/qml` with home-manager's `useUserPackages`), where the profile links the module. Set it on that Quickshell only: the module holds one Qt version's plugin, which nothing else in the session needs. That Quickshell must come from the same nixpkgs as Kotodama, or Kotodama built with its nixpkgs (see [Installing](#installing)).

A compositor keybind opens it through the `kotodama` command, which asks the Daemon to pass the request on to one menu: the one shown last, else the one that connected last. With no menu connected, the Daemon waits two seconds for one, as at login, when the keypress can come before the menu has started; if none comes, `kotodama` exits with 1 and says how to start one. `docs/adr/0029-kotodama-open-and-toggle-reach-one-menu-and-say-so.md` gives the reasons:

```
# Hyprland
bind = SUPER, Space, exec, kotodama toggle
bind = SUPER, V, exec, kotodama open --view clipboard/history
```

`kotodama open [--view <extension>/<item>] [--app <word>] [--query <text>]` opens it with text already typed. `--app emoji` opens it in the Emoji app, searching only the emoji (`--prefix` is its old name, still read). `--view` opens it on a View: it names an Item by its Extension and id, and the Daemon runs that Item's primary Action, such as "Show Clipboard History" on the clipboard's `history` Item, and opens the menu once the View it pushes is there, so the menu never shows the search first. Esc pops the View to the search. The Item may be hidden, so the search never shows it (ADR 0015). If the name matches no Item, or the Action fails, the menu opens on the search and says why. If the View hasn't arrived within a second (an Extension that is slow to start, say), the menu opens on the search, and the View still goes on top when it arrives. To try it, build the module and point Quickshell at it:

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
| `just check` | Checks formatting and runs the lints (rustfmt, clippy, the comment check, nixfmt, shellcheck, actionlint) |
| `just fmt` | Formats the crates brought in line with the [code-quality guide](docs/agents/code-quality.md), with the nightly rustfmt |
| `just comments` | Checks the guide's comment rules in those crates |
| `just test` | Runs the tests; extra arguments go to `cargo nextest run` |
| `just bench` | Runs the benchmarks; extra arguments go to `cargo bench` |
| `just e2e` | Runs the e2e tests against headless sway, or only those named (`just e2e menu`); `just e2e --update menu` rewrites a test's reference image |
| `just demo` | Runs Kotodama on a desktop of its own, a headless sway served over VNC on `localhost:5900`, to try it by hand; on macOS it opens Screen Sharing. See [Try it on macOS](docs/try-it.md#try-it-on-macos) |
| `just image` | Rebuilds the dev container image locally, e.g. after editing `docker/` without committing |

Nix only sees files that git tracks, so `git add` new files before running a recipe.

### e2e tests

`e2e/run` starts sway with the headless backend and the pixman renderer on one 640x480 output, then runs every `e2e/*/test.sh` against it. A test starts its clients with `spawn`, types with `wtype`, and checks the screen with `expect_screen reference.png`, which compares a grim screenshot with the stored image. Each test's screenshots, diff images and logs go in `e2e/out/<test>`, which CI uploads as an artifact.

Hyprland can't run here: it needs a DRM device. Its live tests will run in the NixOS VM test (`checks.<system>.nixos-vm`), which already installs Kotodama through the home-manager module and checks socket activation, the settings, and that apps started from the menu outlive the Daemon.

### CI

GitHub Actions runs `check` and `test`, and `e2e` as a separate job, on x86_64 and arm64. Each restores a cargo cache keyed on `Cargo.lock` and `flake.lock`, which only runs on `main` save. The `latency` e2e test builds the workspace in release, on top of the dependencies' release build in that cache (set `E2E_SKIP` to a space-separated list of tests to leave out of a full run). The NixOS VM job (x86_64) builds the Nix package, whose dependencies are a derivation of their own that Cachix serves, so a change rebuilds only the workspace's crates. The instructions job counts the instructions per keystroke of the change and of the base commit, on one runner, with `scripts/bench-instructions`. The bench job (x86_64 only) compares the benchmarks of the change with those of the base commit, and fails when one is more than 50% slower or the Daemon's resident memory is more than 10% higher. On `main`, it runs both commits on the same runner, one after the other, and saves the new commit's results in the Actions cache. On a pull request whose base has saved results, it runs only the change, with shorter criterion times (`--warm-up-time 1 --measurement-time 2`), and compares it with them. Shared runners differ in speed by up to 2x, so when a timing is over the limit against results from another runner, the job runs the base commit on its own runner and compares again; resident memory doesn't depend on the runner. Without saved results, both commits run on the same runner, as on `main`. The log says which way it went: lines starting with `bench:` name the path, the CPU the saved results came from, and this runner's CPU.

On pull requests, a `changes` job skips the jobs a change can't affect: a change only to Markdown, `docs/` or `LICENSE` runs none of them, and one only to `e2e/` or `examples/` runs only `e2e`. The bench and instructions jobs also skip changes that can't move a benchmark: crates no benchmark builds on (`scripts/bench-crates` lists those it does), the dev image, the NixOS tests and the release config. The `latency` test runs only for the crates the Daemon, the `kotodama` command and the QML module build on, and for the e2e harness. The NixOS VM job runs only for the crates, the lockfiles, the flake and `nix/`. Any other path runs everything. A `linear` job fails a pull request whose branch has a merge commit: rebase onto main instead of merging it. The `ci` job is the one required check. It passes when every job it needs succeeded or was skipped.

On pushes to `main`, CI publishes:

- the dev container image to **`ghcr.io/suplenc/kotodama-dev`** (linux/amd64 and linux/arm64), tagged `latest` and with git's hash of `docker/`, which is the tag `just` uses;
- Nix build outputs to the **`kotodama`** Cachix cache, using the `CACHIX_AUTH_TOKEN` repository secret.

CI and the dev container read from that cache. To use it with your own Nix, add it to `/etc/nix/nix.conf` (or run `cachix use kotodama`):

```
extra-substituters = https://kotodama.cachix.org
extra-trusted-public-keys = kotodama.cachix.org-1:8/a1e4X26O1hz2m5HFJBfKQ+P8m3GL7Z3Iy+xRIeZWE=
```

### Releases

release-please (`.github/workflows/release-please.yml`) reads the Conventional Commits that reach `main` and keeps a release PR open for each package they touch, chosen by the files a commit changes, not its scope. The packages are the core (`crates/kotodama` and `crates/kotodamad`, always released together at one version), `kotodama-protocol`, `kotodama-extension`, the Front end (`kotodama-qml`) and each Bundled extension. For now it only runs when started by hand (Actions → Release → Run workflow), until the setup below is done. A PR bumps its crate's version in its `Cargo.toml` and in `Cargo.lock` and adds to the crate's `CHANGELOG.md`. Merging it tags the release as `<package>-v<version>`, such as `kotodama-folders-v1.2.0` or `kotodama-v1.0.0`. Before 1.0, a breaking change bumps the minor version. Every package starts at 1.0.0 together: for that first run, add `"release-as": "1.0.0"` at the top of `release-please-config.json`, and take it out once the release PRs are merged. Separate releases are safe because a newer Daemon keeps talking to Extensions built for an older protocol (ADR 0026). `kotodama-protocol` and `kotodama-extension` are also published to crates.io once released. Their `major.minor` is the protocol version they speak (their `README.md`), so their release PRs only raise the patch version; a change to `PROTOCOL_VERSION` raises both crates' versions by hand and lands with a `Release-As: <major>.<minor>.0` footer. Publishing needs a `CARGO_REGISTRY_TOKEN` secret. Two things must be set up first: the repository setting that allows GitHub Actions to create pull requests, and a `RELEASE_PLEASE_TOKEN` secret so that CI runs on the release PR (a PR opened with the default token starts no workflows).
