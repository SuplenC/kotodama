// The default menu, logging what the test waits on: keyboard focus, the
// connection, the rows' titles, the query and what the footer asks.
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
        onVisibleChanged: console.log("visible: " + visible)

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onRowsInserted() {
                menu.logRows();
            }
            function onDataChanged() {
                menu.logRows();
            }
            function onQueryChanged() {
                console.log("query: " + menu.search.query);
            }
            function onConfirmingChanged() {
                console.log("confirming: " + (menu.search.confirming !== "" ? menu.search.confirming : "none"));
            }
        }

        function logRows() {
            const titles = [];
            // 0x100 is the title role.
            for (let i = 0; i < menu.search.rowCount(); i++)
                titles.push(menu.search.data(menu.search.index(i, 0), 0x100));
            console.log("rows: " + titles.join(", "));
        }
    }
}
