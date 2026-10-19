#!/usr/bin/env bash
# A test Extension: writes its pid to ./pid, appends every frame it receives to
# ./received, sends the Items in ./items.json after each config, and answers
# every Action with the outcome in ./outcome.json (default "close").
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
send '{"version":1,"formats":["json"]}'
read_frame

while read_frame; do
    printf '%s\n' "$frame" >>received
    if [[ $frame == '{"config":'* ]] && [ -f items.json ]; then
        send "{\"add_items\":{\"items\":$(<items.json)}}"
    elif [[ $frame =~ \"invocation\":([0-9]+) ]]; then
        outcome='"close"'
        if [ -f outcome.json ]; then
            outcome=$(<outcome.json)
        fi
        send "{\"action_done\":{\"invocation\":${BASH_REMATCH[1]},\"outcome\":$outcome}}"
    fi
done
