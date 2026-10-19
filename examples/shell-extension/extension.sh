#!/usr/bin/env bash
# A Kotodama Extension in bash and jq: the reference for Extension authors. See
# README.md next to it for the protocol.
#
# It indexes the bookmarks in its config table as `url` Items, opens them, and
# offers "Paste path" on every `project` Item. Its Timer shows the rest: an
# Argument typed after a keyword, a form, and a job with a progress View.
#
# stdin and stdout carry the protocol, so nothing else may write to stdout. Log
# to stderr.
set -euo pipefail

# Frame lengths count bytes. In the C locale, ${#var} and `read -N` do too.
export LC_ALL=C

# Reads one frame into $frame. Fails when the Daemon hangs up.
read_frame() {
    local len
    IFS= read -r len || return 1
    frame=
    if [ "$len" -gt 0 ]; then
        IFS= read -r -N "$len" frame || return 1
    fi
}

# Sends its argument, a JSON message, as one frame. Running timers send from
# background processes too; one printf is one write, and a write to a pipe
# this small is never split, so frames don't interleave.
send() {
    printf '%d\n%s' "${#1}" "$1"
}

# The Timer: `timer 5` counts down five seconds. Without the seconds, the
# Daemon asks for them in a form, since the Argument is required. Its second
# Action, also on Ctrl+L, asks for a label too, in a form of our own.
timer='{
    "id": "timer",
    "kind": "shell-example.timer",
    "title": "Timer",
    "subtitle": "Counts down, then notifies",
    "keyword": "timer",
    "actions": [
        {"id": "start", "title": "Start timer",
         "arguments": [{"name": "seconds", "placeholder": "Seconds"}]},
        {"id": "start-labelled", "title": "Start a labelled timer", "shortcut": "ctrl+l"}
    ]
}'

# Sends the timer's progress: job id, title, seconds done, seconds in all.
progress() {
    send "$(jq -cn --arg id "$1" --arg title "$2" --argjson done "$3" --argjson all "$4" '{
        job_progress: (if $done < $all then
            {id: $id, title: $title, state: "running", progress: ($done / $all),
             message: "\($all - $done)s left"}
        else
            {id: $id, title: $title, state: "succeeded", progress: 1, message: "Time is up"}
        end)
    }')"
}

# Starts a timer: a progress View to follow it, and a background process that
# reports on it each second. Closing the menu pops the View; the timer runs on,
# and the Daemon sends a desktop notification when it is done.
timers=0
start_timer() {
    local title=$1 seconds=$((10#$2)) id
    timers=$((timers + 1))
    id="timer-$timers"
    send "$(jq -cn --arg id "$id" --arg title "$title" \
        '{push_view: {id: $id, title: $title, content: {progress: {job: $id}}}}')"
    (
        for ((elapsed = 0; elapsed < seconds; elapsed++)); do
            progress "$id" "$title" "$elapsed" "$seconds"
            sleep 1
        done
        progress "$id" "$title" "$seconds" "$seconds"
    ) </dev/null &
}

# Whether $1 is a whole number of seconds a timer can count.
valid_seconds() {
    [[ $1 =~ ^[0-9]{1,4}$ ]] && ((10#$1 > 0))
}

# Answers run_action invocation $1 with outcome $2, a JSON value.
done_with() {
    send "$(jq -cn --argjson invocation "$1" --argjson outcome "$2" \
        '{action_done: {invocation: $invocation, outcome: $outcome}}')"
}

# The handshake is always JSON. Both sides send a hello; this script offers JSON
# only, so JSON is what the connection uses from here on.
send '{"version":1,"formats":["json"]}'
read_frame
if ! jq -e '.version == 1' <<<"$frame" >/dev/null; then
    echo "shell-example: unsupported protocol version: $frame" >&2
    exit 1
fi

# The ids of the Items indexed now, so a config reload can remove them.
ids='[]'

while read_frame; do
    # A message is {"<name>": {...}}, or just "<name>" when it has no fields.
    case $(jq -r 'if type == "string" then . else keys[0] end' <<<"$frame") in
    config)
        # Our [extensions.shell-example] table. It comes after the handshake and
        # again whenever the user's config changes.
        if [ "$ids" != '[]' ]; then
            send "$(jq -cn --argjson ids "$ids" '{remove_items: {ids: $ids}}')"
        fi
        items=$(jq -c '[.config.bookmarks // [] | .[] | {
            id: .url,
            kind: "url",
            url,
            title,
            subtitle: .url,
            actions: [{id: "open", title: "Open in browser"}]
        }]' <<<"$frame")
        ids=$(jq -c 'map(.id)' <<<"$items")
        # Adding the timer again just replaces it, so it is never removed.
        send "$(jq -c --argjson timer "$timer" '{add_items: {items: (. + [$timer])}}' <<<"$items")"
        ;;
    run_action)
        case $(jq -r '.run_action.action' <<<"$frame") in
        open)
            xdg-open "$(jq -r '.run_action.item.url' <<<"$frame")" </dev/null >/dev/null 2>&1 &
            send "$(jq -c '{action_done: {invocation: .run_action.invocation, outcome: "close"}}' <<<"$frame")"
            ;;
        start)
            # The Daemon has parsed `timer 5` into the `seconds` Argument.
            invocation=$(jq -r '.run_action.invocation' <<<"$frame")
            seconds=$(jq -r '.run_action.arguments.seconds // ""' <<<"$frame")
            if valid_seconds "$seconds"; then
                start_timer "$seconds second timer" "$seconds"
                done_with "$invocation" '"keep_open"'
            else
                done_with "$invocation" "$(jq -cn --arg s "$seconds" '{failed: {message: "not a number of seconds: \($s)"}}')"
            fi
            ;;
        start-labelled)
            send '{"push_view":{"id":"timer-form","title":"Labelled timer","content":{"form":{
                "fields":[
                    {"id":"label","label":"Label","input":{"text":{"placeholder":"Tea"}}},
                    {"id":"seconds","label":"Seconds","input":{"text":{"default":"3"}}}
                ],
                "submit":"Start"}}}}'
            done_with "$(jq -r '.run_action.invocation' <<<"$frame")" '"keep_open"'
            ;;
        paste-path)
            # A Contributed action: the Item belongs to another Extension, and
            # all we get of it is its id, kind and the kind's field.
            send "$(jq -c '.run_action | {action_done: {invocation, outcome: {paste: {text: .item.path}}}}' <<<"$frame")"
            ;;
        *)
            send "$(jq -c '.run_action | {action_done: {invocation, outcome: {failed: {message: "unknown action \(.action)"}}}}' <<<"$frame")"
            ;;
        esac
        ;;
    submit_form)
        # Our labelled timer's form. The form goes, and the timer's progress
        # View takes its place.
        if [ "$(jq -r '.submit_form.view' <<<"$frame")" = timer-form ]; then
            label=$(jq -r '.submit_form.values.label // "" | if . == "" then "Timer" else . end' <<<"$frame")
            seconds=$(jq -r '.submit_form.values.seconds // ""' <<<"$frame")
            if valid_seconds "$seconds"; then
                send '{"pop_view":{"view":"timer-form"}}'
                start_timer "$label" "$seconds"
            fi
        fi
        ;;
    *)
        # Ignore what we don't handle, so newer Daemons can send more. That
        # includes view_popped: a timer runs on when its View goes.
        ;;
    esac
done
