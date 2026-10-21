// The default menu, logging the Extension its search is behind and which
// Extension owns each row, so the test can tell a search behind a Prefix
// from the whole search.
import QtQuick
import Quickshell
import Kotodama

ShellRoot {
    KotodamaMenu {
        id: menu
        menuWidth: 560
        top: 40
        maxRows: 6
        fontFamily: "DejaVu Sans"

        onFocusedChanged: console.log("focused: " + focused)

        // The rows' Extensions, once a batch of changes is in, as
        // `rows: [apps, apps]`.
        function logRows() {
            const owners = [];
            for (let row = 0; row < menu.search.rowCount(); row++)
                // 0x104 is the Extension role.
                owners.push(menu.search.data(menu.search.index(row, 0), 0x104));
            console.log("rows: [" + owners.join(", ") + "]");
            // 0x100 is the title role.
            console.log("top: " + menu.search.data(menu.search.index(0, 0), 0x100));
        }

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onScopeChanged() {
                console.log("scope: [" + menu.search.scope + "]");
            }
            function onQueryChanged() {
                console.log("query: [" + menu.search.query + "]");
            }
            function onRowsInserted() {
                Qt.callLater(menu.logRows);
            }
            function onRowsRemoved() {
                Qt.callLater(menu.logRows);
            }
            function onDataChanged() {
                Qt.callLater(menu.logRows);
            }
        }
    }
}
