// The default menu, logging what the latency test waits on.
import QtQuick
import Quickshell
import Kotodama

ShellRoot {
    KotodamaMenu {
        id: menu
        onFocusedChanged: console.log("focused: " + focused)

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onLatencyChanged() {
                console.log("results: " + menu.search.rowCount());
            }
        }
    }
}
