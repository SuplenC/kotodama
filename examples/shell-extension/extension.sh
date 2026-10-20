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

# The bookmarks as a list View, which typing filters. Its Item is hidden: the
# search never shows it, and `kotodama open --view shell-example/bookmarks`
# reaches it, as a keybind would. An Extension with only Live queries names
# its Views this way.
bookmarks_item='{
    "id": "bookmarks",
    "kind": "shell-example.bookmarks",
    "title": "Bookmarks",
    "hidden": true,
    "actions": [{"id": "list-bookmarks", "title": "List bookmarks"}]
}'

# Sends the timer's progress: job id, title, seconds done, seconds in all.
progress() {
    send "$(jq -cn --arg id "$1" --arg title "$2" --argjson elapsed "$3" --argjson all "$4" '{
        job_progress: (if $elapsed < $all then
            {id: $id, title: $title, state: "running", progress: ($elapsed / $all),
             message: "\($all - $elapsed)s left"}
        else
            {id: $id, title: $title, state: "succeeded", progress: 1, message: "Time is up"}
        end)
    }')"
}

# Starts a timer: a progress View to follow it, and a background process that
# reports on it each second. Closing the menu leaves the View open, so the
# timer runs on, and the Daemon sends a desktop notification when it is done.
# Esc on the View cancels it (see view_popped below). Given a third argument,
# the progress View replaces that open View in one step, so the menu never
# shows the search in between, as a pop_view then a push_view could.
timers=0
# The background process and title of each timer, by job id.
declare -A timer_pids=() timer_titles=()
start_timer() {
    local title=$1 seconds=$((10#$2)) replaces=${3:-} id
    timers=$((timers + 1))
    id="timer-$timers"
    send "$(jq -cn --arg id "$id" --arg title "$title" --arg replaces "$replaces" '
        {id: $id, title: $title, content: {progress: {job: $id}}} as $view
        | if $replaces == "" then {push_view: $view}
          else {replace_view: {view: $replaces, with: $view}} end')"
    progress "$id" "$title" 0 "$seconds"
    (
        for ((elapsed = 1; elapsed < seconds; elapsed++)); do
            sleep 1 >/dev/null 2>&1 # so a cancelled timer leaves nothing holding our output
            progress "$id" "$title" "$elapsed" "$seconds"
        done
        sleep 1 >/dev/null 2>&1
        progress "$id" "$title" "$seconds" "$seconds"
    ) </dev/null &
    timer_pids[$id]=$!
    timer_titles[$id]=$title
}

# Cancels timer $1 if it is still counting: the user left its View.
cancel_timer() {
    local id=$1 pid=${timer_pids[$1]:-}
    if [ -n "$pid" ] && kill "$pid" 2>/dev/null; then
        send "$(jq -cn --arg id "$id" --arg title "${timer_titles[$id]}" \
            '{job_progress: {id: $id, title: $title, state: "failed", message: "Cancelled"}}')"
    fi
    unset "timer_pids[$id]" "timer_titles[$id]"
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
# only, so JSON is what the connection uses from here on. Both must speak the
# same protocol version, which goes up whenever a message changes.
version=8
send "{\"version\":$version,\"formats\":[\"json\"]}"
read_frame
if ! jq -e --argjson version "$version" '.version == $version' <<<"$frame" >/dev/null; then
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
        # Adding the timer and the bookmarks list again just replaces them,
        # so they are never removed.
        send "$(jq -c --argjson timer "$timer" --argjson list "$bookmarks_item" \
            '{add_items: {items: (. + [$timer, $list])}}' <<<"$items")"
        ;;
    run_action)
        case $(jq -r '.run_action.action' <<<"$frame") in
        list-bookmarks)
            # Typing filters the list in the Daemon, so we never hear of it.
            send "$(jq -cn --argjson items "$items" \
                '{push_view: {id: "bookmarks", title: "Bookmarks", content: {list: {items: $items}}}}')"
            done_with "$(jq -r '.run_action.invocation' <<<"$frame")" '"keep_open"'
            ;;
        open)
            xdg-open "$(jq -r '.run_action.item.url' <<<"$frame")" </dev/null >/dev/null 2>&1 &
            send "$(jq -c '{action_done: {invocation: .run_action.invocation, outcome: "close"}}' <<<"$frame")"
            ;;
        start)
            # The Daemon has parsed `timer 5` into the `seconds` Argument.
            invocation=$(jq -r '.run_action.invocation' <<<"$frame")
            seconds=$(jq -r '.run_action.arguments.seconds // ""' <<<"$frame")
            if valid_seconds "$seconds"; then
                done_with "$invocation" '"keep_open"'
                start_timer "$seconds second timer" "$seconds"
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
        # Our labelled timer's form. The timer's progress View takes the
        # form's place.
        if [ "$(jq -r '.submit_form.view' <<<"$frame")" = timer-form ]; then
            label=$(jq -r '.submit_form.values.label // "" | if . == "" then "Timer" else . end' <<<"$frame")
            seconds=$(jq -r '.submit_form.values.seconds // ""' <<<"$frame")
            if valid_seconds "$seconds"; then
                start_timer "$label" "$seconds" timer-form
            fi
        fi
        ;;
    view_popped)
        # The user pressed Esc on one of our Views. Closing the menu never
        # sends this, so a timer whose View it was can safely stop.
        view=$(jq -r '.view_popped.view' <<<"$frame")
        if [[ $view == timer-[0-9]* ]]; then
            cancel_timer "$view"
        fi
        ;;
    *)
        # Ignore what we don't handle, so newer Daemons can send more.
        ;;
    esac
done
