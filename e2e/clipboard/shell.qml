// The default menu, with logs of its Views and Action panel for the test to
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
        onVisibleChanged: console.log("visible: " + visible)
        onActionsChanged: {
            if (menu.actions.length > 0)
                console.log("actions: " + menu.actions.map(action => action.title).join(", "));
        }

        Connections {
            target: menu.search
            function onConnectedChanged() {
                console.log("connected: " + menu.search.connected);
            }
            function onViewChanged() {
                const view = menu.search.view;
                console.log("view: " + (view.kind ?? "search") + " " + (view.title ?? ""));
            }
        }
    }
}
