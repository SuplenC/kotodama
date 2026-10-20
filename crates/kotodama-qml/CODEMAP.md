# kotodama-qml code map

The Quickshell Front end. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `qml/KotodamaMenu.qml`: the menu UI; theme properties at the top (colors, font, sizes, `maxRows`). `key(event)` is every View's keys (the Toast's undo key first; `ctrlOnly` for the menu's own Ctrl keys); the Toast sits under the footer.
- `src/lib.rs`: `SearchRust`, the QObject the QML talks to (cxx-qt bridge). The Toast: `toast` (what it says), `offered` (the change it undoes), `undo`, `isUndoKey` (`UNDO_SHORTCUT`), `set_toast`. `QtModel` is `rows::Model` over Qt's begin/end signals; `show_rows` diffs a list View's rows and emits `selectRow` so its selected Item stays selected (`select` tells it which, and sends `SelectionMoved` once per query). `frameStarted` (on `afterAnimating`) and `painted` (on `frameSwapped`) record the Results version on screen, which `run_action` confirms as `shown`.
- `src/connection.rs`: the Daemon connection thread (`spawn`, `Update`, `Writer`); messages that arrived together reach the Qt thread in one batch, shown once (`lib.rs:apply`).
- `src/rows.rs`: `Row` (`Row::listed` for a list View's Items, `Row::relist` to update one in place), `Rows` (the model's rows, a gap buffer), `Model`; `replace` (by position, for the Results) and `diff` (by Item key, for list Views: removes, moves, inserts and changes only what differs; `Fresh` gives it the new rows), Action panel entries (`PanelEntry::shortcut` drops a contributed shortcut an own Action binds).
- `src/views.rs`: `Views` stack, form values, jobs; a list View's filter, the Daemon's matches (`Views::set_filter`, `Views::filtered`, `Open::listed` making rows only as `rows::diff` needs them) and Contributed actions by kind.
- `src/variant.rs`: Views, jobs and actions as QVariant maps for QML.
- `src/shortcut.rs:matches`: Action key bindings like `ctrl+o`.
- `src/plugin.rs`: Qt plugin entry symbols. `build.rs`: cxx-qt build.
- `benches/filter.rs`: a filter reply on a 1000-Item list View, Qt-free (it compiles `rows.rs` and `views.rs` itself, since the cdylib can't be linked).
