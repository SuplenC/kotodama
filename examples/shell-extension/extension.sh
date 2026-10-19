#!/usr/bin/env bash
# A Kotodama Extension in bash and jq: the reference for Extension authors. See
# README.md next to it for the protocol.
#
# It indexes the bookmarks in its config table as `url` Items, opens them, and
# offers "Paste path" on every `project` Item.
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

# Sends its argument, a JSON message, as one frame.
send() {
    printf '%d\n%s' "${#1}" "$1"
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
        send "$(jq -c '{add_items: {items: .}}' <<<"$items")"
        ;;
    run_action)
        case $(jq -r '.run_action.action' <<<"$frame") in
        open)
            xdg-open "$(jq -r '.run_action.item.url' <<<"$frame")" </dev/null >/dev/null 2>&1 &
            send "$(jq -c '{action_done: {invocation: .run_action.invocation, outcome: "close"}}' <<<"$frame")"
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
    *)
        # Ignore what we don't handle, so newer Daemons can send more.
        ;;
    esac
done
