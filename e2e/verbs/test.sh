# shellcheck shell=bash
# A verb in the query chooses the Action Enter runs: `remove fi` puts a red
# `↵ Remove` pill on the Result a package manager offers to remove, and the
# footer names it in red. Ctrl+K lists that Action first; on the Item's
# usual Action, the panel lists its own Actions, a line, the Contributed
# ones, a line, then the destructive ones in red.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

home=$(mktemp -d)
export XDG_CONFIG_HOME="$home/config" XDG_CONFIG_DIRS="$home/etc" XDG_DATA_DIRS="$home/share" XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache"

extensions="$XDG_CONFIG_HOME/kotodama/extensions"
mkdir -p "$extensions/apps" "$extensions/packages"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$extensions/apps/"
printf 'name = "apps"\ncommand = ["extension.sh"]\n' >"$extensions/apps/manifest.toml"
cat >"$extensions/apps/items.json" <<'JSON'
[
  {"id":"firefox","kind":"url","url":"https://firefox","identity":"app:firefox.desktop","title":"Firefox","subtitle":"Web browser",
   "actions":[{"id":"open","title":"Open"},{"id":"edit","title":"Edit bookmark","role":"form"}]},
  {"id":"files","kind":"url","url":"https://files","identity":"app:files.desktop","title":"Files","subtitle":"File manager",
   "actions":[{"id":"open","title":"Open"}]}
]
JSON

# A package manager that offers to remove Firefox alone, and a link to copy
# on every url.
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$extensions/packages/"
cat >"$extensions/packages/manifest.toml" <<'TOML'
name = "packages"
command = ["extension.sh"]

[[contributed_actions]]
kind = "url"
id = "copy-link"
title = "Copy link"

[[contributed_actions]]
targeted = true
id = "remove"
title = "Remove"
keywords = ["remove", "uninstall"]
role = "destructive"
TOML
echo '{"target_action":{"action":"remove","identities":["app:firefox.desktop"]}}' >"$extensions/packages/started.json"

spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path ../menu/shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
mark=$(log_lines quickshell.log)
"$kotodama" open
wait_for_log quickshell.log "focused: true" "$mark"

# wtype's first key is lost if it arrives before the compositor has taken
# wtype's keymap, so it waits a moment first.
wtype -s 200 "remove fi"
expect_screen row.png

wtype -s 200 -M ctrl k -m ctrl
expect_screen chosen.png

# Esc closes the panel, then clears the query.
wtype -s 200 -k Escape -k Escape "firef"
wtype -s 200 -M ctrl k -m ctrl
expect_screen panel.png
