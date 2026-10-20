// The default menu as a host shell would load it, with a font every machine
// has, and logs the test waits on.
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
            // The title in the top row, whenever the rows change.
            function onRowsInserted() {
                menu.logTop();
            }
            function onDataChanged() {
                menu.logTop();
            }
        }

        function logTop() {
            // 0x100 is the title role.
            console.log("top: " + menu.search.data(menu.search.index(0, 0), 0x100));
        }
    }
}
