# shellcheck shell=bash
# The emoji Extension's primary Action types the character into the window
# that has focus: an emoji, a symbol, and an emoji in the configured skin tone.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama-emoji \
    --bin kotodamad --bin kotodama-emoji --example front-end

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state"

# Installed as a package installs it: the binary next to its Manifest.
ext="$XDG_DATA_DIRS/kotodama/extensions/emoji"
mkdir -p "$ext"
cp "$root/crates/kotodama-emoji/manifest.toml" "$ext/"
ln -s "$target/debug/kotodama-emoji" "$ext/kotodama-emoji"

spawn quickshell --path ../type-and-paste/client.qml
wait_for_log quickshell.log "ready"

spawn "$target/debug/kotodamad"
front_end="$target/debug/examples/front-end"
for _ in $(seq 50); do
    if "$front_end" thumbs 2>/dev/null | grep -q Thumbs; then
        break
    fi
    sleep 0.1
done

"$front_end" --run thumbs
wait_for_log quickshell.log "typed: 👍"
"$front_end" --run not equal
wait_for_log quickshell.log "typed: 👍≠"

# The config is watched: a skin tone set now applies to the next Action.
mkdir -p "$XDG_CONFIG_HOME/kotodama"
printf '[extensions.emoji]\nskin_tone = "dark"\n' >"$XDG_CONFIG_HOME/kotodama/config.toml"
sleep 1
"$front_end" --run thumbs
wait_for_log quickshell.log "typed: 👍≠👍🏿"
