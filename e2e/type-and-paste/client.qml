// A window with a text field that logs whatever is typed into it.
import QtQuick
import Quickshell

ShellRoot {
    FloatingWindow {
        visible: true
        color: "white"

        TextInput {
            anchors.fill: parent
            focus: true
            onTextChanged: console.log("typed: " + text)
            Component.onCompleted: console.log("ready")
        }
    }
}
