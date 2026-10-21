# 28. One keyword table with a fixed precedence

Status: accepted (2026-10-21)

Three things claim a word typed first in the search: an Alias (ADR 0012), a Quicklink, and an app's word (ADR 0027). Before this, each was handled on its own. Two apps asking for one word were settled by the order Extensions were found in, which follows the directory listing. An app whose word and name were both taken had no app, so its app-only Items could not be reached. A Quicklink `calc` and the Calculator's word `calc` both matched as keywords, and nothing said which one the user meant. None of these conflicts was reported.

## What it does

The Daemon builds one keyword table whenever the config or the installed Extensions change. Each keyword has one holder, in this order:

1. an Alias;
2. a Quicklink;
3. an app's word set by the user in `[extensions.<name>] word`;
4. an app's word set by its Manifest;
5. an app's Extension name, used when no word is set.

Between two apps on the same step, the Extension whose name sorts first wins. The result never depends on the order Extensions were found in.

An app that loses its word takes its Extension's name. If that name is held too, it takes the name followed by `-2`, `-3` and so on, until one is free. Every Extension therefore has an app, and its Items can always be reached.

Every lost keyword is a `Conflict`. A Quicklink under an Alias's keyword is one too: the Alias's Item goes first, and the Quicklink still matches below it. `conflict::report` is the one place conflicts reach the user. For now it logs each one as a warning when the set changes. The problems list will read them from the same place.

## Why this order

The user wrote Aliases and Quicklinks down by hand and by name, so these are the most deliberate choices. A word set in the config is the user overriding an Extension. A Manifest's word is the author's guess, made without knowing the other Extensions. The name is only a default. Sorting by name makes the outcome the same on every machine with the same Extensions.

## Cost

Everything here runs when the config or the Extensions change, never on a keystroke. The table is built from the Aliases' map and a scan of the Quicklinks, once per app.

## Considered options

- **Keep the first app found.** Rejected: directory order differs between machines and file systems, so two users with the same setup could get different words.
- **Drop an app that cannot get its word or name.** Rejected: its app-only Items, such as Hyprland's binds, would be unreachable, and nothing would say why.
- **Let the app's word win over a Quicklink.** Rejected: the user wrote the Quicklink; the word came with an Extension they may not have chosen.
