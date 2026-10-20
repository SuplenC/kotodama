// The default menu, with logs of its Views, jobs and Action panel for the test
// to wait on, and a stand-in for the desktop's notification service that logs
// what it is sent.
import QtQuick
import Quickshell
import Quickshell.Services.Notifications
import Kotodama

ShellRoot {
    NotificationServer {
        onNotification: notification => console.log("notification: " + notification.summary + ": " + notification.body)
    }

    KotodamaMenu {
        id: menu
        menuWidth: 560
        top: 40
        maxRows: 6
        fontFamily: "DejaVu Sans"

        onFocusedChanged: console.log("focused: " + focused)
        onVisibleChanged: {
            if (visible) {
                const view = menu.search.view;
                console.log("opened on: " + (view.kind ?? "search") + " " + (view.title ?? ""));
                for (const job of menu.search.jobs)
                    console.log("opened with job: " + job.title);
            } else {
                console.log("hidden");
            }
        }
        onActionsChanged: {
            if (menu.actions.length > 0)
                console.log("actions: " + menu.actions.map(action => (action.group !== "" ? action.group + "/" : "") + action.title).join(", "));
            else
                console.log("actions closed");
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
            function onRowsInserted() {
                console.log("rows: " + menu.search.rowCount());
            }
            function onRowsRemoved() {
                console.log("rows: " + menu.search.rowCount());
            }
            function onViewChanged() {
                const view = menu.search.view;
                let line = "view: " + (view.kind ?? "search") + " " + (view.title ?? "");
                if (view.kind === "progress")
                    line += " " + view.job.state + " " + Math.round(view.job.progress * 100) + "%";
                console.log(line);
            }
        }
    }
}
