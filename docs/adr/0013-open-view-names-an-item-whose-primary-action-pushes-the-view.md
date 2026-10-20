# 13. `kotodama open --view` names an Item whose primary Action pushes the View

Status: accepted (2026-10-20)

A keybind should be able to open the menu straight on a View, as SUPER+V opens clipboard history, and the menu's first frame must show that View, not the search with the View pushed over it a moment later. Views only exist once an Extension pushes one, so `--view` can't name a View by its id. It names an Item instead, as `<extension>/<item id>` (`clipboard/history`), and the Daemon runs that Item's primary Action, the one Enter would run. An Extension that wants a View reachable from a keybind exposes it as an Item whose primary Action pushes it, which it usually has already, so the search finds it too. Extension names have no `/`, so the name splits at the first one and an Item id may contain more.

The Daemon holds the request back from the menus until the Action is done. The View it pushed reaches each menu before the `Open`, so the menu puts it on top while hidden and opens on it. If the name matches no Item, the Action needs an Argument, fails or its Extension stops, the menus open on the search with the reason. An Action that closes the menu, types or pastes does so, and the menu stays closed. The run isn't a pick from a search, so Frecency and the Usage log don't record it.

Under socket activation, the keypress may also start the Daemon. Until the named Extension is up and has indexed something, the request waits, so the first keypress after login still opens on the View. It waits at most a second in all, then the menus open on the search; a View that arrives later is pushed over it like any other.

The protocol is unchanged: `Open`'s `view` was already an optional string.

## Considered options

- **The Front end runs the Action and waits for the View.** Rejected: a Front end doesn't know an Item's Actions until it searches, and with several Front ends each would run it.
- **A Manifest section declaring named Views, and a new `ToExtension::OpenView` message.** Rejected for now: it means a protocol version, a second way to reach the same View, and work for every Extension, when an Item with a primary Action already says the same thing.
- **Open at once, and push the View when it comes.** Rejected: the search shows first, which is what the issue rules out.
- **Wait for the View with no time limit.** Rejected: a hung Extension would make the keybind do nothing.
