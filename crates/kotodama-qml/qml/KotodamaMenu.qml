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
    // Whether the footer names the keys the selected row takes, such as
    // "↵ Open · Tab Enter app · ⌃K Actions", when it has nothing else to say.
    property bool showHints: true

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
    // `{id, title, group, shortcut, role, keywords}`, as
    // `KotodamaSearch.actions` gives them.
    property var actions: []
    readonly property bool actionsOpen: actions.length > 0
    // The keys the selected row takes, built for that row alone. Reading the
    // row's markup and unavailable makes the hints follow a new Result in
    // the same place, not only a move of the selection.
    readonly property string hints: {
        const row = list.currentItem;
        if (!menu.showHints || !row || (menu.kind !== "" && menu.kind !== "list"))
            return "";
        row.markup;
        row.unavailable;
        row.primaryTitle;
        row.primaryChosen;
        return search.hints(list.currentIndex);
    }
    // The hints as StyledText when the query chose the Action Enter runs: its
    // `↵` hint takes the colour of the row's pill. Empty otherwise.
    readonly property string chosenHints: {
        const row = list.currentItem;
        const enter = row ? "↵ " + row.primaryTitle : "";
        if (!row || !row.primaryChosen || !menu.hints.startsWith(enter))
            return "";
        const named = "↵ " + menu.actionTitle(row.primaryTitle, row.primaryRole);
        return "<font color=\"" + menu.roleText(row.primaryRole) + "\">" + menu.escaped(named) + "</font>" + menu.escaped(menu.hints.slice(enter.length));
    }

    // An Action's title as the menu shows it: one that opens a form ends in
    // an ellipsis, unless its title already does.
    function actionTitle(title, role) {
        return role === "form" && !title.endsWith("…") && !title.endsWith("...") ? title + "…" : title;
    }

    // The colour of an Action's title where its role shows, and the border
    // of the pill around it.
    function roleText(role) {
        return role === "destructive" ? menu.error : role === "form" ? menu.accent : menu.dimmed;
    }
    function roleBorder(role) {
        return role === "destructive" ? menu.error : role === "form" ? menu.accent : menu.border;
    }

    function escaped(text) {
        return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    }

    // Which part of the Action panel its `n`th entry is in: the Item's own
    // Actions (0), the Contributed ones (1) or the destructive ones (2). The
    // entry Enter runs comes first, whatever it is: it leads the Item's own,
    // or stands alone (-1) when another Extension offers it, so its heading
    // doesn't seem to cover the Item's own Actions under it.
    function panelSection(n) {
        const entry = menu.actions[n];
        if (n === 0)
            return entry.group === "" ? 0 : -1;
        return entry.role === "destructive" ? 2 : entry.group !== "" ? 1 : 0;
    }

    function show(query) {
        menu.failure = "";
        input.text = query;
        input.cursorPosition = query.length;
        // The box may show `query` already, in an app it has left.
        search.query = query;
        list.pointer = null;
        visible = true;
        menu.focusView();
    }

    // Closing pops no View: Extensions aren't told the user left one, so their
    // jobs keep running, and the menu opens again where it was. Only Esc pops.
    function hide() {
        menu.actions = [];
        search.cancelConfirm();
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

    // Ctrl and no other modifier: the menu's own Ctrl keys leave Ctrl+Shift+K
    // and the like to the Actions' key bindings.
    function ctrlOnly(event) {
        return (event.modifiers & (Qt.ControlModifier | Qt.ShiftModifier | Qt.AltModifier | Qt.MetaModifier)) === Qt.ControlModifier;
    }

    // Whether `event` is a modifier pressed on its own, as on the way to a
    // key binding.
    function modifierAlone(event) {
        return [Qt.Key_Shift, Qt.Key_Control, Qt.Key_Alt, Qt.Key_Meta, Qt.Key_Super_L, Qt.Key_Super_R].includes(event.key);
    }

    // While the Toast shows, its key undoes what it offers to, whatever has
    // the keys; otherwise the key is the text box's own undo. True if
    // `event` was it.
    function undoKey(event) {
        if (search.toast === "" || !search.isUndoKey(event.key, event.modifiers))
            return false;
        search.undo();
        return true;
    }

    // The keys every View shares. True if `event` was one of them.
    function key(event) {
        const ctrl = menu.ctrlOnly(event);
        const enter = event.key === Qt.Key_Return || event.key === Qt.Key_Enter;
        if (menu.undoKey(event))
            return true;
        // An Action that asks first waits in the footer: Enter runs it, Esc
        // drops it, and any other key drops it and does what it does.
        if (search.confirming !== "" && !menu.modifierAlone(event)) {
            if (enter) {
                menu.failure = "";
                search.confirm();
                return true;
            }
            search.cancelConfirm();
            if (event.key === Qt.Key_Escape)
                return true;
        }
        if (event.key === Qt.Key_Escape) {
            // One rule everywhere, so Esc never throws away more than the
            // user can see: it clears the text if there is any, else pops
            // the View, else leaves the app for the whole search, else
            // closes the menu.
            if (menu.kind === "list" && filter.text !== "")
                filter.text = "";
            else if (menu.kind === "" && input.text !== "")
                input.text = "";
            else if (search.depth > 0)
                search.popView();
            else if (search.leaveApp())
                menu.failure = "";
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
        } else if (event.key === Qt.Key_Tab && menu.kind === "" && search.enter(list.currentIndex)) {
            // Tab on an app's entry Item enters the app.
            menu.failure = "";
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
    // panel inside grows with the Results. The Toast's row is kept even
    // while it is hidden, so showing it doesn't resize the surface either.
    implicitWidth: menuWidth
    implicitHeight: field.height + maxRows * rowHeight + footer.height + rowHeight * 0.7 + 16
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "kotodama"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    // The Daemon types or pastes once the window is gone and focus is back
    // where the user came from, so it is told only after the window unmaps.
    property bool closing: false
    // However the window opened, the Daemon is told, so the Empty query it
    // shows is ranked for this time of day.
    onVisibleChanged: {
        if (visible)
            search.shown();
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
        // The box shows the rest of the query once it is in an app,
        // or what the Daemon typed. The cursor keeps its place from the end.
        onQueryReplaced: text => {
            const fromEnd = input.text.length - input.cursorPosition;
            input.text = text;
            input.cursorPosition = Math.max(0, text.length - fromEnd);
        }
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
        height: field.height + jobs.height + body.height + footer.height + toast.height
        color: menu.background
        radius: menu.radius
        border.color: menu.border
        border.width: 1

        Item {
            id: field
            width: parent.width
            height: menu.rowHeight + 12

            // The app the search is in, such as Emoji: the box holds what
            // follows the app's word.
            Rectangle {
                id: chip
                objectName: "scope"
                x: 14
                anchors.verticalCenter: parent.verticalCenter
                visible: menu.kind === "" && search.scope !== ""
                width: chipLabel.implicitWidth + 20
                height: menu.rowHeight - 16
                radius: height / 2
                color: menu.selection
                border.color: menu.accent
                border.width: 1

                Text {
                    id: chipLabel
                    anchors.centerIn: parent
                    text: search.scope
                    color: menu.accent
                    font.family: menu.fontFamily
                    font.pixelSize: menu.fontSize
                }
            }

            TextInput {
                id: input
                anchors.fill: parent
                anchors.leftMargin: chip.visible ? chip.x + chip.width + 10 : 18
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
                    text: search.scope !== "" ? "Search " + search.scope : menu.placeholder
                    color: menu.dimmed
                    font: input.font
                }

                // Backspace with nothing typed in an app leaves it: the whole
                // search again, with the app's word typed.
                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Backspace && input.text === "" && search.leaveScope())
                        event.accepted = true;
                    else
                        event.accepted = menu.key(event);
                }
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
            height: menu.kind === "" || menu.kind === "list" ? intro.height + list.height : Math.min(content.height, menu.maxRows * menu.rowHeight)
            clip: true

            // An app's intro card, before anything is typed in it: what it
            // does, and a few queries with what they give. It comes with the
            // app's first Results, so the first frame in the app shows it.
            Column {
                id: intro
                objectName: "intro"
                width: parent.width
                visible: menu.kind === "" && search.introAbout !== ""
                height: visible ? implicitHeight : 0
                topPadding: 10
                bottomPadding: 10
                spacing: 4

                // What the Extension says now, such as that it is paused or
                // why it can't do anything, above what it always says.
                Text {
                    x: 18
                    width: intro.width - 36
                    visible: search.introNote !== ""
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                    text: search.introNote
                    color: menu.accent
                    font.family: menu.fontFamily
                    font.pixelSize: menu.fontSize - 1
                }

                Text {
                    x: 18
                    width: intro.width - 36
                    wrapMode: Text.Wrap
                    maximumLineCount: 3
                    elide: Text.ElideRight
                    text: search.introAbout
                    color: menu.foreground
                    font.family: menu.fontFamily
                    font.pixelSize: menu.fontSize - 1
                }

                Repeater {
                    model: search.introExamples

                    Text {
                        required property var modelData
                        x: 18
                        width: intro.width - 36
                        elide: Text.ElideRight
                        textFormat: Text.PlainText
                        text: modelData.query + "  →  " + modelData.result
                        color: menu.dimmed
                        font.family: menu.fontFamily
                        font.pixelSize: menu.fontSize - 3
                    }
                }

                Rectangle {
                    x: 18
                    width: intro.width - 36
                    height: 1
                    color: menu.border
                }
            }

            ListView {
                id: list
                y: intro.height
                width: parent.width
                visible: menu.kind === "" || menu.kind === "list"
                height: visible ? Math.max(0, Math.min(count * menu.rowHeight, menu.maxRows * menu.rowHeight - jobs.height - intro.height)) : 0
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

                // Where the pointer last was in the window, or null before it
                // has been over a row since the menu opened.
                property var pointer: null

                // Only a pointer that moved picks a row. A window that maps
                // under a resting pointer gets a pointer-enter, and Qt resends
                // hover when rows move under a still pointer: either would
                // otherwise take the selection off the top row the user is
                // typing for.
                function hover(index, area, x, y) {
                    const at = area.mapToItem(null, x, y);
                    const moved = list.pointer !== null && (at.x !== list.pointer.x || at.y !== list.pointer.y);
                    list.pointer = at;
                    if (moved)
                        list.currentIndex = index;
                }

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
                    // The Alias the query named this Result by, drawn as a
                    // badge; empty for none.
                    required property string alias
                    // Why the Result can't run, shown in place of its
                    // subtitle; the row is greyed out. Empty when it can run.
                    required property string unavailable
                    // The Action Enter runs, its role, and whether the query
                    // chose it by a verb in place of the Item's first Action.
                    required property string primaryTitle
                    required property string primaryRole
                    required property bool primaryChosen

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
                        opacity: row.unavailable === "" ? 1 : 0.4
                        source: row.icon.startsWith("/") ? "file://" + row.icon : Quickshell.iconPath(row.icon, true)
                    }

                    // The Action a verb in the query chose, which Enter runs,
                    // in its role's colour.
                    Rectangle {
                        id: verb
                        anchors.right: parent.right
                        anchors.rightMargin: 16
                        anchors.verticalCenter: parent.verticalCenter
                        visible: row.primaryChosen
                        width: visible ? verbText.implicitWidth + 18 : 0
                        height: verbText.implicitHeight + 4
                        radius: height / 2
                        color: "transparent"
                        border.color: menu.roleBorder(row.primaryRole)

                        Text {
                            id: verbText
                            anchors.centerIn: parent
                            text: "↵ " + menu.actionTitle(row.primaryTitle, row.primaryRole)
                            textFormat: Text.PlainText
                            color: menu.roleText(row.primaryRole)
                            font.family: menu.fontFamily
                            font.pixelSize: menu.fontSize - 4
                        }
                    }

                    Rectangle {
                        id: badge
                        anchors.right: verb.left
                        anchors.rightMargin: verb.visible && badge.visible ? 6 : 0
                        anchors.verticalCenter: parent.verticalCenter
                        visible: row.alias !== ""
                        width: visible ? badgeText.implicitWidth + 12 : 0
                        height: badgeText.implicitHeight + 4
                        radius: height / 2
                        color: "transparent"
                        border.color: menu.border

                        Text {
                            id: badgeText
                            anchors.centerIn: parent
                            text: row.alias
                            color: menu.dimmed
                            font.family: menu.fontFamily
                            font.pixelSize: menu.fontSize - 4
                        }
                    }

                    Column {
                        anchors.left: parent.left
                        anchors.leftMargin: 16 + menu.rowHeight - 16 + 12
                        anchors.right: badge.left
                        anchors.rightMargin: badge.visible || verb.visible ? 8 : 0
                        anchors.verticalCenter: parent.verticalCenter

                        Text {
                            width: parent.width
                            text: row.markup
                            textFormat: Text.StyledText
                            elide: Text.ElideRight
                            color: row.unavailable === "" ? menu.foreground : menu.dimmed
                            opacity: row.unavailable === "" ? 1 : 0.6
                            font.family: menu.fontFamily
                            font.pixelSize: menu.fontSize
                        }

                        Text {
                            width: parent.width
                            visible: text !== ""
                            text: row.unavailable === "" ? row.subtitle : row.unavailable
                            elide: Text.ElideRight
                            color: menu.dimmed
                            font.family: menu.fontFamily
                            font.pixelSize: menu.fontSize - 3
                        }
                    }

                    MouseArea {
                        id: area
                        anchors.fill: parent
                        hoverEnabled: true
                        onEntered: list.hover(row.index, area, area.mouseX, area.mouseY)
                        onPositionChanged: mouse => list.hover(row.index, area, mouse.x, mouse.y)
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
                                // The menu has no picker: a file or folder field is typed into as a text field is.
                                readonly property bool typedIn: modelData.kind === "text" || modelData.kind === "file" || modelData.kind === "folder"

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
                                        visible: formField.typedIn
                                        verticalAlignment: TextInput.AlignVCenter
                                        clip: true
                                        color: menu.foreground
                                        selectionColor: menu.selection
                                        font.family: menu.fontFamily
                                        font.pixelSize: menu.fontSize
                                        echoMode: formField.modelData.secret ? TextInput.Password : TextInput.Normal
                                        Component.onCompleted: {
                                            if (formField.typedIn)
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

        // The footer: what the user should know now, else the keys the
        // selected row takes; and on the right, while the Daemon knows of
        // any, how many problems there are, which a click lists.
        Item {
            id: footer
            anchors.top: body.bottom
            width: parent.width
            height: footerText.text === "" && search.problems === 0 ? 0 : menu.rowHeight * 0.7

            Text {
                id: footerText
                anchors.left: parent.left
                anchors.right: problems.left
                height: parent.height
                verticalAlignment: Text.AlignVCenter
                leftPadding: 18
                rightPadding: 18
                elide: Text.ElideRight
                color: search.confirming !== "" || menu.failure !== "" ? menu.error : menu.dimmed
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize - 3
                // Hints are all the footer shows, and the query chose what
                // Enter runs.
                readonly property bool chosen: search.confirming === "" && menu.failure === "" && search.down.length === 0 && menu.chosenHints !== ""
                textFormat: chosen ? Text.StyledText : Text.AutoText
                text: search.confirming !== "" ? search.confirming : menu.failure !== "" ? menu.failure : search.down.length > 0 ? "Unavailable: " + search.down.join(", ") : chosen ? menu.chosenHints : menu.hints
            }

            Text {
                id: problems
                anchors.right: parent.right
                anchors.rightMargin: visible ? 18 : 0
                height: parent.height
                verticalAlignment: Text.AlignVCenter
                width: visible ? implicitWidth : 0
                visible: search.problems > 0
                text: search.problems === 1 ? "1 problem" : search.problems + " problems"
                color: menu.error
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize - 3

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: search.showProblems()
                }
            }
        }

        // The Toast: what Forget or "Clear history" just did, and the key that
        // undoes it. It never takes the keys or the selection, and goes when
        // the Daemon withdraws it, once the change is undone or too old to be.
        Item {
            id: toast
            anchors.top: footer.bottom
            width: parent.width
            height: search.toast === "" ? 0 : menu.rowHeight * 0.7
            visible: search.toast !== ""

            Text {
                anchors.left: parent.left
                anchors.leftMargin: 18
                anchors.right: undoHint.left
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                text: search.toast
                color: menu.foreground
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize - 3
            }

            Text {
                id: undoHint
                anchors.right: parent.right
                anchors.rightMargin: 18
                anchors.verticalCenter: parent.verticalCenter
                text: "Ctrl+Z  Undo"
                color: menu.accent
                font.family: menu.fontFamily
                font.pixelSize: menu.fontSize - 3
            }
        }

        // The Action panel: Ctrl+K lists the selected Result's Actions, the
        // one Enter runs and the Item's own first, then the Contributed ones
        // under the Extension that offers each, then the destructive ones in
        // red, with a line between each part. Up and Down choose, Enter runs,
        // Esc or Ctrl+K closes it.
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
                const ctrl = menu.ctrlOnly(event);
                // Undo may move the Result the panel is for.
                if (menu.undoKey(event)) {
                    menu.closeActions();
                } else if (event.key === Qt.Key_Escape || (ctrl && event.key === Qt.Key_K)) {
                    menu.closeActions();
                } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N)) {
                    actionList.current = Math.min(menu.actions.length - 1, actionList.current + 1);
                } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P)) {
                    actionList.current = Math.max(0, actionList.current - 1);
                } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    menu.failure = "";
                    // By what the panel shows, not by its place: the Result
                    // may have been sent again with its Actions reordered.
                    const shown = menu.actions[actionList.current];
                    search.runListed(actionList.row, shown.group, shown.id);
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
                        readonly property int section: menu.panelSection(index)
                        readonly property bool divided: index > 0 && menu.panelSection(index - 1) !== section
                        // The destructive part names no Extension: its red
                        // sets it apart, whoever offers each.
                        readonly property bool heading: modelData.group !== "" && section !== 2 && (index === 0 || divided || menu.actions[index - 1].group !== modelData.group)

                        Item {
                            visible: entry.divided
                            width: parent.width
                            height: visible ? 9 : 0

                            Rectangle {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.leftMargin: 12
                                anchors.rightMargin: 12
                                anchors.verticalCenter: parent.verticalCenter
                                height: 1
                                color: menu.border
                            }
                        }

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
                                text: menu.actionTitle(entry.modelData.title, entry.modelData.role)
                                color: entry.modelData.role === "destructive" ? menu.error : menu.foreground
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
