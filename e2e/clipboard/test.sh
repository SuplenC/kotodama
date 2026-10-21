# shellcheck shell=bash
# The clipboard history watches the real clipboard: what is copied becomes a
# Result, what a password manager copies never does, an image keeps a
# thumbnail, and the primary Action pastes into the focused window.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama -p kotodama-clipboard \
    --bin kotodamad --bin kotodama --bin kotodama-clipboard --example front-end --example copy
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

# Installed as a package installs it: the binary next to its Manifest.
ext="$XDG_DATA_DIRS/kotodama/extensions/clipboard"
mkdir -p "$ext"
cp "$root/crates/kotodama-clipboard/manifest.toml" "$ext/"
ln -s "$target/debug/kotodama-clipboard" "$ext/kotodama-clipboard"

front_end="$target/debug/examples/front-end"
copy="$target/debug/examples/copy"

# results <query>: the clipboard's Results for a query in its app, where
# what was copied is, one per line.
results() {
    "$front_end" --app "clipboard $1" 2>/dev/null | awk -F'\t' '$1 == "clipboard" { print $3 }'
}
# wait_for_result <query> <title>: waits until the query finds the title.
wait_for_result() {
    for _ in $(seq 50); do
        if results "$1" | grep -qxF -- "$2"; then
            return 0
        fi
        sleep 0.1
    done
    echo "no Result \"$2\" for \"$1\"; got: $(results "$1")" >&2
    return 1
}
# copy_as <name> <mime>...: copies stdin as the given types, and keeps
# serving it in the background, as an app that copied would. Returns once it
# is on the clipboard, so the next copy comes after it.
copy_as() {
    local name=$1 mark
    shift
    cat >"$home/$name"
    mark=$(log_lines copy.log)
    spawn "$copy" "$home/$name" "$@"
    wait_for_log copy.log "copy: copied" "$mark"
}

# At debug, the Daemon logs each copy once the compositor has it.
KOTODAMA_LOG=debug spawn "$target/debug/kotodamad"
wait_for_result "clipboard history" "Clipboard History"

# Something copied as plain text shows up.
printf 'first copy' | copy_as first text/plain\;charset=utf-8 text/plain
wait_for_result "first copy" "first copy"

# A password manager's copy is never kept, not even on disk. The copy after
# it shows the watcher has seen it and moved on.
printf 'hunter2-secret' | copy_as secret text/plain\;charset=utf-8 x-kde-passwordManagerHint
printf 'after the secret' | copy_as after text/plain\;charset=utf-8
wait_for_result "after the secret" "after the secret"
if [ -n "$(results hunter2)" ]; then
    echo "the password manager's copy was kept: $(results hunter2)" >&2
    exit 1
fi
if grep -rq hunter2 "$XDG_STATE_HOME"; then
    echo "the password manager's copy is on disk" >&2
    exit 1
fi

# An image keeps a thumbnail, which is its icon and its detail View's picture.
magick -size 320x200 xc:'#1e90ff' png:- | copy_as image image/png
wait_for_result "image 320" "Image, 320×200"
thumbnail=$(find "$XDG_STATE_HOME/kotodama/clipboard/thumbs" -name '*.png' | head -n1)
[ "$(magick identify -format '%wx%h' "$thumbnail")" = 256x160 ]

# In the menu, Ctrl+K lists the image's Actions, and Show Details pushes its
# detail View, with the thumbnail.
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
mark=$(log_lines quickshell.log)
"$target/debug/kotodama" open --prefix clipboard --query "image 320"
wait_for_log quickshell.log "focused: true" "$mark"
press -M ctrl -k k -m ctrl "actions: Paste, Copy to Clipboard, Show Details, Delete from History"
press -k Down -k Down -k Return "view: detail Copied Image"
expect_screen detail.png
press -k Escape "view: search"
# The first Esc leaves the clipboard's app, the next closes the menu.
press -k Escape -k Escape "visible: false"

# The primary Action pastes into the window that has focus.
spawn quickshell --path ../type-and-paste/client.qml
wait_for_log quickshell.log "ready"
"$front_end" --app --run "clipboard first copy"
wait_for_log quickshell.log "typed: first copy"

# Copy to Clipboard hands the clip to the Daemon, which sets the clipboard:
# the next Ctrl+V pastes it.
mark=$(log_lines quickshell.log)
"$target/debug/kotodama" open --prefix clipboard --query "after the secret"
wait_for_log quickshell.log "focused: true" "$mark"
press -M ctrl -k k -m ctrl "actions: Paste, Copy to Clipboard, Show Details, Delete from History"
copied=$(log_lines kotodamad.log)
press -k Down -k Return "visible: false"
wait_for_log kotodamad.log "copied to the clipboard" "$copied"
wtype -M ctrl v -m ctrl
wait_for_log quickshell.log "typed: first copyafter the secret"
