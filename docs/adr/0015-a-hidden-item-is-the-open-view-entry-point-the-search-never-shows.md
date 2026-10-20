# 15. A hidden Item is the `--view` entry point the search never shows

Status: accepted (2026-10-20)

`kotodama open --view <extension>/<item>` runs an Indexed item's primary Action (ADR 0014). An Extension with only Live queries has no Indexed items, so `--view` can't reach its Views. Neither can an Extension that wants a View behind a keybind but doesn't want it among the search's Results.

An Item can now be `hidden`. A hidden Item is sent in `add_items` like any other, but the Daemon keeps it out of the Index, so no query, Alias or Frecency ever shows it. `--view` looks it up after the Index, and runs its primary Action as it would any Item's. `remove_items` drops it, and so does the Extension stopping. An Extension with only Live queries adds one hidden Item for each View it wants reachable. `hidden` is ignored outside `add_items`: a Live query's Results and a list View's Items always show.

This changes the Item layout, so it waits for the protocol version that list View filtering needs anyway (ADR 0012).

## Considered options

- **A Manifest section declaring named Views, and a new `ToExtension::OpenView` message.** Rejected again (see ADR 0014): it is a second way to reach the same View, and needs a message every Extension must answer.
- **Send the `--view` name to the Extension as a Live query.** Rejected: a Live query's Results are rows to show, not a View to push, and the Extension would need to tell a name from something typed.
- **Let `--view` name an Item from a Live query's last Results.** Rejected: there are none until something is typed, which is never the case for a keybind.
