# A Kotodama Extension in bash and jq

`extension.sh` is a complete Extension in a little over 200 lines of bash and jq. It needs bash 4 or later and jq. [docs/extensions.md](../../docs/extensions.md) is the guide to writing your own, in any language, and `kotodama extension new` writes a small one to start from.

It does five things:

- turns the bookmarks in its config table into `url` Items;
- opens a bookmark when you pick it;
- lists the bookmarks in a View that typing filters, which `kotodama open --view shell-example/bookmarks` opens;
- offers **Paste path** on every `project` Item, whichever Extension added it;
- adds a **Timer**. Type `timer 5` and press Enter to count down five seconds in a progress View. Press Enter on plain `timer` and the Daemon asks for the seconds in a form. Its second Action, **Start a labelled timer** (Ctrl+L, or pick it from the Ctrl+K Action panel), asks for a label too, in the Extension's own form. Esc on its progress View cancels it. Closing the menu leaves it running, and a desktop notification says when it's done.

## Installing it

Copy or link this directory into `~/.config/kotodama/extensions/`, then give it some bookmarks in any file in `~/.config/kotodama/`:

```toml
[extensions.shell-example]
bookmarks = [
    { title = "Kotodama", url = "https://github.com/SuplenC/kotodama" },
]
```

## The protocol in brief

The Daemon starts `command` from `manifest.toml`, in this directory, and talks to it over stdin and stdout. Everything below is in `crates/kotodama-protocol`, and [docs/protocol.md](../../docs/protocol.md) shows every message.

**Frames.** Every message is one frame: its length in bytes as decimal digits, a newline, then the message. No newline follows the message.

```
30
{"remove_items":{"ids":["a"]}}
```

**Handshake.** Each side sends a hello first, always in JSON: `{"version":1,"minor":0,"formats":["json"]}`. `version` and `minor` are the protocol version your Extension was written for, 1.0 here; `PROTOCOL_VERSION` in `crates/kotodama-protocol` is the newest. A minor version only adds to the one before, so a Daemon on 1.3 talks to an Extension written for 1.0 and never sends it what 1.0 lacks. The Daemon turns away an Extension newer than itself, and your Extension should turn away a Daemon older than the version it needs, as this one does. List the Wire formats you support, from `postcard`, `protobuf`, `msgpack` and `json`. Both sides then use the fastest they share; postcard only between peers on the same version. JSON is required, so offering only JSON is enough. For `protobuf`, generate your messages from `crates/kotodama-protocol/kotodama.proto`.

**Messages.** A message is an object with one key, its name, holding its fields: `{"add_items": {"items": [...]}}`. A message without fields is just its name as a string: `"request_action_catalog"`. Optional fields can be left out.

**Config.** Right after the handshake, and again whenever the user's config changes, the Daemon sends your `[extensions.<name>]` table as `{"config": {...}}`. Never read config files yourself.

**Items.** Every Item has an Item kind. A shared kind needs its one field: `app` needs `desktop_id`; `file`, `folder` and `project` need `path`; `window` needs `address`; `url` needs `url`. Any other kind must be named `<your extension name>.<kind>`, as in `shell-example.note`. Every Item lists at least one Action; the first is its primary Action. The Daemon rejects a message with an Item that breaks these rules.

```json
{"id": "kotodama", "kind": "url", "url": "https://github.com/SuplenC/kotodama",
 "title": "Kotodama", "actions": [{"id": "open", "title": "Open in browser"}]}
```

Adding an Item whose id you already sent replaces it. `remove_items` takes the ids to drop. An Item with `"hidden": true` never shows in the search: only `kotodama open --view <extension>/<item id>` reaches it, as a keybind would, and runs its primary Action. That is how an Extension with only Live queries makes a View reachable.

**Identity.** When your Item is something another Extension may offer too, such as an app, a file or a web page, give it an `identity`: a string naming the thing itself, which every Extension that offers it spells the same way. Items with the same identity share their History, so picking one raises the others. Each still shows as a Result of its own, and the one the user picks through most comes first. For a shared kind, use the kind and its field, as in `app:firefox.desktop`, `file:/etc/hosts` or `url:https://github.com/SuplenC/kotodama`. Leave it out for anything only your Extension offers. An empty identity is rejected.

**Actions.** When the user runs one of your Actions, you get `run_action` with an `invocation` number, the Action's `id`, the Item's `id`, `kind` and kind field, and any Arguments. Answer with `action_done` and the same `invocation`, and an outcome: `"close"`, `"keep_open"`, `{"type": {"text": "..."}}` to type into the focused window, `{"paste": {"text": "..."}}` to paste, `{"copy": {"text": "..."}}` to put it on the clipboard, or `{"failed": {"message": "..."}}`. The Daemon does the typing, pasting and copying, so don't call `wtype` or `wl-copy` yourself.

