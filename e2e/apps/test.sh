# shellcheck shell=bash
# Every Extension is an app. The search shows an entry Item for each, and Tab
# or Enter on it enters the app: a chip names it, its intro card shows before
# anything is typed, and the query searches its Items alone. Esc goes back to
# the whole search, as Backspace on the empty box does. The emoji are in their
# app alone; the calculator answers in both.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama -p kotodama-folders \
    -p kotodama-calculator -p kotodama-emoji --bin kotodamad --bin kotodama --bin kotodama-folders \
    --bin kotodama-calculator --bin kotodama-emoji
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

# Installed as a package installs them: each binary next to its Manifest.
for name in folders calculator emoji; do
    ext="$XDG_DATA_DIRS/kotodama/extensions/$name"
    mkdir -p "$ext"
    cp "$root/crates/kotodama-$name/manifest.toml" "$ext/"
    ln -s "$target/debug/kotodama-$name" "$ext/kotodama-$name"
done

# Two projects for the folders Extension to find, under a home of the test's
# own, so the reference image shows them at the same `~/Projects` each run.
for project in alpha beta; do
    git init --quiet "$home/Projects/$project"
done
mkdir -p "$XDG_CONFIG_HOME/kotodama"
cat >"$XDG_CONFIG_HOME/kotodama/config.toml" <<TOML
[extensions.folders]
folders = [{ path = "~/Projects", kind = "projects", depth = 2 }]
TOML

HOME=$home spawn "$target/debug/kotodamad"
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
# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}
# last_rows: the rows the menu logged last.
last_rows() {
    grep -F "rows: " "$E2E_OUT/quickshell.log" | tail -n1
}

# `projects` and Tab: the Projects app, with its intro card over the projects,
# in the first frame in it. The projects may still be on their way.
open
press "projects" "top: Projects"
mark=$(log_lines quickshell.log)
press -k Tab "scope: [Projects]"
wait_for_log quickshell.log "intro: [The projects and scripts" "$mark"
wait_for_log quickshell.log "query: []" "$mark"
wait_for_log quickshell.log "rows: [folders, folders]" "$mark"
expect_screen reference.png

# Typing filters the projects, and hides the intro card.
mark=$(log_lines quickshell.log)
press "alp" "rows: [folders]"
wait_for_log quickshell.log "intro: []" "$mark"

# Backspace on the empty box leaves the app, with its word typed: the entry
# Item comes first, and Enter on it enters the app again.
press -k BackSpace -k BackSpace -k BackSpace "query: []"
mark=$(log_lines quickshell.log)
press -k BackSpace "scope: []"
wait_for_log quickshell.log "query: [projects]" "$mark"
wait_for_log quickshell.log "top: Projects" "$mark"
mark=$(log_lines quickshell.log)
press -k Return "scope: [Projects]"
wait_for_log quickshell.log "query: []" "$mark"

# Esc clears what is typed first, and stays in the app.
press "alp" "query: [alp]"
mark=$(log_lines quickshell.log)
press -k Escape "query: []"
if grep -F "scope: []" <(tail -n +"$((mark + 1))" "$E2E_OUT/quickshell.log"); then
    echo "Esc with text typed left the app" >&2
    exit 1
fi

# With nothing typed, Esc goes back to the whole search, with the menu open:
# the Empty query, whose most used is the Projects app just entered.
mark=$(log_lines quickshell.log)
press -k Escape "scope: []"
wait_for_log quickshell.log "top: Projects" "$mark"
if grep -F "focused: false" <(tail -n +"$((mark + 1))" "$E2E_OUT/quickshell.log"); then
    echo "Esc in the app closed the menu" >&2
    exit 1
fi

# In the search too, Esc clears the text before it closes the menu.
press "alp" "query: [alp]"
mark=$(log_lines quickshell.log)
press -k Escape "query: []"
if grep -F "focused: false" <(tail -n +"$((mark + 1))" "$E2E_OUT/quickshell.log"); then
    echo "Esc with text typed closed the menu" >&2
    exit 1
fi
press -k Escape "focused: false"

# The emoji are in their app, which `kotodama open --app` opens on, and not
# in the search.
mark=$(log_lines quickshell.log)
open --app emoji --query fire
wait_for_log quickshell.log "scope: [Emoji]" "$mark"
wait_for_log quickshell.log "rows: [emoji" "$mark"
close
open
press "fire" "query: [fire]"
press -k space "query: [fire ]"
if [[ "$(last_rows)" == *emoji* ]]; then
    echo "the search showed the emoji: $(last_rows)" >&2
    exit 1
fi
close

# The calculator answers `15min in sec` in the search, and in its app, which
# Tab on its entry Item enters.
open
press "15min in sec" "top: 900 secs"
close
open
press "calc" "top: Calculator"
press -k Tab "scope: [Calculator]"
press "15min in sec" "top: 900 secs"
if [[ "$(last_rows)" != *"rows: [calculator]" ]]; then
    echo "the calculator's app showed $(last_rows)" >&2
    exit 1
fi
close
