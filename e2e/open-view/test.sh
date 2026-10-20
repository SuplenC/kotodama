# shellcheck shell=bash
# `kotodama open --view <extension>/<item>` opens the menu on the View the
# Item's primary Action pushes, as a SUPER+V keybind opens clipboard history:
# the View is there in the first frame, never the search first, and Esc pops
# it to the search. A name with no View behind it opens the search and says so.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama -p kotodama-clipboard \
    --bin kotodamad --bin kotodama --bin kotodama-clipboard
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state"

# Installed as a package installs it: the binary next to its Manifest.
ext="$XDG_DATA_DIRS/kotodama/extensions/clipboard"
mkdir -p "$ext"
cp "$root/crates/kotodama-clipboard/manifest.toml" "$ext/"
ln -s "$target/debug/kotodama-clipboard" "$ext/kotodama-clipboard"

kotodama="$target/debug/kotodama"
spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}

# open_on <view> <expected>: runs `kotodama open --view <view>` and waits
# until the menu has opened on <expected> and has the keys. It fails if the
# menu opened on anything else first.
open_on() {
    local mark opened
    mark=$(log_lines quickshell.log)
    "$kotodama" open --view "$1"
    wait_for_log quickshell.log "opened on: $2" "$mark"
    wait_for_log quickshell.log "focused: true" "$mark"
    opened=$(tail -n "+$((mark + 1))" "$E2E_OUT/quickshell.log" | grep -F "opened on: " | head -n1)
    if [[ "$opened" != *"opened on: $2"* ]]; then
        echo "the menu's first frame showed \"${opened#*opened on: }\", not \"$2\"" >&2
        return 1
    fi
}

# Right after the Daemon starts, as under socket activation, the clipboard
# Extension may still be starting: the menu waits for its View.
open_on clipboard/history "list Clipboard History"
# Esc pops the View to the search, and the menu stays open on it.
press -k Escape "view: search"
press -k Escape "visible: false"

# Again, from a warm Daemon.
open_on clipboard/history "list Clipboard History"
press -k Escape "view: search"
press -k Escape "visible: false"

# A name with no View behind it opens the search, and says why.
mark=$(log_lines quickshell.log)
open_on clipboard/nothing "search"
wait_for_log quickshell.log "failure: " "$mark"
press -k Escape "visible: false"
