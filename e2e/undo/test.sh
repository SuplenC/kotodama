# shellcheck shell=bash
# Forget, from its key in the search, drops a picked Result to where it was
# before the pick, and shows a Toast that keeps the keys and the selection
# where they were. The Toast's key, Ctrl+Z, undoes it: the Result is back in
# the same place, and the Toast goes.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

ext="$XDG_CONFIG_HOME/kotodama/extensions/notes"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "notes"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
# Two notes that match "note" equally well: until one is picked, Note A ranks
# first. Each Action names its note, so the Action panel tells which is first.
cat >"$ext/items.json" <<'JSON'
[
  {"id":"a","kind":"notes.note","title":"Note A","actions":[{"id":"open","title":"Open A"}]},
  {"id":"b","kind":"notes.note","title":"Note B","actions":[{"id":"open","title":"Open B"}]}
]
JSON
echo '"keep_open"' >"$ext/outcome.json"

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
mark=$(log_lines quickshell.log)
"$kotodama" open --query note
wait_for_log quickshell.log "focused: true" "$mark"
wait_for_log quickshell.log "rows: 2" "$mark"

# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}
# first <action>: the first row's Action panel opens with `action` first.
first() {
    press -M ctrl -k k -m ctrl "actions: $1"
    press -k Escape "actions closed"
}
# search_again: types the last letter of "note" again, so the Daemon ranks
# the Results afresh rather than keep the rows on screen where they are.
search_again() {
    press -k BackSpace "query: not"
    press e "query: note"
}

first "Open A"

# Picking Note B ranks it first. Its Action keeps the menu open.
wtype -s 200 -k Down -k Return
search_again
first "Open B, kotodama/Forget"

# Forget's key drops it back below Note A, and the Toast offers to undo.
press -M ctrl -M shift -k BackSpace -m shift -m ctrl "toast: Forgot"
# The rows on screen and the selection stay where they were until the next
# search, and the keys stay with the search box.
first "Open B"
search_again
first "Open A"

# Ctrl+Z undoes it: Note B is back in the same place, and the Toast goes. The
# search box keeps its text: the key undid the Forget, not the typing.
press -M ctrl -k z -m ctrl "toast: none"
search_again
first "Open B, kotodama/Forget"
