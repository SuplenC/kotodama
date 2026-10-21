# shellcheck shell=bash
# The menu has the keys from the moment it opens, and closes once they go
# elsewhere, as Spotlight does: when a keybind opens a window, or a click lands
# outside it. A click inside keeps it open with the keys.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

# Ctrl+Return opens a window that logs what is typed into it, as a desktop's
# bind opens a terminal. sway starts it, so the test keeps its pid to stop it,
# and passes it the Qt settings sway's own environment lacks.
window="$home/window"
cat >"$window" <<SH
#!/bin/sh
echo \$\$ >"$home/window.pid"
export QT_QPA_PLATFORM=$QT_QPA_PLATFORM QT_QUICK_BACKEND=$QT_QUICK_BACKEND
exec quickshell --path "$root/e2e/type-and-paste/client.qml" >>"$E2E_OUT/window.log" 2>&1
SH
chmod +x "$window"
swaymsg -q "bindsym ctrl+Return exec $window"
# The next test starts with neither the bind nor the window.
close_window() {
    if [ -f "$home/window.pid" ]; then
        kill "$(cat "$home/window.pid")" 2>/dev/null || true
        rm "$home/window.pid"
    fi
    for _ in $(seq 50); do
        if swaymsg -t get_tree | jq -e '[recurse(.nodes[]?, .floating_nodes[]?) | select(.pid? and .pid > 0)] | length == 0' >/dev/null; then
            return 0
        fi
        sleep 0.1
    done
    echo "the window never closed" >&2
    return 1
}
trap 'swaymsg -q "unbindsym ctrl+Return" || true; close_window || true; stop_spawned' EXIT

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
# open: opens the menu and waits until keys reach it.
open() {
    local mark
    mark=$(log_lines quickshell.log)
    "$kotodama" open
    wait_for_log quickshell.log "shown" "$mark"
    wait_for_log quickshell.log "focused: true" "$mark"
}
# keys <log> <wtype args>: sends keys, then waits for the log line given last.
keys() {
    local log=$1 mark
    shift
    mark=$(log_lines "$log")
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log "$log" "${!#}" "$mark"
}
# click <x> <y>: the left button, at a point on the output. Setting the cursor
# moves no focus, so only the click does.
click() {
    swaymsg -q "seat seat0 cursor set $1 $2"
    swaymsg -q "seat seat0 cursor press button1"
    swaymsg -q "seat seat0 cursor release button1"
}

# Keys typed the moment it opens all reach the menu, so it needs no click.
open
keys quickshell.log ab "query: ab"

# A bind opens a window: the menu closes and the window has the keys.
mark=$(log_lines quickshell.log)
keys window.log -M ctrl -k Return -m ctrl "ready"
wait_for_log quickshell.log "hidden" "$mark"
keys window.log xy "typed: xy"

# A click inside the menu keeps it open, with the keys; one outside it, on
# the window, closes it and gives the window the keys. The menu is 560 wide
# and centred on the 640 wide output, from 40 down.
open
click 320 60
keys quickshell.log c "query: c"
mark=$(log_lines quickshell.log)
click 10 240
wait_for_log quickshell.log "hidden" "$mark"
keys window.log z "typed: xyz"

# Opened over a focused window, it has the keys too.
open
keys quickshell.log d "query: d"
