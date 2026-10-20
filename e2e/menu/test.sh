# shellcheck shell=bash
# `kotodama open` shows the menu, typing finds Results, and Enter runs one:
# the menu closes before the Daemon types into the window underneath.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

ext="$XDG_CONFIG_HOME/kotodama/extensions/apps"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "apps"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
cat >"$ext/items.json" <<'JSON'
[
  {"id":"firefox","kind":"url","url":"https://firefox","title":"Firefox","subtitle":"Web browser","actions":[{"id":"go","title":"Go"}]},
  {"id":"files","kind":"url","url":"https://files","title":"Files","subtitle":"File manager","actions":[{"id":"go","title":"Go"}]},
  {"id":"fish","kind":"url","url":"https://fish","title":"Fish","actions":[{"id":"go","title":"Go"}]},
  {"id":"snippet","kind":"url","url":"https://snippet","title":"Snippet","actions":[{"id":"go","title":"Go"}]}
]
JSON
printf '{"type":{"text":"Hello, Wayland!"}}' >"$ext/outcome.json"

# A Live query whose Result comes after the Results it is asked with.
notes="$XDG_CONFIG_HOME/kotodama/extensions/notes"
mkdir -p "$notes"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$notes/"
printf 'name = "notes"\ncommand = ["extension.sh"]\n[live_query]\ntriggers = ["^zeb"]\ndebounce_ms = 20\n' >"$notes/manifest.toml"
cat >"$notes/live.json" <<'JSON'
[{"item":{"id":"zebra","kind":"url","url":"https://zebra","title":"Zebra crossing","actions":[{"id":"go","title":"Go"}]},"score":1.0,"answer":false}]
JSON
printf '{"type":{"text":"From notes"}}' >"$notes/outcome.json"

# The window the user was in before opening the menu.
spawn quickshell --path ../type-and-paste/client.qml
wait_for_log quickshell.log "ready"

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
# close: closes the open menu and waits until it lets go of the keys.
close() {
    local mark
    mark=$(log_lines quickshell.log)
    "$kotodama" toggle
    wait_for_log quickshell.log "focused: false" "$mark"
}

# The Extension may still be sending its Items; Snippet is the last one.
open --query snippet
close

# wtype's first key is lost if it arrives before the compositor has taken
# wtype's keymap, so it waits a moment first.
open
wtype -s 200 "fi"
expect_screen reference.png
close

open --query snip
wtype -s 200 -k Return
wait_for_log quickshell.log "typed: Hello, Wayland!"

# A late Result runs once a frame has shown it: the menu confirms the Results
# on screen, and the Daemon refuses one the user hasn't seen.
mark=$(log_lines quickshell.log)
open --query zebra
wait_for_log quickshell.log "top: Zebra crossing" "$mark"
wtype -s 200 -k Return
wait_for_log quickshell.log "typed: Hello, Wayland!From notes"
