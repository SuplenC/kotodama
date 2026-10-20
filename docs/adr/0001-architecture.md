# 1. Architecture: a warm daemon, a native Quickshell module, out-of-process Extensions

Status: accepted (2026-10-18)

## Context

Kotodama should beat Raycast, Spotlight and Omarchy's menu at finding and doing anything, with speed and memory use as the top priorities. It should be extensible, work with Quickshell by default, and run long jobs (say, a ten-minute install) without tying them to the menu window. It was designed alongside KubaOS (`SuplenC/kubaos`, ADR 0002), which is its first user but not a dependency. Terms are defined in `GLOSSARY.md`.

## Decision

**Rust, with a long-running Daemon.** `kotodamad` owns the index, the ranking, the History and the Extensions. It runs as a systemd user service started on first use of its socket, so it starts on the first keypress and stays warm after that. Front ends are thin and talk to it over a Unix socket.

**The Quickshell Front end is a Rust QML module (cxx-qt).** It speaks the fastest Wire format to the Daemon and exposes a native list model to QML, so results never pass through JavaScript. QML only draws them, which lets the menu take its look from the rest of the shell.

**Extensions run out of process.** An Extension is a directory with a `manifest.toml`, found in `$XDG_DATA_DIRS/kotodama/extensions` (installed by packages) and `~/.config/kotodama/extensions` (the user's own). The Daemon starts each one, keeps it running, and restarts it with backoff if it crashes; while it is down, its Results disappear and a small marker shows it. The generic sources (apps, calculator and units, emoji, clipboard history, web fallback, Folder sources, Hyprland windows and shortcuts, AI) are Bundled extensions using the same protocol, which keeps the protocol complete.

**The Wire format is negotiated.** At the handshake each side lists what it supports and they use the fastest they share: postcard (Rust only), then MessagePack, then JSON, which every Extension must support. All three go through serde, so one set of protocol types serves them all.

**Indexed items first, Live queries only when needed.** Decoding a message takes microseconds; a round trip to an Extension on every keystroke is what costs time. Extensions therefore push Indexed items ahead of time and update them when they change, and the Daemon searches them itself with no IPC while the user types. Only sources that can't be indexed (calculator, web, package search) get Live queries. Those are debounced, cancelled when the query changes, and streamed in as they arrive, so a slow Extension never holds back the rest.

**Ranking** uses nucleo (Helix's matcher) for fuzzy, typo-tolerant matching, plus Frecency, Learned choices, Aliases and Quicklinks.

**Interaction** is Raycast-style: Views pushed on a stack and popped with Esc, an Action panel per Result (Enter runs the primary Action, Ctrl+K lists all of them), and Arguments typed inline.

**Secrets** (an AI API key, for instance) are never stored in Kotodama's config. They come from a configured `*_command` whose output is the secret, so Kotodama never depends on a particular password manager.

## Considered options

- **Everything in QML/JS inside Quickshell.** Rejected: no warm index or History across sessions, jobs die with the window, and JS-side ranking and decoding are slow.
- **A process per query, as rofi scripts work.** Rejected: start-up cost on every keystroke.
- **In-process plugins (dynamic libraries, Lua, Rhai).** Rejected: one bad plugin freezes or crashes the menu, and it limits which languages Extensions can be written in.
- **JSON only.** Rejected: it is the slowest of the three. It is kept as the baseline so a shell-script Extension stays easy to write.
- **rkyv.** Rejected for now: it isn't serde-based, so it would need a second set of protocol types.
- **Kotodama draws its own window.** Rejected: on a Quickshell desktop the menu should look and behave like the rest of the shell. Other Front ends can still be added over the same socket.
