# 7. The `kotodama` command goes through the Daemon

Status: accepted (2026-10-19)

A compositor keybind runs `kotodama toggle` or `kotodama open`, and something has to tell the menu. The command connects to the Daemon's socket like a Front end and sends `Open` or `Toggle`; the Daemon passes it on to every other connected Front end, and each one decides what showing or hiding means for its own window. Under socket activation, the first keypress also starts the Daemon.

## Considered options

- **Quickshell's IPC (`qs ipc call`).** Rejected: it only reaches the Quickshell Front end, so another Front end would need its own command, and the Daemon would stay cold until the menu first asked it for something.
- **The Daemon decides whether to open or close.** Rejected: only the Front end knows whether its window is showing, so `Toggle` is passed on and the Front end decides.
