// The default menu, logging what the test waits on: keyboard focus, the
// connection, the rows' titles once they have changed, and where the pointer
// moves over the menu.
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

        // On the window's content, under every row: Qt hands a hover event to
        // the items under the pointer before their parents, so the rows have
        // had a move once it is logged. A move while the menu goes away has
        // the handler no longer hovered and isn't logged.
        HoverHandler {
            onPointChanged: {
                if (hovered)
                    console.log("pointer: " + Math.round(point.position.x) + " " + Math.round(point.position.y));
            }
        }

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onRowsInserted() {
                Qt.callLater(menu.logRows);
            }
            function onRowsRemoved() {
                Qt.callLater(menu.logRows);
            }
            function onRowsMoved() {
                Qt.callLater(menu.logRows);
            }
            function onDataChanged() {
                Qt.callLater(menu.logRows);
            }
            function onModelReset() {
                Qt.callLater(menu.logRows);
            }
        }

        // Called later, once for all the signals of one change, so a logged
        // line is never a step in the middle of it, with the right rows in
        // the wrong order.
        function logRows() {
            const titles = [];
            // 0x100 is the title role.
            for (let i = 0; i < menu.search.rowCount(); i++)
                titles.push(menu.search.data(menu.search.index(i, 0), 0x100));
            console.log("rows: " + titles.join(" "));
        }
    }
}
