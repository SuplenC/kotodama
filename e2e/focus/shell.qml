// The default menu, logging whether it shows, has the keys, and what was
// typed into it, for the test to wait on.
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
        onVisibleChanged: console.log(visible ? "shown" : "hidden")

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onQueryChanged() {
                console.log("query: " + menu.search.query);
            }
        }
    }
}
