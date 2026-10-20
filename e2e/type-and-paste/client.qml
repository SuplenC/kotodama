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
            // Keys reach it from now on: its window is mapped and has focus.
            onActiveFocusChanged: {
                if (activeFocus)
                    console.log("ready");
            }
        }
    }
}