**Withdrawing a pick.** Running an Action counts as a pick of its Result, which raises it in the ranking. When the run turns out to be a mistake, say the app it launched closed again within a few seconds, send `{"withdraw_pick": {"invocation": 7}}` with the run's `invocation`. The History then becomes as if the user had never picked the Result. Only the Extension that ran the Action may withdraw its pick, and only for a minute after `run_action`; the Daemon ignores any other. The apps Extension does this for an app whose launch closes within `withdraw_within` seconds: everything it started, its whole scope, is gone, and no copy of the program ran before it.

**Arguments and shortcuts.** An Action can list `arguments`, each with a `name`, an optional `placeholder`, and `optional: true` if it may be left out. With an Item `keyword`, the user types them after it: `timer 5` gives `{"seconds": "5"}`. Each Argument takes one word, and the last takes the rest. The Daemon asks for a required one that wasn't typed in a form of its own, so `run_action` always has them. A `shortcut` such as `"ctrl+l"` runs the Action from the menu; it needs Ctrl, Alt or Super, since a bare key types into the search. Give an Action that can't be undone `confirm: true`: the menu asks `Delete Everything? ↵ Confirm · Esc Cancel` in its footer and runs it on a second Enter only, so you need no form of your own to ask. In Rust, `Action::new(..).confirm()`.

**Views.** `push_view` puts a View on top of the menu: a `list` of Items, a `form`, a Markdown `detail`, or a `progress` View that follows a job. `update_view` replaces it, and `pop_view` takes it away. To go from one View to the next, as from a form to its progress, send `{"replace_view": {"view": "<open view id>", "with": {...}}}`: a `pop_view` then a `push_view` may show the search in between. When the user leaves one with Esc, you get `view_popped`; treat it as "cancel" if that suits you.

Typing into a `list` filters it. The Daemon matches the Items as it does the search, best first and in your order within each Tier, and never asks you. To filter it yourself, as for results from a web service, set `"filters_itself": true` in the list: you then get `{"filter_view": {"view": "...", "text": "..."}}` with the whole text on each keystroke, empty once cleared, and answer with an `update_view` holding the Items to show. Contributed actions are offered on a list's Items by their kind, as on Results. Closing the menu pops nothing and sends nothing: your Views are still there when it opens again. Submitting a form sends you `submit_form` with its values. The Daemon checks them first, so a field you didn't mark `"optional": true` is filled in, a `number` is within its `min`, `max` and `step` (and arrives as a number), and a text matches its `pattern`; a form that fails stays open with the Front end saying what is wrong, and you get nothing. Answer an Action that pushed a View with `"keep_open"`.

**Jobs.** Report long work with `job_progress`: an `id`, a `title`, a `state` (`running`, `succeeded` or `failed`), and optionally a `progress` from 0 to 1 and a `message`. A job runs on when the menu closes. The menu lists running jobs when it opens again, and when one finishes, the Daemon sends a desktop notification with its title and message.

**An app.** Your Extension is an app: the search shows one entry Item for it, found by its title and its word, and Tab or Enter on it enters the app, where the query searches your Items alone and a chip names the app. Esc goes back to the whole search, and `kotodama open --app shell` opens the menu in it. In `manifest.toml`, `word = "shell"` sets the word (`prefix`, its old name, is still read) (unset, your Extension's name), `title` and `icon` the entry Item, and `about` and `examples` the intro card the app shows before anything is typed. Each Item is in the search, or in the app only with `"app_only": true`; `in_search = false` in the Manifest puts all of them in the app only. Your Live queries get every query typed in the app, routed as if after your first Live query prefix. The user's `[extensions.<name>]` `word` and `in_search` override yours. The commented `in_search` line in `manifest.toml` puts this example's Items in its app only.

**Saying whether it can do anything.** The search shows your app's entry Item only while your Extension can do something, so nobody enters an app with nothing in it. Until you say otherwise, it can once you have sent Items, or from the start if `manifest.toml` asks for Live queries. When that isn't so, as for an Extension for one compositor running under another, or one that needs a setting the user hasn't made, send `{"status": {"usable": false, "note": "Set a token in [extensions.shell]"}}`, and `{"status": {"usable": true}}` once it can. The `note` is optional; the app's intro card shows it, as `kotodama open --app shell` still enters the app. A usable Extension may send a note too, such as that it is paused. Send a status again after every start: the Daemon forgets it when your Extension stops. In Rust, `kotodama_extension::SentStatus` sends each change once.

**Contributed actions.** Declare them in `manifest.toml` under `[[contributed_actions]]`, with the Item kind they apply to, and `confirm = true` for one the menu asks about first. They arrive as ordinary `run_action` messages carrying the other Extension's Item id, kind and kind field. That is all you ever see of another Extension's Items.

**stdout is the protocol.** Write logs to stderr, and redirect the output of anything you start.
