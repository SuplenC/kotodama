// The default menu, with logs of its Action panel and Toast for the test to
// wait on.
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
        onActionsChanged: {
            if (menu.actions.length > 0)
                console.log("actions: " + menu.actions.map(action => (action.group !== "" ? action.group + "/" : "") + action.title).join(", "));
            else
                console.log("actions closed");
        }

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onRowsInserted() {
                console.log("rows: " + menu.search.rowCount());
            }
            function onToastChanged() {
                console.log("toast: " + (menu.search.toast !== "" ? menu.search.toast : "none"));
            }
            function onQueryChanged() {
                console.log("query: " + menu.search.query);
            }
        }
    }
}
