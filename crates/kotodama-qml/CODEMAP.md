# kotodama-qml code map

The Quickshell Front end. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `qml/KotodamaMenu.qml`: the menu UI; theme properties at the top (colors, font, sizes, `maxRows`).
- `src/lib.rs`: `SearchRust`, the QObject the QML talks to (cxx-qt bridge).
- `src/connection.rs`: the Daemon connection thread (`spawn`, `Update`, `Writer`).
- `src/rows.rs`: `Row`, `Replace` (list model diffing without reset), Action panel entries.
- `src/views.rs`: `Views` stack, form values, jobs.
- `src/variant.rs`: Views, jobs and actions as QVariant maps for QML.
- `src/shortcut.rs:matches`: Action key bindings like `ctrl+o`.
- `src/plugin.rs`: Qt plugin entry symbols. `build.rs`: cxx-qt build.
