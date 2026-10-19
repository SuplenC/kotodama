# shellcheck shell=bash
# The shell example's Timer, from the keyboard alone: an Argument typed after
# its keyword, the form the Daemon opens for a missing one, the Action panel,
# the Extension's own form, a progress View that updates, Esc cancelling a job
# that takes it as "cancel", the same job running on after the menu closes,
# and the desktop notification when it is done.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state"

extensions="$XDG_CONFIG_HOME/kotodama/extensions"
mkdir -p "$extensions/shell-example" "$extensions/extras"
cp "$root"/examples/shell-extension/* "$extensions/shell-example/"
# Another Extension's Action on the timer, which the Action panel lists after
# the timer's own.
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$extensions/extras/"
cat >"$extensions/extras/manifest.toml" <<'TOML'
name = "extras"
command = ["extension.sh"]

[[contributed_actions]]
kind = "shell-example.timer"
id = "share"
title = "Share timer"
TOML

# A session bus of our own, where the shell below stands in for the desktop's
# notification service.
export DBUS_SESSION_BUS_ADDRESS="unix:path=$home/bus"
dbus=$(dirname "$(command -v dbus-daemon)")/..
spawn dbus-daemon --config-file="$dbus/share/dbus-1/session.conf" --nofork --address="$DBUS_SESSION_BUS_ADDRESS"
for _ in $(seq 50); do
    [ -S "$home/bus" ] && break
    sleep 0.1
done

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
# open <args>: opens the menu and waits until keys reach it.
open() {
    local mark
    mark=$(log_lines quickshell.log)
    "$kotodama" open "$@"
    wait_for_log quickshell.log "focused: true" "$mark"
}
# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}

# `timer 1` passes the Argument: Enter starts a one second timer, whose
# progress View shows it running, then done.
mark=$(log_lines quickshell.log)
open --query "timer 1"
wait_for_log quickshell.log "rows: 1" "$mark"
mark=$(log_lines quickshell.log)
press -k Return "view: progress 1 second timer running 0%"
wait_for_log quickshell.log "view: progress 1 second timer succeeded 100%" "$mark"
wait_for_log quickshell.log "notification: 1 second timer: Time is up" "$mark"

# Without the Argument, the Daemon asks for it in a form; submitting it runs
# the Action with what was typed.
press -k Escape "view: search"
# `timer 1` becomes `timer`.
wtype -s 200 -k BackSpace -k BackSpace
press -k Return "view: form Start timer"
press 2 -k Return "view: progress 2 second timer running"

# Esc leaves the progress View, which the timer takes as "cancel".
mark=$(log_lines quickshell.log)
press -k Escape "view: search"
wait_for_log quickshell.log "notification: 2 second timer: Cancelled" "$mark"

# Ctrl+K lists the timer's own Actions, then the one extras contributes.
press -M ctrl -k k -m ctrl "actions: Start timer, Start a labelled timer, extras/Share timer"
# The second runs from the panel, and pushes the Extension's own form. Tab
# moves to the seconds, filled in with 3, which become 5.
press -k Down -k Return "view: form Labelled timer"
mark=$(log_lines quickshell.log)
press Tea -k Tab -k BackSpace 5 -k Return "view: progress Tea running 0%"
wait_for_log quickshell.log "view: progress Tea running 20%" "$mark"

# Closing the menu isn't Esc: the timer isn't told, so it runs on, and the
# menu opens again on its progress View.
mark=$(log_lines quickshell.log)
"$kotodama" toggle
wait_for_log quickshell.log "focused: false" "$mark"
mark=$(log_lines quickshell.log)
open --query timer
wait_for_log quickshell.log "opened on: progress Tea" "$mark"
wait_for_log quickshell.log "opened with job: Tea" "$mark"

# It ends, and the Daemon tells the desktop.
wait_for_log quickshell.log "notification: Tea: Time is up" "$mark"

# An Action's key binding runs it: Ctrl+L, the labelled timer.
press -k Escape "view: search"
press -M ctrl -k l -m ctrl "view: form Labelled timer"
press -k Escape "view: search"
