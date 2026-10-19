# shellcheck shell=bash
# An Action whose outcome is "type" types its text into the window that has focus.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad --bin kotodamad --example front-end

home=$(mktemp -d)
ext="$home/config/kotodama/extensions/typer"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "typer"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
printf '[{"id":"s","kind":"url","url":"https://s","title":"Snippet","actions":[{"id":"type","title":"Type"}]}]' >"$ext/items.json"
printf '{"type":{"text":"Hello, Wayland!"}}' >"$ext/outcome.json"
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share"

spawn quickshell --path client.qml
wait_for_log quickshell.log "ready"

spawn "$target/debug/kotodamad"
for _ in $(seq 50); do
    if "$target/debug/examples/front-end" snippet 2>/dev/null | grep -q Snippet; then
        break
    fi
    sleep 0.1
done
"$target/debug/examples/front-end" --run snippet

wait_for_log quickshell.log "typed: Hello, Wayland!"
