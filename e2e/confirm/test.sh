# shellcheck shell=bash
# An Action that asks first runs on a second Enter only: the first one asks
# in the footer, Esc drops the question and keeps the menu open, and typing
# drops it too.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

ext="$XDG_CONFIG_HOME/kotodama/extensions/vault"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "vault"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
cat >"$ext/items.json" <<'JSON'
[
  {"id":"wipe","kind":"vault.secret","title":"Wipe Secrets","actions":[{"id":"wipe","title":"Wipe Secrets","confirm":true}]}
]
JSON

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
mark=$(log_lines quickshell.log)
"$kotodama" open --query wipe
wait_for_log quickshell.log "focused: true" "$mark"
wait_for_log quickshell.log "rows: Wipe Secrets" "$mark"

# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}
# runs: how many times the Extension was asked to run an Action.
runs() {
    jq -s 'map(select(.run_action)) | length' "$ext/received"
}
# ran <n>: fails unless the Extension was asked to run an Action n times.
ran() {
    local count
    for _ in $(seq 25); do
        count=$(runs)
        if [ "$count" = "$1" ]; then
            break
        fi
        sleep 0.2
    done
    if [ "$count" != "$1" ]; then
        echo "ran $count times, not $1" >&2
        return 1
    fi
}

# Enter asks; nothing runs.
press -k Return "confirming: Wipe Secrets? ↵ Confirm · Esc Cancel"
sleep 0.5
ran 0

# Esc drops the question and leaves the query and the menu as they were.
press -k Escape "confirming: none"
sleep 0.5
ran 0
if grep -q "visible: false" "$E2E_OUT/quickshell.log"; then
    echo "Esc closed the menu rather than drop the question" >&2
    exit 1
fi

# Typing drops it too.
press -k Return "confirming: Wipe Secrets? ↵ Confirm · Esc Cancel"
press s "confirming: none"
press -k BackSpace "query: wipe"
sleep 0.5
ran 0

# Enter twice runs it, once.
press -k Return "confirming: Wipe Secrets? ↵ Confirm · Esc Cancel"
press -k Return "confirming: none"
ran 1
