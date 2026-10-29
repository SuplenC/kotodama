# shellcheck shell=bash
# A form with every kind of field, from the keyboard alone: how it looks as it
# opens, the Daemon's errors under the wrong fields after a refused submit, a
# file field's completion list, then each kind's keys filling it in until the
# Extension gets what was entered.
root=$(cd ../.. && pwd)
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p kotodamad -p kotodama --bin kotodamad --bin kotodama
imports=$("$root/scripts/qml-module")

scratch=$(mktemp -d)
export XDG_CONFIG_HOME="$scratch/config" XDG_CONFIG_DIRS="$scratch/etc" XDG_DATA_DIRS="$scratch/share" XDG_STATE_HOME="$scratch/state" XDG_CACHE_HOME="$scratch/cache"

# The form is taller than the harness's output, which grows for this test.
swaymsg -q output HEADLESS-1 resolution 640x800
restore_output() {
    swaymsg -q output HEADLESS-1 resolution 640x480
    stop_spawned
}
trap restore_output EXIT

# A home of the test's own for the file field to complete in, its files
# modified a fixed time ago so the list reads the same each run.
home="$scratch/home"
mkdir -p "$home/Documents" "$home/Downloads" "$home/Music"
touch "$home/Documents/report.pdf" "$home/Documents/old.txt" "$home/photo.png"
touch -d "2 hours ago" "$home/do-list.txt"
touch -d "26 hours ago" "$home/draft-notes.txt"
touch -d "3 hours ago" "$home/Documents/report.pdf"

extensions="$XDG_CONFIG_HOME/kotodama/extensions"
mkdir -p "$extensions/reminders"
cp "$root/crates/kotodamad/tests/fixtures/recorder/extension.sh" "$extensions/reminders/"
printf 'name = "reminders"\ncommand = ["extension.sh"]\n' >"$extensions/reminders/manifest.toml"
cat >"$extensions/reminders/items.json" <<'JSON'
[{"id": "remind", "kind": "reminders.new", "title": "New reminder", "actions": [{"id": "new", "title": "Create"}]}]
JSON
echo '"keep_open"' >"$extensions/reminders/outcome.json"
cat >"$extensions/reminders/replies-new.json" <<'JSON'
{"push_view": {"id": "reminder", "title": "New reminder", "content": {"form": {"submit": "Save", "fields": [{"id": "about", "label": "About", "input": {"description": {"text": "A reminder pops up at the time you pick, with its notes and the file attached."}}}, {"id": "title", "label": "Title", "input": {"text": {"placeholder": "What to remember"}}}, {"id": "notes", "label": "Notes", "input": {"text_area": {"default": "Bring the slides\nand the adapter"}}, "optional": true}, {"id": "when", "label": "When", "input": {"date": {"time": true, "default": "2026-10-30T09:30"}}}, {"id": "repeat", "label": "Repeat (days)", "input": {"number": {"min": 0, "max": 30, "step": 1, "default": 7}}, "optional": true}, {"id": "tags", "label": "Tags", "input": {"multi_select": {"options": [{"value": "work", "label": "Work"}, {"value": "home", "label": "Home"}, {"value": "urgent", "label": "Urgent"}], "default": ["work", "urgent"]}}, "optional": true}, {"id": "priority", "label": "Priority", "input": {"select": {"options": [{"value": "low", "label": "Low"}, {"value": "normal", "label": "Normal"}, {"value": "high", "label": "High"}], "default": "normal"}}}, {"id": "sound", "label": "Sound", "input": {"checkbox": {"default": true}}, "optional": true}, {"id": "code", "label": "Room code", "input": {"text": {"default": "12ab"}}, "optional": true, "pattern": "[0-9]{6}"}, {"id": "attachment", "label": "Attachment", "input": {"file": {"accept": ["pdf", "txt"], "placeholder": "A PDF or a text file"}}}]}}}}
JSON

HOME=$home spawn "$target/debug/kotodamad"
QML_IMPORT_PATH=$imports spawn quickshell --path shell.qml
wait_for_log quickshell.log "connected: true"

kotodama="$target/debug/kotodama"
# press <wtype args>: sends keys, then waits for the log line given last.
press() {
    local mark
    mark=$(log_lines quickshell.log)
    # wtype's first key is lost if it arrives before the compositor has taken
    # wtype's keymap, so it waits a moment first.
    wtype -s 200 "${@:1:$#-1}"
    wait_for_log quickshell.log "${!#}" "$mark"
}

mark=$(log_lines quickshell.log)
"$kotodama" open --query "new reminder"
wait_for_log quickshell.log "focused: true" "$mark"
wait_for_log quickshell.log "top: New reminder" "$mark"

# The form opens on its first field, the description passed over: Title.
press -k Return "view: form New reminder"
expect_screen open.png

# Enter with the title and the attachment empty and the room code wrong:
# the form stays open and each error shows under its field.
press -k Return "errors: Required, "
expect_screen errors.png

# Shift+Tab from the first field goes round to the last, the attachment.
# Typing a path lists what completes it, folders first, and Down picks one.
wtype -s 200 -M shift -k Tab -m shift
# The tilde is typed, for the Daemon to expand.
# shellcheck disable=SC2088
press "~/do" "completions: Documents, Downloads, do-list.txt, draft-notes.txt"
wtype -s 200 -k Down
expect_screen files.png

# Tab on a folder fills it in and lists what is inside; Enter on a file
# takes it, and closes the list.
press -k Tab "completions: old.txt, report.pdf"
wtype -s 200 -k Down -k Down -k Return
# Tab goes round to the title.
wtype -s 200 -k Tab "Call Ana"
# Shift+Enter starts a line in the notes, at their end.
wtype -s 200 -k Tab -M ctrl -k End -m ctrl -M shift -k Return -m shift "and a pen"
# Right steps the repeat from 7 to 8; in the tags, Right moves to Home and
# Space ticks it; the room code is typed over.
wtype -s 200 -k Tab -k Tab -k Right -k Tab -k Right -k space -k Tab -k Tab -k Tab -M ctrl a -m ctrl 123456
wtype -s 200 -k Return

received="$extensions/reminders/received"
for _ in $(seq 50); do
    grep -qF '"submit_form"' "$received" 2>/dev/null && break
    sleep 0.2
done
if ! grep -qF '"submit_form"' "$received" 2>/dev/null; then
    echo "the form was never submitted to the Extension" >&2
    exit 1
fi
values=$(grep -F '"submit_form"' "$received" | tail -n1 | jq -c '.submit_form.values')
expected=$(jq -cn --arg home "$home" '{
    title: "Call Ana",
    notes: "Bring the slides\nand the adapter\nand a pen",
    when: "2026-10-30T09:30",
    repeat: 8,
    tags: ["work", "home", "urgent"],
    priority: "normal",
    sound: true,
    code: "123456",
    attachment: ($home + "/Documents/report.pdf")
}')
if [ "$(jq -S . <<<"$values")" != "$(jq -S . <<<"$expected")" ]; then
    echo "submitted $values, expected $expected" >&2
    exit 1
fi
