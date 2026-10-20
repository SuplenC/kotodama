// The menu: a layer-shell window with a search box over the Daemon's Results,
// the Views Extensions push on top of it, the jobs they run, and each Result's
// Action panel. Its look comes from the properties below, so a host shell can
// bind them to its own theme; it can also draw its own window around a
// KotodamaSearch.
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
    property color error: "#f38ba8"
    property string fontFamily: "sans-serif"
    property int fontSize: 15
    property real radius: 12
    property int menuWidth: 640
    property int rowHeight: 44
    property int maxRows: 8
    property real top: screen ? screen.height / 5 : 120
    property string placeholder: "Search"

    readonly property KotodamaSearch search: search
    // Whether keys reach the menu: the compositor has given the window
    // keyboard focus.
    readonly property bool focused: input.activeFocus || filter.activeFocus || viewKeys.activeFocus || form.activeFocus || actionPanel.activeFocus
    // Why the last Action failed, until the next Action or opening.
    property string failure: ""
    // The top View's kind (`list`, `form`, `detail` or `progress`), or empty
    // at the search.
    readonly property string kind: search.view.kind ?? ""
    // The Action panel of the selected row while Ctrl+K shows it: each entry
    // `{title, group, shortcut}`, as `KotodamaSearch.actions` gives them.
    property var actions: []
    readonly property bool actionsOpen: actions.length > 0

    function show(query) {
        menu.failure = "";
        input.text = query;
        input.cursorPosition = query.length;
        visible = true;
        menu.focusView();
    }

    // Closing pops no View: Extensions aren't told the user left one, so their
    // jobs keep running, and the menu opens again where it was. Only Esc pops.
    function hide() {
        menu.actions = [];
        visible = false;
    }

    // Gives keys to the top View: the search box, a list's filter, the first
    // field of a form, or the menu itself for a detail or a progress View.
    function focusView() {
        if (menu.actionsOpen)
            actionPanel.forceActiveFocus();
        else if (menu.kind === "")
            input.forceActiveFocus();
        else if (menu.kind === "list")
            filter.forceActiveFocus();
        else if (menu.kind === "form" && fields.count > 0)
            fields.itemAt(0).take();
        else
            viewKeys.forceActiveFocus();
    }

    function openActions() {
        const actions = search.actions(list.currentIndex);
        if (actions.length > 0) {
            actionList.row = list.currentIndex;
            actionList.current = 0;
            menu.actions = actions;
            actionPanel.forceActiveFocus();
        }
    }

    function closeActions() {
        menu.actions = [];
        menu.focusView();
    }

    // The keys every View shares. True if `event` was one of them.
    function key(event) {
        const ctrl = event.modifiers & Qt.ControlModifier;
        const enter = event.key === Qt.Key_Return || event.key === Qt.Key_Enter;
        if (event.key === Qt.Key_Escape) {
            // Esc clears a list's filter first, then pops the View.
            if (menu.kind === "list" && filter.text !== "")
                filter.text = "";
            else if (search.depth > 0)
                search.popView();
            else
                menu.hide();
            return true;
        }
        if (menu.kind === "form") {
            if (enter) {
                menu.failure = "";
                search.submit();
            } else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Down) {
                form.move(1);
            } else if (event.key === Qt.Key_Backtab || event.key === Qt.Key_Up) {
                form.move(-1);
            } else {
                return false;
            }
            return true;
        }
        if (menu.kind !== "" && menu.kind !== "list")
            return false;
        if (ctrl && event.key === Qt.Key_K) {
            menu.openActions();
        } else if (enter) {
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
        } else if ((event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) && search.runShortcut(list.currentIndex, event.key, event.modifiers)) {
            menu.failure = "";
        } else {
            return false;
        }
        return true;
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
        // A list View's rows changed: its selected Item is at `row`, or the
        // top row is selected, as in the search, if it went away.
        onSelectRow: row => {
            list.currentIndex = row;
            if (row === 0)
                list.positionViewAtBeginning();
            else
                list.positionViewAtIndex(row, ListView.Contain);
        }
        onDepthChanged: {
            menu.actions = [];
            list.currentIndex = 0;
            list.positionViewAtBeginning();
        }
        // A View that is pushed, popped or changed takes the keys; one whose
        // job progressed keeps them where they are. A list's filter shows
        // what was typed into it, which is nothing for a new one.
        onViewChanged: {
            const typed = search.view.filter ?? "";
            if (filter.text !== typed)
                filter.text = typed;
            if (menu.visible && !menu.actionsOpen)
                Qt.callLater(menu.focusView);
        }
    }

    Rectangle {
        id: panel
        width: parent.width
        height: field.height + jobs.height + body.height + footer.height
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
                visible: menu.kind === ""
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

                Keys.onPressed: event => event.accepted = menu.key(event)
            }

            // A list View's filter, with the View's title until something is
            // typed. The Daemon matches what is typed as it does the search,
            // so the rows narrow as they would there.
            TextInput {
                id: filter
                anchors.fill: parent
                anchors.leftMargin: 18
                anchors.rightMargin: 18
                visible: menu.kind === "list"
                verticalAlignment: TextInput.AlignVCenter
                color: menu.foreground
                selectionColor: menu.selection
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize + 3
                // The selection stays where it is until the matches come;
                // see onSelectRow.
                onTextChanged: search.filter(text)

                Text {
                    anchors.fill: parent
                    verticalAlignment: Text.AlignVCenter
                    visible: filter.text === ""
                    elide: Text.ElideRight
                    text: search.view.title ?? ""
                    color: menu.dimmed
                    font: filter.font
                }

                Keys.onPressed: event => event.accepted = menu.key(event)
            }

            // The title of any other View stands where the search box was.
            Text {
                anchors.fill: parent
                anchors.leftMargin: 18
                anchors.rightMargin: 18
                visible: menu.kind !== "" && menu.kind !== "list"
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
                text: search.view.title ?? ""
                color: menu.foreground
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize + 3
                font.bold: true
            }

            // Takes the keys of a View with nothing to type into.
            Item {
                id: viewKeys
                Keys.onPressed: event => event.accepted = menu.key(event)
            }
        }

        // The jobs still running, under the search box.
        Column {
            id: jobs
            y: field.height
            width: parent.width
            visible: menu.kind !== "progress"
            height: visible ? implicitHeight : 0

            Repeater {
                model: search.jobs

                Item {
                    id: running
                    required property var modelData
                    width: jobs.width
                    height: menu.rowHeight * 0.7

                    Text {
                        anchors.left: parent.left
                        anchors.leftMargin: 18
                        anchors.right: bar.left
                        anchors.rightMargin: 12
                        anchors.verticalCenter: parent.verticalCenter
                        elide: Text.ElideRight
                        color: menu.dimmed
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize - 3
                        text: running.modelData.title + (running.modelData.message !== "" ? " · " + running.modelData.message : "")
                    }

                    ProgressBar {
                        id: bar
                        anchors.right: parent.right
                        anchors.rightMargin: 18
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width / 4
                        progress: running.modelData.progress
                        color: menu.selection
                        fill: menu.accent
                        unknown: menu.border
                        error: menu.error
                    }
                }
            }
        }

        Item {
            id: body
            y: field.height + jobs.height
            width: parent.width
            height: menu.kind === "" || menu.kind === "list" ? list.height : Math.min(content.height, menu.maxRows * menu.rowHeight)
            clip: true

            ListView {
                id: list
                width: parent.width
                visible: menu.kind === "" || menu.kind === "list"
                height: visible ? Math.min(count * menu.rowHeight, menu.maxRows * menu.rowHeight - jobs.height) : 0
                clip: true
                model: search
                boundsBehavior: Flickable.StopAtBounds
                highlightMoveDuration: 0
                onCountChanged: {
                    if (currentIndex < 0 && count > 0)
                        currentIndex = 0;
                }
                // A list View's filter keeps the selected Item selected. In the
                // search, moving off the top row stops Late Results reranking
                // the rows.
                onCurrentIndexChanged: search.select(currentIndex)

                // Once the rerank after a keystroke is over, Late Results go
                // below the rows on screen, so the Daemon is told which those
                // are.
                readonly property int firstRow: Math.floor(contentY / menu.rowHeight)
                onFirstRowChanged: search.viewport(firstRow, menu.maxRows)

                // A run names the Results of the last frame on screen, so the
                // Daemon refuses a Late Result the user hasn't seen yet.
                // afterAnimating comes on this thread just before the frame
                // takes the model; frameSwapped comes queued from the render
                // thread, after Results that arrived since may have changed it.
                Connections {
                    target: list.Window.window
                    function onAfterAnimating() {
                        search.frameStarted();
                    }
                    function onFrameSwapped() {
                        search.painted();
                    }
                }

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

            // A form, a detail or a progress View.
            Column {
                id: content
                width: parent.width
                visible: !list.visible
                topPadding: 4
                bottomPadding: 12
                spacing: 8

                // The form: Tab and Shift+Tab move between fields, Space ticks a
                // checkbox, Left and Right choose an option, Enter submits.
                FocusScope {
                    id: form
                    width: parent.width
                    visible: menu.kind === "form"
                    height: visible ? formFields.implicitHeight : 0
                    property int current: 0

                    function move(by) {
                        if (fields.count > 0)
                            fields.itemAt((current + by + fields.count) % fields.count).take();
                    }

                    Column {
                        id: formFields
                        width: parent.width
                        spacing: 8

                        Repeater {
                            id: fields
                            model: menu.kind === "form" ? search.view.fields : []

                            Item {
                                id: formField
                                required property int index
                                required property var modelData
                                width: formFields.width
                                height: menu.rowHeight
                                readonly property bool current: typed.activeFocus || toggle.activeFocus || choice.activeFocus

                                function take() {
                                    const kind = modelData.kind;
                                    (kind === "checkbox" ? toggle : kind === "select" ? choice : typed).forceActiveFocus();
                                }

                                onCurrentChanged: {
                                    if (current)
                                        form.current = index;
                                }

                                Text {
                                    id: label
                                    x: 18
                                    width: parent.width / 3 - 18
                                    anchors.verticalCenter: parent.verticalCenter
                                    elide: Text.ElideRight
                                    text: formField.modelData.label
                                    color: formField.current ? menu.foreground : menu.dimmed
                                    font.family: menu.fontFamily
                                    font.pixelSize: menu.fontSize
                                }

                                Rectangle {
                                    anchors.left: label.right
                                    anchors.right: parent.right
                                    anchors.rightMargin: 18
                                    anchors.verticalCenter: parent.verticalCenter
                                    height: menu.rowHeight - 12
                                    radius: menu.radius / 2
                                    color: "transparent"
                                    border.width: 1
                                    border.color: formField.current ? menu.accent : menu.border

                                    TextInput {
                                        id: typed
                                        anchors.fill: parent
                                        anchors.leftMargin: 10
                                        anchors.rightMargin: 10
                                        visible: formField.modelData.kind === "text"
                                        verticalAlignment: TextInput.AlignVCenter
                                        clip: true
                                        color: menu.foreground
                                        selectionColor: menu.selection
                                        font.family: menu.fontFamily
                                        font.pixelSize: menu.fontSize
                                        echoMode: formField.modelData.secret ? TextInput.Password : TextInput.Normal
                                        Component.onCompleted: {
                                            if (formField.modelData.kind === "text")
                                                text = formField.modelData.value;
                                        }
                                        onTextChanged: search.setText(formField.index, text)
                                        Keys.onPressed: event => event.accepted = menu.key(event)

                                        Text {
                                            anchors.fill: parent
                                            verticalAlignment: Text.AlignVCenter
                                            visible: typed.text === ""
                                            text: formField.modelData.placeholder
                                            color: menu.dimmed
                                            font: typed.font
                                        }
                                    }

                                    Text {
                                        id: toggle
                                        anchors.fill: parent
                                        anchors.leftMargin: 10
                                        visible: formField.modelData.kind === "checkbox"
                                        verticalAlignment: Text.AlignVCenter
                                        property bool checked: formField.modelData.value === true
                                        text: checked ? "[x]" : "[ ]"
                                        color: menu.foreground
                                        font.family: menu.fontFamily
                                        font.pixelSize: menu.fontSize
                                        Keys.onPressed: event => {
                                            if (menu.key(event)) {
                                                event.accepted = true;
                                            } else if (event.key === Qt.Key_Space) {
                                                checked = !checked;
                                                search.setChecked(formField.index, checked);
                                                event.accepted = true;
                                            }
                                        }
                                    }

                                    Text {
                                        id: choice
                                        anchors.fill: parent
                                        anchors.leftMargin: 10
                                        anchors.rightMargin: 10
                                        visible: formField.modelData.kind === "select"
                                        verticalAlignment: Text.AlignVCenter
                                        elide: Text.ElideRight
                                        readonly property var options: formField.modelData.options
                                        property int chosen: Math.max(0, options.findIndex(option => option.value === formField.modelData.value))
                                        text: options.length > 0 ? "‹ " + options[chosen].label + " ›" : ""
                                        color: menu.foreground
                                        font.family: menu.fontFamily
                                        font.pixelSize: menu.fontSize
                                        Keys.onPressed: event => {
                                            const by = event.key === Qt.Key_Right ? 1 : event.key === Qt.Key_Left ? -1 : 0;
                                            if (menu.key(event)) {
                                                event.accepted = true;
                                            } else if (by !== 0 && options.length > 0) {
                                                chosen = (chosen + by + options.length) % options.length;
                                                search.setText(formField.index, options[chosen].value);
                                                event.accepted = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                Text {
                    id: submitHint
                    x: 18
                    visible: menu.kind === "form"
                    text: "Enter: " + (search.view.submit ?? "")
                    color: menu.dimmed
                    font.family: menu.fontFamily
                    font.pixelSize: menu.fontSize - 3
                }

                // A detail View: Markdown and the metadata beside it.
                Text {
                    x: 18
                    width: parent.width - 36
                    visible: menu.kind === "detail"
                    wrapMode: Text.Wrap
                    textFormat: Text.MarkdownText
                    text: search.view.markdown ?? ""
                    color: menu.foreground
                    font.family: menu.fontFamily
                    font.pixelSize: menu.fontSize
                }

                Repeater {
                    model: menu.kind === "detail" ? search.view.metadata : []

                    Text {
                        required property var modelData
                        x: 18
                        width: content.width - 36
                        elide: Text.ElideRight
                        text: modelData.label + ": " + modelData.value
                        color: menu.dimmed
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize - 3
                    }
                }

                // A progress View: the job it follows.
                Column {
                    x: 18
                    width: parent.width - 36
                    visible: menu.kind === "progress"
                    spacing: 8
                    readonly property var job: search.view.job ?? ({})

                    ProgressBar {
                        width: parent.width
                        progress: parent.job.progress ?? -1
                        failed: parent.job.state === "failed"
                        color: menu.selection
                        fill: menu.accent
                        unknown: menu.border
                        error: menu.error
                    }

                    Text {
                        width: parent.width
                        elide: Text.ElideRight
                        text: {
                            const job = parent.job;
                            const state = job.state === "succeeded" ? "Done" : job.state === "failed" ? "Failed" : job.progress >= 0 ? Math.round(job.progress * 100) + "%" : "Running";
                            return job.message ? state + " · " + job.message : state;
                        }
                        color: parent.job.state === "failed" ? menu.error : menu.dimmed
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize - 3
                    }
                }
            }
        }

        Text {
            id: footer
            anchors.top: body.bottom
            width: parent.width
            leftPadding: 18
            rightPadding: 18
            height: text === "" ? 0 : menu.rowHeight * 0.7
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            color: menu.failure !== "" ? menu.error : menu.dimmed
            font.family: menu.fontFamily
            font.pixelSize: menu.fontSize - 3
            text: menu.failure !== "" ? menu.failure : search.down.length > 0 ? "Unavailable: " + search.down.join(", ") : ""
        }

        // The Action panel: Ctrl+K lists the selected Result's Actions, the
        // Item's own first, then the Contributed ones under the Extension that
        // offers each. Up and Down choose, Enter runs, Esc or Ctrl+K closes it.
        Rectangle {
            id: actionPanel
            visible: menu.actionsOpen
            anchors.right: parent.right
            anchors.rightMargin: 8
            y: field.height
            width: parent.width / 2
            height: actionList.implicitHeight + 12
            color: menu.background
            radius: menu.radius / 2
            border.color: menu.border
            border.width: 1

            Keys.onPressed: event => {
                const ctrl = event.modifiers & Qt.ControlModifier;
                if (event.key === Qt.Key_Escape || (ctrl && event.key === Qt.Key_K)) {
                    menu.closeActions();
                } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N)) {
                    actionList.current = Math.min(menu.actions.length - 1, actionList.current + 1);
                } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P)) {
                    actionList.current = Math.max(0, actionList.current - 1);
                } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    menu.failure = "";
                    search.runAction(actionList.row, actionList.current);
                    menu.closeActions();
                } else if (search.runShortcut(actionList.row, event.key, event.modifiers)) {
                    menu.failure = "";
                    menu.closeActions();
                }
                event.accepted = true;
            }

            Column {
                id: actionList
                y: 6
                width: parent.width
                property int row: 0
                property int current: 0

                Repeater {
                    model: menu.actions

                    Column {
                        id: entry
                        required property int index
                        required property var modelData
                        width: actionList.width
                        readonly property bool heading: modelData.group !== "" && (index === 0 || menu.actions[index - 1].group !== modelData.group)

                        Text {
                            visible: entry.heading
                            leftPadding: 12
                            height: visible ? menu.rowHeight * 0.6 : 0
                            verticalAlignment: Text.AlignBottom
                            text: entry.modelData.group
                            color: menu.dimmed
                            font.family: menu.fontFamily
                            font.pixelSize: menu.fontSize - 4
                            font.bold: true
                        }

                        Item {
                            width: parent.width
                            height: menu.rowHeight * 0.8

                            Rectangle {
                                anchors.fill: parent
                                anchors.leftMargin: 6
                                anchors.rightMargin: 6
                                radius: menu.radius / 2
                                color: entry.index === actionList.current ? menu.selection : "transparent"
                            }

                            Text {
                                anchors.left: parent.left
                                anchors.leftMargin: 12
                                anchors.right: shortcut.left
                                anchors.rightMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                elide: Text.ElideRight
                                text: entry.modelData.title
                                color: menu.foreground
                                font.family: menu.fontFamily
                                font.pixelSize: menu.fontSize
                            }

                            Text {
                                id: shortcut
                                anchors.right: parent.right
                                anchors.rightMargin: 12
                                anchors.verticalCenter: parent.verticalCenter
                                text: entry.index === 0 ? "Enter" : entry.modelData.shortcut
                                color: menu.dimmed
                                font.family: menu.fontFamily
                                font.pixelSize: menu.fontSize - 3
                            }
                        }
                    }
                }
            }
        }
    }

    // How far a job has got: a bar that fills, or a dimmed one when the
    // Extension can't tell.
    component ProgressBar: Rectangle {
        id: bar
        property real progress: -1
        property bool failed: false
        // Inline components can't see the menu, so the colors are passed in.
        property color fill
        property color unknown
        property color error
        height: 4
        radius: 2

        Rectangle {
            height: bar.height
            radius: bar.radius
            width: bar.progress < 0 ? bar.width : bar.width * Math.min(1, bar.progress)
            color: bar.failed ? bar.error : bar.progress < 0 ? bar.unknown : bar.fill
        }
    }
}
