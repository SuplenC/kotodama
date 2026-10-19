# A Kotodama Extension in bash and jq

`extension.sh` is a complete Extension in about 80 lines of bash and jq. Start here if you are writing your own, in any language. It needs bash 4 or later and jq.

It does three things:

- turns the bookmarks in its config table into `url` Items;
- opens a bookmark when you pick it;
- offers **Paste path** on every `project` Item, whichever Extension added it.

## Installing it

Copy or link this directory into `~/.config/kotodama/extensions/`, then give it some bookmarks in any file in `~/.config/kotodama/`:

```toml
[extensions.shell-example]
bookmarks = [
    { title = "Kotodama", url = "https://github.com/SuplenC/kotodama" },
]
```

## The protocol in brief

The Daemon starts `command` from `manifest.toml`, in this directory, and talks to it over stdin and stdout. Everything below is in `crates/kotodama-protocol`, whose docs are the full reference.

**Frames.** Every message is one frame: its length in bytes as decimal digits, a newline, then the message. No newline follows the message.

```
30
{"remove_items":{"ids":["a"]}}
```

**Handshake.** Each side sends a hello first, always in JSON: `{"version":1,"formats":["json"]}`. List the Wire formats you support, from `postcard`, `msgpack` and `json`. Both sides then use the fastest they share. JSON is required, so offering only JSON is enough.

**Messages.** A message is an object with one key, its name, holding its fields: `{"add_items": {"items": [...]}}`. A message without fields is just its name as a string: `"request_action_catalog"`. Optional fields can be left out.

**Config.** Right after the handshake, and again whenever the user's config changes, the Daemon sends your `[extensions.<name>]` table as `{"config": {...}}`. Never read config files yourself.

**Items.** Every Item has an Item kind. A shared kind needs its one field: `app` needs `desktop_id`; `file`, `folder` and `project` need `path`; `window` needs `address`; `url` needs `url`. Any other kind must be named `<your extension name>.<kind>`, as in `shell-example.note`. Every Item lists at least one Action; the first is its primary Action. The Daemon rejects a message with an Item that breaks these rules.

```json
{"id": "kotodama", "kind": "url", "url": "https://github.com/SuplenC/kotodama",
 "title": "Kotodama", "actions": [{"id": "open", "title": "Open in browser"}]}
```

Adding an Item whose id you already sent replaces it. `remove_items` takes the ids to drop.

**Identity.** When your Item is something another Extension may offer too, such as an app, a file or a web page, give it an `identity`: a string naming the thing itself, which every Extension that offers it spells the same way. Items with the same identity share their History, so picking one raises the others; both rows still show, and the one the user picks through comes first. For a shared kind, use the kind and its field, as in `app:firefox.desktop`, `file:/etc/hosts` or `url:https://github.com/SuplenC/kotodama`. Leave it out for anything only your Extension offers. An empty identity is rejected.

**Actions.** When the user runs one of your Actions, you get `run_action` with an `invocation` number, the Action's `id`, the Item's `id`, `kind` and kind field, and any Arguments. Answer with `action_done` and the same `invocation`, and an outcome: `"close"`, `"keep_open"`, `{"type": {"text": "..."}}` to type into the focused window, `{"paste": {"text": "..."}}` to paste, or `{"failed": {"message": "..."}}`. The Daemon does the typing and pasting, so don't call `wtype` or `wl-copy` yourself.

**Contributed actions.** Declare them in `manifest.toml` under `[[contributed_actions]]`, with the Item kind they apply to. They arrive as ordinary `run_action` messages carrying the other Extension's Item id, kind and kind field. That is all you ever see of another Extension's Items.

**stdout is the protocol.** Write logs to stderr, and redirect the output of anything you start.
