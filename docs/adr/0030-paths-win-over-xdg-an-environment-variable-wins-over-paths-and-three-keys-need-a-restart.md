# 30. `[paths]` wins over XDG, an environment variable wins over `[paths]`, and three keys need a restart

Status: accepted (2026-10-21)

The Daemon's directories and its socket came from the XDG variables alone. The config can now set them in `[paths]`, and the config itself can come from another directory. Each value has more than one possible source, so the order between them needs to be fixed. Most settings reload with the config, but a few of these can't, and that needs to be written down too.

## What it does

- **Where the config is.** `kotodamad --config <dir>` wins, then `KOTODAMA_CONFIG_DIR`, then `$XDG_CONFIG_DIRS/kotodama` followed by `$XDG_CONFIG_HOME/kotodama`. A directory named either way is the whole config: the XDG directories aren't read beside it.
- **The socket.** `KOTODAMA_SOCKET` wins, then `[paths] socket`, then `$XDG_RUNTIME_DIR/kotodama.sock`. Front ends and the `kotodama` command read `KOTODAMA_SOCKET` and the XDG default, never the Daemon's config.
- **The state and cache directories.** `[paths] state_dir` and `cache_dir` win over `$XDG_STATE_HOME/kotodama` and `$XDG_CACHE_HOME/kotodama`.
- **Extension directories.** The default ones, `$XDG_DATA_DIRS/kotodama/extensions` and `extensions/` in the last config directory, unless `default_extension_dirs = false`, then `extension_dirs` in order. An Extension in a later directory replaces one of the same name before it.
- **Paths are absolute or start with `~/`.** A relative path would depend on the Daemon's working directory, which systemd sets and the user never sees.
- **Read at start.** `state_dir`, `cache_dir` and `socket` are read once, when the Daemon starts. A change to one while it runs is logged as needing a restart, and the old value stays. `extension_dirs` and `default_extension_dirs` reload: the watcher moves to the new directories and the Extensions are found again.
- **The restart keys.** `[extensions] retry_max_seconds`, the longest wait before a crashed Extension restarts, 30 by default, and `give_up_after`, the crashes in a row after which it stays down until its program or Manifest changes, 0 by default, which never gives up. Both reload.

## Why this order

A command-line flag and an environment variable are set for one run or one session. The config is set for good. The narrower setting wins, as it does for `KOTODAMA_LOG` over `[log] level`. `[paths]` wins over XDG because XDG gives every program the same default, and `[paths]` is a choice made for Kotodama alone.

The socket is the exception that shapes the rest. A Front end has to find the socket before it can talk to the Daemon, so it can't learn it from the Daemon's config. If `[paths] socket` were the only way to move the socket, every Front end would have to parse the Daemon's config too. `KOTODAMA_SOCKET` is the one variable both sides read. It wins on the Daemon's side as well, so the two can never disagree when it is set.

## Why three keys need a restart

The History and the Usage log are open files under `state_dir`. Moving them while the Daemon runs means writing the History out, opening it again elsewhere, and deciding what happens to the file left behind. The cache is the same. The socket is the address Front ends are already connected to, or that systemd's socket unit holds. Rebinding it drops every Front end, and under socket activation the Daemon doesn't own it at all. None of these changes often. A restart is cheap and does it cleanly.

## Why the Daemon gives up by count

A crashed Extension restarts after a wait that doubles from 100 ms. Without a cap, a few crashes would make the wait unbounded. Without the option to give up, an Extension that crashes at start keeps costing a process start every 30 seconds for as long as the Daemon runs. Counting crashes in a row, rather than in a time window, needs no clock state. It also clears as soon as the Extension stays up once. Giving up puts the Extension on the problems list. It is retried when its program or Manifest changes, the same way as an Extension that can't start.

## Cost

Nothing on the keystroke path. The paths are resolved once at start and on a config reload. The restart settings are read when an Extension goes down.

## Considered options

- **`[paths]` below the XDG variables.** Rejected: XDG variables are often set by the session for every program, so the config could never move Kotodama's directories.
- **The socket from the config only.** Rejected: Front ends would need to read the Daemon's config to connect.
- **Reload `state_dir` and `cache_dir` live.** Rejected for now: it needs a move or a reopen of the History. The work isn't worth it for a setting that changes about once.
- **Give up after N crashes in a time window.** Rejected: it adds a clock and a window to tune. Crashes in a row with a capped backoff already bound the cost.
