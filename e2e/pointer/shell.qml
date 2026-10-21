// The default menu, logging what the test waits on: keyboard focus, the
// connection, and the rows' titles whenever they change.
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

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onRowsInserted() {
                menu.logRows();
            }
            function onRowsRemoved() {
                menu.logRows();
            }
            function onRowsMoved() {
                menu.logRows();
            }
            function onDataChanged() {
                menu.logRows();
            }
            function onModelReset() {
                menu.logRows();
            }
        }

        function logRows() {
            const titles = [];
            // 0x100 is the title role.
            for (let i = 0; i < menu.search.rowCount(); i++)
                titles.push(menu.search.data(menu.search.index(i, 0), 0x100));
            console.log("rows: " + titles.join(" "));
        }
    }
}
