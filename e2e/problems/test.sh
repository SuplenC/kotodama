# shellcheck shell=bash
# A setting that isn't one is a problem: the footer counts it, a click on the
# count lists it in a View, and fixing the config clears it.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

# Headless sway has no pointer of its own: this one gives the seat one for the
# test, which `swaymsg seat - cursor` then moves and clicks.
rustc --edition 2024 -O -o "$E2E_OUT/virtual-pointer" ../pointer/virtual-pointer.rs
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
config="$XDG_CONFIG_HOME/kotodama"
mkdir -p "$config"
printf '[history]\nhalf_life_days = 0\n' >"$config/config.toml"

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"
wait_for_log quickshell.log "problems: 1"

kotodama="$target/debug/kotodama"
# `kotodama check` lists the problem and fails.
if "$kotodama" check >"$E2E_OUT/check.txt"; then
    echo "kotodama check succeeded with a problem" >&2
    exit 1
fi
grep -q "^history.half_life_days" "$E2E_OUT/check.txt"

mark=$(log_lines quickshell.log)
"$kotodama" open --query nothing-matches-this
wait_for_log quickshell.log "focused: true" "$mark"
expect_screen footer.png

# The count sits at the right end of the footer, under the empty list.
mark=$(log_lines quickshell.log)
swaymsg seat - cursor set 540 113 >/dev/null
swaymsg seat - cursor press button1 >/dev/null
swaymsg seat - cursor release button1 >/dev/null
wait_for_log quickshell.log "view: Problems" "$mark"
expect_screen list.png

# Fixing the config clears it, in the list that is open and the footer.
mark=$(log_lines quickshell.log)
printf '[history]\nhalf_life_days = 7\n' >"$config/config.toml"
wait_for_log quickshell.log "problems: 0" "$mark"
"$kotodama" check
