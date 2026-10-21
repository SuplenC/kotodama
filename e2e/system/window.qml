// A window titled by $E2E_TITLE, for the system Extension to find, focus and
// close.
import QtQuick
import Quickshell

ShellRoot {
    FloatingWindow {
        visible: true
        title: Quickshell.env("E2E_TITLE")
        color: "white"
    }
}
