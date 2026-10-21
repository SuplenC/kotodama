# shellcheck shell=bash
# The system Extension on sway: two windows are found by their titles through
# wlr-foreign-toplevel-management, Focus focuses one, Close closes the other,
# and the list follows. Its power Items are listed without logind, which the
# test environment doesn't run.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama-system \
    --bin kotodamad --bin kotodama-system --example front-end

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"
# No system bus: the power Items are listed all the same.
export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$home/no-bus"

# Installed as a package installs it: the binary next to its Manifest.
ext="$XDG_DATA_DIRS/kotodama/extensions/system"
mkdir -p "$ext"
cp "$root/crates/kotodama-system/manifest.toml" "$ext/"
ln -s "$target/debug/kotodama-system" "$ext/kotodama-system"

spawn "$target/debug/kotodamad"
front_end="$target/debug/examples/front-end"

# until <command...>: retries the command for up to 10 seconds.
until_ok() {
    for _ in $(seq 50); do
        if "$@"; then
            return 0
        fi
        sleep 0.2
    done
    echo "never true: $*" >&2
    return 1
}
# shows <query> <pattern>: the Results for the query have a row matching the pattern.
shows() {
    "$front_end" "$1" 2>/dev/null | grep -qP "$2"
}
focused() {
    swaymsg -t get_tree | jq -e --arg title "$1" '[.. | objects | select(.focused == true) | .name] == [$title]' >/dev/null
}
mapped() {
    swaymsg -t get_tree | jq -e --arg title "$1" 'any(.. | objects; .name == $title)' >/dev/null
}

until_ok shows "power off" '^system\tpower-off\tPower Off$'
"$front_end" reboot | grep -qP '^system\treboot\tReboot$'
# Without logind there is no Hibernate.
if "$front_end" hibernate | grep -q '^system'; then
    echo "Hibernate is listed without logind" >&2
    exit 1
fi

E2E_TITLE=e2e-alpha spawn quickshell --path window.qml
until_ok mapped e2e-alpha
E2E_TITLE=e2e-beta spawn quickshell --path window.qml
until_ok mapped e2e-beta
until_ok shows e2e-alpha '^system\twindow-[0-9]+\te2e-alpha$'
until_ok shows e2e-beta '^system\twindow-[0-9]+\te2e-beta$'
until_ok focused e2e-beta

# Focus, the primary Action.
"$front_end" --run e2e-alpha
until_ok focused e2e-alpha

"$front_end" --action close e2e-beta
until_ok eval '! mapped e2e-beta'
until_ok eval '! shows e2e-beta e2e-beta'
shows e2e-alpha '^system\twindow-[0-9]+\te2e-alpha$'

