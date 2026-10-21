// The demo's Quickshell config: the menu, as the ready Front end runs it, over
// a desktop background that shows what to try. The background is a layer-shell
// window on the Background layer, drawn in QML so its text is as crisp as the
// menu's, and laid out around the menu's own geometry so the menu never covers
// it. Only `just demo` (scripts/demo) runs it.
import QtQuick
import Quickshell
import Quickshell.Wayland
import Kotodama

ShellRoot {
    KotodamaMenu {
        id: menu
    }

    PanelWindow {
        id: desk

        // The menu's palette, with a darker desk under it.
        readonly property color base: "#11111b"
        readonly property color card: "#181825"
        readonly property color line: "#313244"
        readonly property color text: menu.foreground
        readonly property color soft: menu.dimmed
        readonly property color faint: "#7f849c"
        readonly property color chip: "#1e1e2e"

        // Sizes follow the output: 1 at 1920x1080, smaller below it.
        readonly property real k: Math.max(0.75, Math.min(1.25, Math.min(width / 1920, height / 1080)))
        readonly property int font: Math.round(16 * k)
        readonly property int gap: Math.round(24 * k)

        // Where the menu's surface is, so nothing is drawn under it.
        readonly property real menuLeft: (width - menu.menuWidth) / 2
        readonly property real menuRight: menuLeft + menu.menuWidth
        readonly property real menuTop: menu.top
        readonly property real menuBottom: menu.top + menu.implicitHeight
        readonly property real side: menuLeft - 2 * gap
        // Both side columns start at one height, centred on the taller.
        readonly property real columnsTop: Math.max(gap, (height - Math.max(leftColumn.height, rightColumn.height)) / 2)

        anchors {
            top: true
            bottom: true
            left: true
            right: true
        }
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Background
        WlrLayershell.namespace: "kotodama-demo-background"
        color: base

        // A faint glow where the menu opens.
        Rectangle {
            x: desk.menuLeft - desk.gap / 2
            y: desk.menuTop - desk.gap / 2
            width: menu.menuWidth + desk.gap
            height: desk.menuBottom - desk.menuTop + desk.gap
            radius: menu.radius + desk.gap / 2
            color: "#16161f"
            border.color: "#1c1c29"
        }

        // While the menu is closed: how to open it.
        Column {
            visible: !menu.visible
            anchors.horizontalCenter: parent.horizontalCenter
            y: desk.menuTop + (desk.menuBottom - desk.menuTop) / 2 - height / 2
            spacing: desk.gap / 2
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: desk.gap / 3
                KeyCaps {
                    keys: "Alt+Space"
                    big: true
                }
                Note {
                    text: "or"
                    anchors.verticalCenter: parent.verticalCenter
                }
                KeyCaps {
                    keys: "Ctrl+Space"
                    big: true
                }
            }
            Note {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "opens the menu here"
            }
        }

        // The title, above the menu.
        Column {
            anchors.horizontalCenter: parent.horizontalCenter
            width: menu.menuWidth + 2 * desk.gap
            y: Math.max(desk.gap / 2, (desk.menuTop - desk.gap - height) / 2)
            spacing: desk.gap / 3
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Kotodama"
                color: desk.text
                font.family: menu.fontFamily
                font.pixelSize: Math.round(40 * desk.k)
                font.weight: Font.DemiBold
            }
            Note {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "Type, and it finds and does things. Every chip on this desktop is a query to try."
            }
        }

        // Below the menu: the keys that work everywhere, and the guide.
        Column {
            anchors.horizontalCenter: parent.horizontalCenter
            y: desk.menuBottom + desk.gap * 1.5
            spacing: desk.gap * 0.8
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: desk.gap * 0.6
                Hint {
                    keys: "Alt+Space"
                    what: "menu"
                }
                Hint {
                    keys: "Alt+V"
                    what: "clipboard"
                }
                Hint {
                    keys: "Alt+B"
                    what: "bookmarks"
                }
                Hint {
                    keys: "Alt+Return"
                    what: "terminal"
                }
            }
            Column {
                anchors.horizontalCenter: parent.horizontalCenter
                width: menu.menuWidth - desk.gap
                spacing: desk.gap * 0.4
                Step {
                    number: "1"
                    text: "Extensions, small programs in any language, offer apps, projects, clips and emoji."
                }
                Step {
                    number: "2"
                    text: "The Daemon, kotodamad, ranks them on every keystroke and learns from your picks."
                }
                Step {
                    number: "3"
                    text: "Enter runs a Result's first Action, and Ctrl+K lists the others."
                }
            }
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: desk.gap / 3
                Note {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Every feature, with more to try:"
                    color: desk.text
                }
                Query {
                    text: "guide"
                    big: true
                }
            }
        }

        // Left of the menu.
        Column {
            id: leftColumn
            x: desk.gap
            y: desk.columnsTop
            width: desk.side
            spacing: desk.gap * 0.75
            Card {
                title: "Apps"
                hue: "#89b4fa"
                Example {
                    query: "fea"
                    does: "starts FeatherPad"
                }
                Example {
                    query: "featerpad"
                    does: "a typo still finds it"
                }
                Example {
                    query: "fs"
                    does: "initials: Foot Server"
                }
                Example {
                    query: "htop"
                    does: "opens in a terminal"
                }
            }
            Card {
                title: "Projects and scripts"
                hue: "#a6e3a1"
                Example {
                    query: "aurora"
                    does: "git project, in tmux"
                }
                Example {
                    query: "api"
                    does: "work/api and home/api"
                }
                Example {
                    query: "deploy"
                    does: "runs a script"
                }
                Example {
                    query: "greet"
                    does: "asks for its arguments"
                }
                Example {
                    query: "backup"
                    does: "runs as a job"
                }
            }
            Card {
                title: "Calculator"
                hue: "#fab387"
                Example {
                    query: "12 km in mi"
                    does: "units"
                }
                Example {
                    query: "2^10"
                    does: "math; Enter copies"
                }
                Example {
                    query: "100 usd to eur"
                    does: "currency"
                }
                Example {
                    query: "5pm PT in Warsaw"
                    does: "time zones"
                }
                Example {
                    query: "days until dec 25"
                    does: "dates"
                }
            }
        }

        // Right of the menu.
        Column {
            id: rightColumn
            x: desk.menuRight + desk.gap
            y: desk.columnsTop
            width: desk.side
            spacing: desk.gap * 0.75
            Card {
                title: "Your keywords"
                hue: "#cba6f7"
                Example {
                    query: "ed"
                    does: "Alias for FeatherPad"
                }
                Example {
                    query: "aur"
                    does: "Alias: terminal in aurora"
                }
                Example {
                    query: "yt cats"
                    does: "Quicklink with a query"
                }
                Example {
                    query: "say hi"
                    does: "Quicklink: a command"
                }
            }
            Card {
                title: "On any Result"
                hue: "#f9e2af"
                Example {
                    keys: "Ctrl+K"
                    does: "all its Actions"
                }
                Example {
                    keys: "Ctrl+Shift+⌫"
                    does: "Forget it"
                }
                Example {
                    keys: "Ctrl+Z"
                    does: "undo the Forget"
                }
                Example {
                    query: "undo"
                    does: "undo later"
                }
                Example {
                    query: "clear history"
                    does: "start over"
                }
                Note {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    text: "Pick one Result 3 times for a query and it comes first for that query: a Learned choice."
                }
            }
            Card {
                title: "Views, emoji and the web"
                hue: "#94e2d5"
                Example {
                    query: "timer 5"
                    does: "a progress View"
                }
                Example {
                    query: "fire"
                    does: "types 🔥 where you were"
                }
                Example {
                    query: "? open a terminal"
                    does: "AI, with an API key"
                }
                Example {
                    query: "example.com"
                    does: "opens an address"
                }
                Example {
                    query: "anything else"
                    does: "web search"
                }
            }
        }
    }

    // A card: a colored heading over its examples.
    component Card: Rectangle {
        id: card

        property string title
        property color hue
        default property alias content: body.data

        width: parent.width
        height: body.height + 2 * desk.gap * 0.7
        radius: menu.radius
        color: desk.card
        border.color: desk.line

        Rectangle {
            x: 0
            y: desk.gap * 0.85
            width: 3
            height: Math.round(desk.font * 1.3)
            color: card.hue
        }
        Column {
            id: body
            x: desk.gap * 0.8
            y: desk.gap * 0.7
            width: card.width - 2 * x
            spacing: desk.gap * 0.32
            Text {
                text: card.title
                color: card.hue
                font.family: menu.fontFamily
                font.pixelSize: Math.round(desk.font * 1.1)
                font.weight: Font.DemiBold
                bottomPadding: desk.gap * 0.15
            }
        }
    }

    // One thing to try: a query (or keys) and what it does.
    component Example: Item {
        id: example

        property string query
        property string keys
        property string does

        width: parent.width
        height: Math.max(chipItem.height, capsItem.height, whatText.height)

        Query {
            id: chipItem
            visible: example.query !== ""
            text: example.query
        }
        KeyCaps {
            id: capsItem
            visible: example.keys !== ""
            keys: example.keys
        }
        Text {
            id: whatText
            anchors.verticalCenter: parent.verticalCenter
            x: Math.max(parent.width * 0.42, (example.query !== "" ? chipItem.width : capsItem.width) + desk.gap / 2)
            width: parent.width - x
            wrapMode: Text.WordWrap
            text: example.does
            color: desk.soft
            font.family: menu.fontFamily
            font.pixelSize: desk.font
        }
    }

    // Text to type, as it would look typed.
    component Query: Rectangle {
        property alias text: label.text
        property bool big: false

        width: label.implicitWidth + desk.font
        height: label.implicitHeight + desk.font * 0.45
        radius: 6
        color: desk.chip
        border.color: desk.line
        Text {
            id: label
            anchors.centerIn: parent
            color: menu.accent
            font.family: "monospace"
            font.pixelSize: Math.round(desk.font * (parent.big ? 1.2 : 0.95))
        }
    }

    // Keys to press, each as a keycap.
    component KeyCaps: Row {
        id: keysRow

        property string keys
        property bool big: false

        spacing: 4
        Repeater {
            model: keysRow.keys.split("+")
            Rectangle {
                required property string modelData
                width: Math.max(height, capText.implicitWidth + desk.font * 0.8)
                height: capText.implicitHeight + desk.font * 0.4
                radius: 5
                color: "#262637"
                border.color: "#45475a"
                Rectangle {
                    anchors.bottom: parent.bottom
                    width: parent.width
                    height: 2
                    radius: 1
                    color: "#45475a"
                }
                Text {
                    id: capText
                    anchors.centerIn: parent
                    text: parent.modelData
                    color: desk.text
                    font.family: menu.fontFamily
                    font.pixelSize: Math.round(desk.font * (keysRow.big ? 1.15 : 0.85))
                }
            }
        }
    }

    // Keys, and what they open.
    component Hint: Row {
        property alias keys: hintKeys.keys
        property alias what: hintText.text

        spacing: desk.gap / 4
        KeyCaps {
            id: hintKeys
        }
        Note {
            id: hintText
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    // One step of how Kotodama works.
    component Step: Row {
        property alias number: numberText.text
        property alias text: stepText.text

        width: parent.width
        spacing: desk.gap / 2
        Text {
            id: numberText
            color: menu.accent
            font.family: menu.fontFamily
            anchors.verticalCenter: parent.verticalCenter
            width: desk.font
            horizontalAlignment: Text.AlignHCenter
            font.pixelSize: Math.round(desk.font * 1.3)
            font.weight: Font.DemiBold
        }
        Note {
            id: stepText
            width: parent.width - numberText.width - parent.spacing
            anchors.verticalCenter: parent.verticalCenter
            wrapMode: Text.WordWrap
        }
    }

    component Note: Text {
        lineHeight: 1.15
        color: desk.soft
        font.family: menu.fontFamily
        font.pixelSize: desk.font
    }
}
