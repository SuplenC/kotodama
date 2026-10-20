#!/usr/bin/env bash
# A test Extension: writes its pid to ./pid, appends every frame it receives to
# ./received, sends the Items in ./items.json after each config, answers each
# Live query with the results in ./live.json if there is one, and answers
# every Action with the outcome in ./outcome.json (default "close"), after the
# messages in ./replies-<action id>.json if there are any.
set -euo pipefail
export LC_ALL=C

read_frame() {
    local len
    IFS= read -r len || return 1
    frame=
    if [ "$len" -gt 0 ]; then
        IFS= read -r -N "$len" frame || return 1
    fi
}

send() {
    printf '%d\n%s' "${#1}" "$1"
}

echo $$ >pid
send '{"version":8,"formats":["json"]}'
read_frame

while read_frame; do
    printf '%s\n' "$frame" >>received
    if [[ $frame == '{"config":'* ]] && [ -f items.json ]; then
        send "{\"add_items\":{\"items\":$(<items.json)}}"
    elif [[ $frame =~ ^\{\"live_query\":\{\"id\":([0-9]+) ]] && [ -f live.json ]; then
        send "{\"live_results\":{\"live_query\":${BASH_REMATCH[1]},\"results\":$(<live.json),\"done\":true}}"
    elif [[ $frame =~ \"invocation\":([0-9]+) ]]; then
        invocation=${BASH_REMATCH[1]}
        # Before it answers, the messages in ./replies-<action>.json, one per
        # line, all in one reply.
        if [[ $frame =~ \"action\":\"([^\"]+)\" ]] && [ -f "replies-${BASH_REMATCH[1]}.json" ]; then
            while IFS= read -r reply; do
                send "$reply"
            done <"replies-${BASH_REMATCH[1]}.json"
        fi
        outcome='"close"'
        if [ -f outcome.json ]; then
            outcome=$(<outcome.json)
        fi
        send "{\"action_done\":{\"invocation\":$invocation,\"outcome\":$outcome}}"
    fi
done
