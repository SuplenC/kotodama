# shellcheck shell=bash
# A pointer resting where the menu opens leaves the selection on the top row
# while the rows change under it, so Enter runs the top row; moving the pointer
# onto a row selects it.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

# Headless sway has no pointer of its own: this one gives the seat one for the
# test, which `swaymsg seat - cursor` then moves.
rustc --edition 2024 -O -o "$E2E_OUT/virtual-pointer" virtual-pointer.rs
spawn "$E2E_OUT/virtual-pointer"
wait_for_log virtual-pointer.log "pointer ready"
has_pointer() {
    # 1 is WL_SEAT_CAPABILITY_POINTER.
    swaymsg -t get_seats | jq -e 'any(.[]; .capabilities % 2 == 1)' >/dev/null
}
for _ in $(seq 50); do
    if has_pointer; then
        break
    fi
    sleep 0.1
done
has_pointer

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

# Items whose ids are their titles in lowercase, so the rows' titles name them.
ext="$XDG_CONFIG_HOME/kotodama/extensions/apps"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "apps"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
items=""
for title in Firefox Files Fish Fig Fax Ferry Forum Fern; do
    id=${title,,}
    items+="${items:+,}{\"id\":\"$id\",\"kind\":\"url\",\"url\":\"https://$id\",\"title\":\"$title\",\"actions\":[{\"id\":\"go\",\"title\":\"Go\"}]}"
done
printf '[%s]' "$items" >"$ext/items.json"

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
# row <n>: the id of the menu's nth row, from the rows it last logged.
row() {
    local titles
    titles=$(grep -F "rows: " "$E2E_OUT/quickshell.log" | tail -n1)
    titles=${titles#*rows: }
    titles=${titles,,}
    cut -d' ' -f"$1" <<<"$titles"
}
# rows_are <mark> <ids>: waits until the rows the menu last logged, past line
# <mark> of its log, are the Items <ids>, in any order, sorted and
# space-separated.
rows_are() {
    local shown=""
    for _ in $(seq 50); do
        shown=$(tail -n "+$(($1 + 1))" "$E2E_OUT/quickshell.log" | grep -F "rows: " | tail -n1)
        shown=${shown#*rows: }
        if [ "$(tr ' ' '\n' <<<"${shown,,}" | sort | xargs)" = "$2" ]; then
            return 0
        fi
        sleep 0.2
    done
    echo "the rows are $shown, never $2" >&2
    return 1
}
# ran <id>: waits until the Extension is asked to run an Item, and fails if
# the Item is another.
ran() {
    local last=""
    for _ in $(seq 50); do
        last=$(jq -r 'select(.run_action) | .run_action.item.id' "$ext/received" 2>/dev/null | tail -n1)
        if [ -n "$last" ]; then
            break
        fi
        sleep 0.2
    done
    if [ "$last" != "$1" ]; then
        echo "ran ${last:-nothing}, not $1" >&2
        return 1
    fi
}
# enter_runs <id>: presses Enter, checks it ran the Item <id>, and waits until
# the menu has closed on the Action's outcome. The Extension is asked to run
# the Item before the menu closes; a menu asked to open in between takes the
# query and is then closed.
enter_runs() {
    local mark
    mark=$(log_lines quickshell.log)
    : >"$ext/received"
    wtype -k Return
    ran "$1"
    wait_for_log quickshell.log "focused: false" "$mark"
}
# pointer_moves <y>: moves the pointer <y> pixels down and waits until the
# menu has had the move. By 0, the menu gets a pointer-enter at the resting
# pointer, as it would on mapping under it: Sway sends a layer surface that
# maps under the pointer none, so a move by nothing makes it send one.
pointer_moves() {
    local mark
    mark=$(log_lines quickshell.log)
    swaymsg seat - cursor move 0 "$1" >/dev/null
    wait_for_log quickshell.log "pointer: " "$mark"
}

fi_rows="fig files firefox fish"

# The Extension may still be sending its Items; Fern is the last one.
open --query fern
close

# Over the menu's third row: the menu spans x 40 to 600 on the 640-wide
# output, and its rows start under the 56-pixel search box below the 40-pixel
# top margin, 44 pixels each.
swaymsg seat - cursor set 320 206 >/dev/null

# The pointer enters the menu over the third row once it shows Results.
# wtype's first key is lost if it arrives before the compositor has taken
# wtype's keymap, so it waits a moment first.
open
mark=$(log_lines quickshell.log)
wtype -s 200 "fi"
rows_are "$mark" "$fi_rows"
pointer_moves 0
enter_runs "$(row 1)"

# Rows appear under the resting pointer: Fig alone, then Backspace brings
# back the rest.
mark=$(log_lines quickshell.log)
open --query fig
rows_are "$mark" fig
pointer_moves 0
mark=$(log_lines quickshell.log)
wtype -s 200 -k BackSpace
rows_are "$mark" "$fi_rows"
enter_runs "$(row 1)"

# Once the pointer is in the menu, moving it one row down selects the row it
# lands on. Opened on the query it closed on, the menu would log no rows to
# wait for, so the query is typed again.
open
mark=$(log_lines quickshell.log)
wtype -s 200 "fi"
rows_are "$mark" "$fi_rows"
pointer_moves 0
fourth=$(row 4)
pointer_moves 44
enter_runs "$fourth"
