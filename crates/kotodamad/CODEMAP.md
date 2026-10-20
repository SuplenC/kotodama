# kotodamad code map

The Daemon. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`; tests are in `tests/`, benches in `benches/`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs:run`: wires paths, config, discovery, watchers, server and the main loop.
- `main.rs`: binary entry; calls `run`.
- `daemon.rs`: the state machine. `Daemon::handle(Event)` is the one entry point; `Event`, `Output`, `Launcher`/`Desktop` traits.
  - `Candidate` and `Candidate::rank`: the full Result order (Tier, then Learned choices/Frecency, then kind order; ADR 0006, 0011).
  - `Session`: per-Front-end state (query, shown Results, list View filters); `FromFrontEnd::Viewport` handling keeps visible rows stable (ADR 0003).
  - Views: `Daemon::filter_view` and `send_filtered` (a list View's filter, or `ToExtension::FilterView` when it filters itself), `view_contributed` (Contributed actions by Item kind), `front_end_up` re-sends the open Views to a Front end that connects. Hidden Items (ADR 0015) live in `Daemon::hidden`, outside the Index, for `--view` alone.
  - `Daemon::query`, `Daemon::empty_query`: searching, and what shows before typing. `Daemon::refresh` narrows a search to the last one's matches (`Narrowing`, forgotten on the empty query); `Daemon::add_near_misses` adds Near misses when real matches leave rows empty (ADR 0013); `Daemon::add_fallbacks` adds a Fallback's Live results (`[live_query] fallback`), last, after `learn`, and asks it the query, under the same condition (ADR 0016). `Daemon::picked` never makes a Fallback's Result a Learned choice.
  - Own Results under `OWN = "kotodama"` (ADR 0005): `FORGET`, `CLEAR_HISTORY`, `UNDO` (ADR 0010). Limits `MAX_RESULTS`, `MAX_LIVE_RESULTS`.
- `rank.rs`: match quality. `Tier`, `Query` (folding, `searches_subtitles`), `Letters` (character sets as bits), `Filter` (what lets a search skip Items before nucleo), `Haystack`, `fold_text`. `Tiers::near_miss`, `Tiers::near_miss_highlights` and `LETTERS_PER_TYPO`: typo tolerance (ADR 0013).
- `index.rs`: `Index`, every Indexed item in memory, searched with nucleo, with dense per-slot initials and every-letter `Letters` arrays (`starting_with`); `keyed` (keywords), `identified` (shared identities, ADR 0008).
- `history.rs`: `History` on disk (`$XDG_STATE_HOME/kotodama/history.json`); `Habit::frecency`, `History::learned` (Learned choices), `Settings::from_config`.
- `clock.rs`: `Clock`, `When` (time of day for Frecency, ADR 0009).
- `kind_order.rs`: `KindOrder::place`, configurable kind order for equal matches.
- `empty_query.rs`: `EmptyQuery::from_config`, the empty-query menu.
- `keywords.rs`: Aliases and Quicklinks from config (`Keywords`, `Quicklink::command`); URL templates and the opener are `kotodama_extension::url`.
- `open_view.rs`: `kotodama open --view` (ADR 0014): `ViewName` (`<extension>/<item>`), `Opening`. The Daemon side is `daemon.rs:Daemon::open_on_view` (runs the Item's primary Action, holds `Open`/`Toggle` back until the View is there, `OPEN_VIEW_TIMEOUT`).
- `views.rs`: `Views`, every View Extensions have open, the top last; `Views::filter` matches a list View's Items with the search's matcher and Tiers, narrowing as the user types on (`Narrowing`), highlights for the first `HIGHLIGHTED`.
- `arguments.rs`: text after a keyword filling an Action's Arguments, and the form for missing ones.
- `config.rs`: `load` (merge every `*.toml` in config dirs), `watch` (a `kotodama_extension::watch::Watcher` on thread `config`), `extension_table` (`[extensions.<name>]`).
- `discovery.rs`: `discover` (`<dir>/*/manifest.toml`), `watch` for Extension dirs (a `Watcher` on thread `extensions`); `Found`.
- `host.rs`: `Processes`, spawning Extension processes with reader/writer threads.
- `link.rs:pump`: post-handshake connection threads (Extension or Front end).
- `server.rs`: `listen`, `serve`, the Front end Unix socket.
- `desktop.rs`: paste/type worker over Wayland (virtual keyboard, `Keymap`, `Wayland`, `Worker`); data-control and finding the compositor are `kotodama_extension::wayland`; keys wait `SETTLE` after the menu closes (`settle_left`).
- `notify.rs:send`: desktop notifications over D-Bus.
- `paths.rs:Paths::from_env`: socket, config and Extension dirs from XDG.
- `usage.rs`: opt-in Usage log (`Writer`, `forget`).
