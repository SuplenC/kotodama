# shellcheck shell=bash
# An Extension whose Manifest gives it a Prefix, the shell example here, keeps
# its Items out of the search: `shell fi` searches them alone, behind a chip
# that names the Extension. Backspace on an empty box leaves the Prefix, and
# Enter on the Extension's entry Item, or `kotodama open --prefix`, enters it.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

ext="$XDG_CONFIG_HOME/kotodama/extensions/apps"
mkdir -p "$ext"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$ext/"
printf 'name = "apps"\ncommand = ["extension.sh"]\n' >"$ext/manifest.toml"
cat >"$ext/items.json" <<'JSON'
[
  {"id":"firefox","kind":"url","url":"https://firefox","title":"Firefox","actions":[{"id":"go","title":"Go"}]},
  {"id":"files","kind":"url","url":"https://files","title":"Files","actions":[{"id":"go","title":"Go"}]}
]
JSON

# The shell example as a third party ships it, with a Prefix in its Manifest.
shell="$XDG_CONFIG_HOME/kotodama/extensions/shell-example"
mkdir -p "$shell"
cp "$root"/examples/shell-extension/extension.sh "$shell/"
{
    printf 'prefix = "shell"\ntitle = "Shell example"\n'
    cat "$root/examples/shell-extension/manifest.toml"
} >"$shell/manifest.toml"
cat >"$XDG_CONFIG_HOME/kotodama/config.toml" <<'TOML'
[extensions.shell-example]
bookmarks = [
    { title = "Firefox docs", url = "https://firefox-source-docs.mozilla.org" },
    { title = "Fish shell", url = "https://fishshell.com" },
]
TOML

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

# Opened behind the Prefix, the box holds what follows it, after a chip, and
# only the shell example's rows show. Both Extensions may still be sending
# their Items.
mark=$(log_lines quickshell.log)
open --query "shell fire"
wait_for_log quickshell.log "scope: [Shell example]" "$mark"
wait_for_log quickshell.log "rows: [shell-example]" "$mark"
expect_screen reference.png
close

# The whole search leaves the shell example's Items out.
open
press "fi" "rows: [apps, apps]"
close

# Typed, the Prefix and a space enter it, however fast the rest follows.
open
mark=$(log_lines quickshell.log)
press "shell fi" "rows: [shell-example, shell-example]"
wait_for_log quickshell.log "query: [fi]" "$mark"
if [[ "$(last_rows)" != *"rows: [shell-example, shell-example]"* ]]; then
    echo "the search behind the Prefix showed $(last_rows)" >&2
    exit 1
fi

# Backspace on the empty box leaves the Prefix, which is typed again: the
# shell example's entry Item comes first.
press -k BackSpace -k BackSpace "query: []"
mark=$(log_lines quickshell.log)
press -k BackSpace "scope: []"
wait_for_log quickshell.log "query: [shell]" "$mark"
wait_for_log quickshell.log "top: Shell example" "$mark"

# Enter on the entry Item enters the Prefix again, with nothing typed.
mark=$(log_lines quickshell.log)
press -k Return "scope: [Shell example]"
wait_for_log quickshell.log "query: []" "$mark"
close

mark=$(log_lines quickshell.log)
open --prefix shell
wait_for_log quickshell.log "scope: [Shell example]" "$mark"
press -k Escape "focused: false"
