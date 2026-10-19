// A trivial layer-shell window with flat colors only, so software rendering
// gives the same pixels on every machine.
import QtQuick
import Quickshell
import Quickshell.Wayland

ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        margins.top: 80
        margins.left: 120
        implicitWidth: 400
        implicitHeight: 240
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "kotodama-e2e"
        color: "#1e1e2e"

        Rectangle {
            x: 20
            y: 20
            width: 360
            height: 40
            color: "#89b4fa"
        }

        Rectangle {
            x: 20
            y: 80
            width: 160
            height: 140
            color: "#f38ba8"
        }
    }
}
