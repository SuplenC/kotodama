# shellcheck shell=bash
# Actions whose outcome is "type" or "paste" put their text into the window
# that has focus, and "copy" puts it on the clipboard for the next Ctrl+V.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad --bin kotodamad --example front-end

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share"

# install <name> <title> <outcome>: a test Extension with one Item whose
# Action answers with the outcome.
install() {
    local ext="$XDG_CONFIG_HOME/kotodama/extensions/$1"
    mkdir -p "$ext"
    cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
    printf 'name = "%s"\ncommand = ["extension.sh"]\n' "$1" >"$ext/manifest.toml"
    printf '[{"id":"%s","kind":"url","url":"https://%s","title":"%s","actions":[{"id":"go","title":"Go"}]}]' "$1" "$1" "$2" >"$ext/items.json"
    printf '%s' "$3" >"$ext/outcome.json"
}
install typer Snippet '{"type":{"text":"Hello, Wayland!"}}'
install paster Clip '{"paste":{"text":" Pasted."}}'
install copier Copier '{"copy":{"text":" Copied."}}'

spawn quickshell --path client.qml
wait_for_log quickshell.log "ready"

spawn "$target/debug/kotodamad"
front_end="$target/debug/examples/front-end"
for _ in $(seq 50); do
    if "$front_end" clip 2>/dev/null | grep -q Clip; then
        break
    fi
    sleep 0.1
done

"$front_end" --run snippet
wait_for_log quickshell.log "typed: Hello, Wayland!"
"$front_end" --run clip
wait_for_log quickshell.log "typed: Hello, Wayland! Pasted."
"$front_end" --run copier
# The copy happens after the menu closes, on the Daemon's Wayland thread.
sleep 1
wtype -M ctrl v -m ctrl
wait_for_log quickshell.log "typed: Hello, Wayland! Pasted. Copied."
