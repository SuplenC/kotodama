// The default menu, logging what the test waits on: keyboard focus, the
// connection, how many problems the footer counts, and the View on top.
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
            function onProblemsChanged() {
                console.log("problems: " + menu.search.problems);
            }
            function onViewChanged() {
                console.log("view: " + (menu.search.view.title ?? "none"));
            }
        }
    }
}
