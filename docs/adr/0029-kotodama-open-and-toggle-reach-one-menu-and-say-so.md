# 29. `kotodama open` and `toggle` reach one menu, and say so

Status: accepted (2026-10-21)

A user enabled `programs.kotodama`, bound SUPER+Space to `kotodama toggle`, and nothing happened. The command exited 0. `frontEnd.enable` was off by default, and the command only sent its message: the Daemon passed it on to every connected Front end, of which there were none, and answered nothing. The other way round, a Quickshell `Variants` over the screens, or the `kotodama-menu` service beside a shell of the user's own that places `KotodamaMenu` too, opened several menus at once. Each took the keyboard. A request sent at login, before the menu had connected, was lost.

## What it does

- **One menu.** `FromFrontEnd::Open` and `Toggle` go to one menu. A menu is a Front end that never asked for a menu itself, and so isn't the `kotodama` command. The one chosen is the menu last shown (`FromFrontEnd::Shown`), or, before any was shown, the one that connected last. The View an `open --view` waits for, and a failure to open it, go to that menu too.
- **An answer.** The Daemon answers the Front end that asked with `ToFrontEnd::Reached { menus }`: 1, or 0 when there is no menu. `kotodama` reads it. With 0 it exits with 1 and says `no menu is running`, and how to start one.
- **A short wait at login.** With no menu connected, the Daemon holds the request for `MENU_WAIT`, two seconds. The first Front end to search in that time gets it. A Front end that has only connected may be the command itself, before its message arrives; one that searched is a menu. Then the Daemon answers 1. After two seconds it answers 0. The command waits for the answer within its read timeout of five seconds.
- **`frontEnd.enable` is on by default**, unless home-manager runs a Quickshell of the user's own (`programs.quickshell.enable`). A user with such a shell is the one who places `KotodamaMenu` in it, and a second Quickshell would stay resident for a menu that never opens. Everyone else gets a menu with no further setup.

## Why one menu, and which

Two menus opening at once is never what the user wants: both take the keyboard and one keypress lands in either. The menu on the focused screen would be best, but on Wayland the focused output is something only the compositor knows, through its own IPC. The Daemon knows no compositor. The menu last shown is the one the user last used, so a second toggle closes what the first opened. Without one, the newest connection is the menu most likely to be current, as after a restart of the shell.

## The protocol

`ToFrontEnd::Reached` is a new message. A menu never sends `Open` or `Toggle`, so it never receives one. The Quickshell Front end skips it anyway. Protocol 1.0 is unreleased (ADR 0026), so 1.0 is amended and its layout pinned again.

## Considered options

- **Keep passing it on to every Front end, and count them.** Rejected: it keeps the several-menus problem, and the count would include other running `kotodama` commands.
- **The menu on the focused output.** Rejected for now: it needs each compositor's IPC, or Front ends that report which output has the focus.
- **`frontEnd.enable` on whatever else runs.** Rejected: beside the user's own Quickshell it only adds a second resident Quickshell, which home-manager can see and avoid.
- **Fail at once with no menu.** Rejected: at login, the keybind often comes before Quickshell has connected, and that request should open the menu, not fail.
