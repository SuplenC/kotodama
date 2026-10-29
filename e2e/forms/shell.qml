// The default menu, tall enough for a form with every kind of field, with
// logs of its Views, the Daemon's errors and the paths offered to a file
// field for the test to wait on.
import QtQuick
import Quickshell
import Kotodama

ShellRoot {
    KotodamaMenu {
        id: menu
        menuWidth: 560
        top: 20
        maxRows: 15
        fontFamily: "DejaVu Sans"

        onFocusedChanged: console.log("focused: " + focused)
        onFailureChanged: console.log("failure: " + failure)

        function logTop() {
            // 0x100 is the title role.
            console.log("top: " + menu.search.data(menu.search.index(0, 0), 0x100));
        }

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onRowsInserted() {
                menu.logTop();
            }
            function onDataChanged() {
                menu.logTop();
            }
            function onViewChanged() {
                console.log("view: " + (menu.search.view.kind ?? "search") + " " + (menu.search.view.title ?? ""));
            }
            function onFieldErrorsChanged() {
                console.log("errors: " + menu.search.fieldErrors.filter(error => error !== "").join(", "));
            }
            function onCompletionsChanged() {
                console.log("completions: " + menu.search.completions.map(path => path.title).join(", "));
            }
        }
    }
}
