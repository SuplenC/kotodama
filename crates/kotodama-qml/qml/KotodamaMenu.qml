// The menu: a layer-shell window with a search box over the Daemon's Results.
// Its look comes from the properties below, so a host shell can bind them to
// its own theme; it can also draw its own window around a KotodamaSearch.
import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Widgets
import Kotodama

PanelWindow {
    id: menu

    // The look.
    property color background: "#1e1e2e"
    property color foreground: "#cdd6f4"
    property color dimmed: "#a6adc8"
    property color accent: "#89b4fa"
    property color selection: "#313244"
    property color border: "#45475a"
    property string fontFamily: "sans-serif"
    property int fontSize: 15
    property real radius: 12
    property int menuWidth: 640
    property int rowHeight: 44
    property int maxRows: 8
    property real top: screen ? screen.height / 5 : 120
    property string placeholder: "Search"

    readonly property KotodamaSearch search: search
    // Whether keys reach the search box: the compositor has given the window
    // keyboard focus.
    readonly property bool focused: input.activeFocus
    // Why the last Action failed, until the next Action or opening.
    property string failure: ""

    function show(query) {
        menu.failure = "";
        input.text = query;
        input.cursorPosition = query.length;
        visible = true;
        input.forceActiveFocus();
    }

    function hide() {
        visible = false;
    }

    visible: false
    // Anchored at the top only, so the compositor centres it horizontally.
    anchors.top: true
    margins.top: top
    // The surface keeps one size, so typing never waits on a resize; the
    // panel inside grows with the Results.
    implicitWidth: menuWidth
    implicitHeight: field.height + maxRows * rowHeight + footer.height + 16
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "kotodama"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    // The Daemon types or pastes once the window is gone and focus is back
    // where the user came from, so it is told only after the window unmaps.
    property bool closing: false
    onVisibleChanged: {
        if (!visible && closing) {
            closing = false;
            Qt.callLater(search.closed);
        }
    }

    KotodamaSearch {
        id: search
        highlightColor: menu.accent
        onOpenRequested: (query, view) => menu.show(query)
        onToggleRequested: menu.visible ? menu.hide() : menu.show("")
        onCloseRequested: {
            if (menu.visible) {
                menu.closing = true;
                menu.hide();
            } else {
                search.closed();
            }
        }
        onActionFailed: message => menu.failure = message
    }

    Rectangle {
        id: panel
        width: parent.width
        height: field.height + list.height + footer.height
        color: menu.background
        radius: menu.radius
        border.color: menu.border
        border.width: 1

        Item {
            id: field
            width: parent.width
            height: menu.rowHeight + 12

            TextInput {
                id: input
                anchors.fill: parent
                anchors.leftMargin: 18
                anchors.rightMargin: 18
                verticalAlignment: TextInput.AlignVCenter
                color: menu.foreground
                selectionColor: menu.selection
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize + 3
                focus: true
                onTextChanged: {
                    search.query = text;
                    list.currentIndex = 0;
                    list.positionViewAtBeginning();
                }

                Text {
                    anchors.fill: parent
                    verticalAlignment: Text.AlignVCenter
                    visible: input.text === ""
                    text: menu.placeholder
                    color: menu.dimmed
                    font: input.font
                }

                Keys.onPressed: event => {
                    const ctrl = event.modifiers & Qt.ControlModifier;
                    if (event.key === Qt.Key_Escape) {
                        menu.hide();
                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        menu.failure = "";
                        search.run(list.currentIndex);
                    } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N)) {
                        list.incrementCurrentIndex();
                    } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P)) {
                        list.decrementCurrentIndex();
                    } else if (event.key === Qt.Key_PageDown) {
                        list.currentIndex = Math.min(list.count - 1, list.currentIndex + menu.maxRows);
                    } else if (event.key === Qt.Key_PageUp) {
                        list.currentIndex = Math.max(0, list.currentIndex - menu.maxRows);
                    } else {
                        return;
                    }
                    event.accepted = true;
                }
            }
        }

        ListView {
            id: list
            y: field.height
            width: parent.width
            height: Math.min(count, menu.maxRows) * menu.rowHeight
            clip: true
            model: search
            boundsBehavior: Flickable.StopAtBounds
            highlightMoveDuration: 0
            onCountChanged: {
                if (currentIndex < 0 && count > 0)
                    currentIndex = 0;
            }

            // Late Results go below the rows on screen (ADR 0003), so the
            // Daemon is told which those are.
            readonly property int firstRow: Math.floor(contentY / menu.rowHeight)
            onFirstRowChanged: search.viewport(firstRow, menu.maxRows)

            delegate: Item {
                id: row

                required property int index
                required property string markup
                required property string subtitle
                required property string icon

                width: ListView.view.width
                height: menu.rowHeight

                Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: 6
                    anchors.rightMargin: 6
                    radius: menu.radius / 2
                    color: row.ListView.isCurrentItem ? menu.selection : "transparent"
                }

                IconImage {
                    id: image
                    x: 16
                    anchors.verticalCenter: parent.verticalCenter
                    implicitSize: menu.rowHeight - 16
                    visible: row.icon !== ""
                    source: row.icon.startsWith("/") ? "file://" + row.icon : Quickshell.iconPath(row.icon, true)
                }

                Column {
                    anchors.left: parent.left
                    anchors.leftMargin: 16 + menu.rowHeight - 16 + 12
                    anchors.right: parent.right
                    anchors.rightMargin: 16
                    anchors.verticalCenter: parent.verticalCenter

                    Text {
                        width: parent.width
                        text: row.markup
                        textFormat: Text.StyledText
                        elide: Text.ElideRight
                        color: menu.foreground
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize
                    }

                    Text {
                        width: parent.width
                        visible: row.subtitle !== ""
                        text: row.subtitle
                        elide: Text.ElideRight
                        color: menu.dimmed
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize - 3
                    }
                }

                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    onEntered: list.currentIndex = row.index
                    onClicked: search.run(row.index)
                }
            }
        }

        Text {
            id: footer
            anchors.top: list.bottom
            width: parent.width
            leftPadding: 18
            rightPadding: 18
            height: text === "" ? 0 : menu.rowHeight * 0.7
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            color: menu.failure !== "" ? "#f38ba8" : menu.dimmed
            font.family: menu.fontFamily
            font.pixelSize: menu.fontSize - 3
            text: menu.failure !== "" ? menu.failure : search.down.length > 0 ? "Unavailable: " + search.down.join(", ") : ""
        }
    }
}
