// The default menu, logging what it shows the moment it opens, so the test
// can tell a View that was there in the first frame from one pushed after.
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
        // The window maps, and so draws its first frame, only after this
        // handler returns: what it logs is what that frame shows.
        onVisibleChanged: {
            const view = menu.search.view;
            if (visible)
                console.log("opened on: " + (view.kind ?? "search") + " " + (view.title ?? ""));
            else
                console.log("visible: false");
        }
        onFailureChanged: {
            if (failure !== "")
                console.log("failure: " + failure);
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
