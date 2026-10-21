# shellcheck shell=bash
# The Daemon and the clipboard Extension start before the compositor is
# where they look for it, as under socket activation before the compositor's
# environment reaches the service manager: no WAYLAND_DISPLAY, and no socket
# in their runtime directory yet. Once it is there, without a restart, the
# clipboard history records what is copied and the Daemon pastes.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama-clipboard \
    --bin kotodamad --bin kotodama-clipboard --example front-end --example copy

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state"

ext="$XDG_DATA_DIRS/kotodama/extensions/clipboard"
mkdir -p "$ext"
cp "$root/crates/kotodama-clipboard/manifest.toml" "$ext/"
ln -s "$target/debug/kotodama-clipboard" "$ext/kotodama-clipboard"

# The Daemon's own runtime directory, where the compositor's socket is not,
# yet.
runtime="$home/runtime"
mkdir -m 700 "$runtime"
WAYLAND_DISPLAY='' XDG_RUNTIME_DIR="$runtime" spawn "$target/debug/kotodamad"

front_end() {
    XDG_RUNTIME_DIR="$runtime" "$target/debug/examples/front-end" "$@"
}
# wait_for_result <query> <title>: waits until the query finds the title in
# the clipboard's app.
wait_for_result() {
    for _ in $(seq 50); do
        if front_end --app "clipboard $1" 2>/dev/null | awk -F'\t' '$1 == "clipboard" { print $3 }' | grep -qxF -- "$2"; then
            return 0
        fi
        sleep 0.1
    done
    echo "no Result \"$2\" for \"$1\"" >&2
    return 1
}

wait_for_result "clipboard history" "Clipboard History"
wait_for_log kotodamad.log "waiting for the compositor"

# The compositor's socket turns up in the Daemon's runtime directory.
mark=$(log_lines kotodamad.log)
ln -s "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" "$runtime/wayland-1"
wait_for_log kotodamad.log "watching the clipboard" "$mark"

printf 'copied late' >"$home/late"
spawn "$target/debug/examples/copy" "$home/late" text/plain\;charset=utf-8
wait_for_log copy.log "copy: copied"
wait_for_result "copied late" "copied late"

# The Daemon finds the compositor the same way to paste.
spawn quickshell --path ../type-and-paste/client.qml
wait_for_log quickshell.log "ready"
front_end --app --run "clipboard copied late"
wait_for_log quickshell.log "typed: copied late"
