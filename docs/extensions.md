# Writing an Extension

An Extension adds things to Kotodama's menu: Items to search, Results computed as you type, Actions to run, Views to show. Any program that reads and writes its stdin and stdout can be one, in any language. This guide covers what an Extension declares, what it says to the Daemon, and what the Daemon promises in return. [protocol.md](protocol.md) has every message with an example.

## Start here

```sh
kotodama extension new notes --lang python    # or shell, or rust
kotodama extension check notes
ln -s "$PWD/notes" ~/.config/kotodama/extensions/notes
```

`new` writes a small Extension that offers one Item and answers one Live query. `check` starts it as the Daemon would, plays the Daemon's part for a moment, and says in plain words what the Daemon would refuse. Run `check` after every change: `--query <text>` types a Live query, and `--run <id>` runs the primary Action of an Item or Live result. Once the directory is linked, the Daemon picks the Extension up without a restart; type `notes hello` in the menu.

Three complete examples show more:

- [examples/shell-extension](../examples/shell-extension): bash and jq. It covers Items from the config, a Contributed action, an Argument, a form, a job with a progress View, and a list View.
- [examples/python-extension](../examples/python-extension): Python's standard library, in JSON. It covers a Live query, two Actions, the config table, and `status`.
- [examples/typescript-extension](../examples/typescript-extension): Node, in protobuf, with a hand-written codec. It covers a Live query and two Actions.

A Rust Extension builds on the `kotodama-extension` crate, as the Bundled extensions do. It does the handshake, reads messages on a thread, sends only the Items that changed, and logs `rejected`. It and `kotodama-protocol` are on crates.io, and their `major.minor` is the protocol version they speak: depend on `"1.0"` of both, as `kotodama extension new --lang rust` does.

The crate's `dbus` module is a small D-Bus client for an Extension that calls a system service now and then, as the system Extension calls logind: `Bus::system()` or `Bus::session()`, `call` with a `Call` whose arguments `Marshal` writes, and the answer's `body()`, an `Unmarshal`. It reads every basic type (`bool`, `byte`, `i16` to `u64`, `f64`, `string` for strings and object paths, `signature`), a variant (`variant` gives the value's signature, then the value is read with the reader it names), and arrays and dict entries (`array`, `align_struct`), so `org.freedesktop.DBus.Properties.Get`'s `v` and `GetAll`'s `a{sv}` can be read; `skip` passes over a value whose type is of no interest. Each reader returns `None` on malformed bytes and never panics.

## Where it lives, and how it runs

An Extension is a directory holding a `manifest.toml` and whatever it runs. The Daemon looks in `$XDG_DATA_DIRS/kotodama/extensions/*/`, which is where a package installs one, and in `~/.config/kotodama/extensions/*/`, where you put your own. An Extension in the second place replaces one of the same name in the first. The user's config can add directories (`[paths] extension_dirs`), switch an Extension off, or override its `command`, `env` and Live query in `[extensions.<name>]`; `kotodama config --schema` lists those keys.

The lifecycle:

1. The Daemon reads the Manifest. It refuses one that doesn't parse, whose trigger isn't a valid regular expression, or that is named `kotodama`, and logs why.
2. It starts `command` from the Extension's directory, with stdin and stdout as pipes and stderr going to the Daemon's log. A relative program path is resolved against the directory and never looked up on `PATH`. So `command = ["python3", "x.py"]` fails: use a script with a `#!` line, made executable.
3. Both sides send a hello within 5 seconds, and settle on a Wire format.
4. The Daemon sends the Extension's config table. The Extension answers with its Items, if it has any, and from then on handles messages as they come.
5. An Extension that exits, or closes its stdout, is restarted. The first wait is 0.1 s, doubling with each crash in a row up to 30 s. One that ran for 10 s before it stopped starts its backoff over. Editing `manifest.toml` restarts the Extension with the new Manifest.
6. When the Daemon exits, the Extension's stdin closes, and the kernel stops the Extension with it. An Extension should exit when reading stdin gives end-of-file.

stdout carries the protocol and nothing else. A stray `print` breaks the connection, so log to stderr and redirect the output of programs you start.

## The Manifest

```toml
name = "notes"                         # required
command = ["./extension.py"]           # required: the program and its arguments
description = "Notes you keep"
word = "notes"                         # the app's word
title = "Notes"                        # the app's name as the user reads it
icon = "accessories-text-editor"       # a freedesktop icon name, or a path
about = "Searches your notes."         # shown on entering the app
examples = [{ query = "notes groceries", result = "The groceries note" }]
commands = ["new-note"]                # Items listed first in the app
in_search = true                       # false: its Items show in its app alone
action_catalog = false                 # true: may read every Action on offer

[live_query]
prefixes = ["notes "]
triggers = ['^#\w+$']
debounce_ms = 0
fallback = false

[[contributed_actions]]
kind = "file"
id = "add-to-notes"
title = "Add to notes"
shortcut = "ctrl+shift+n"
confirm = false
```

- `name` uses lowercase letters, digits and hyphens. It names the Extension everywhere: its config table is `[extensions.<name>]`, and its own Item kinds are `<name>.<kind>`.
- `command` runs from the Extension's directory, as above.
- `word`, `title`, `icon`, `about`, `examples`, `commands` and `in_search` describe its app (see [Apps](#apps-and-where-items-show)).
- `[live_query]` asks for Live queries. It needs at least one of `prefixes`, `triggers` or `fallback` (see [Live queries](#live-queries)).
- `[[contributed_actions]]` offers an Action on every Item of a kind, whichever Extension owns it (see [Actions](#actions)).
- `action_catalog` lets the Extension read the Action catalog, and propose Actions from it (see [Trust](#trust)).

Unknown keys are refused, so a typo shows up in `kotodama extension check`.

## Talking to the Daemon

**Frames.** Every message is one frame: its length in bytes as ASCII digits, a newline, then the message. No newline follows the message.

```
30
{"remove_items":{"ids":["a"]}}
```

**The handshake.** Both sides send their hello at once, always in JSON:

```json
{"version":1,"minor":0,"formats":["json"]}
```

`version` and `minor` are the protocol version the Extension was written for, 1.0 today. The Daemon turns away an Extension that needs a newer protocol than it speaks. An Extension should turn away a Daemon older than the version it needs, and say so on stderr. `formats` lists the Wire formats the Extension reads and writes:

| Format | When to offer it |
|---|---|
| `json` | Required, and enough on its own. |
| `protobuf` | A fast binary format any language has a library for. Generate the types from [`kotodama.proto`](../crates/kotodama-protocol/kotodama.proto), or write the few fields you use by hand, as the TypeScript example does. |
| `msgpack` | MessagePack, if your language has it at hand. |
| `postcard` | Rust only, and only between peers on the same protocol version. `kotodama-extension` offers it for you. |

Both sides use the fastest format they share, in the order postcard, protobuf, msgpack, json. Everything after the hello is in that format.

**Messages.** In JSON a message is an object with one key, its name, holding its fields: `{"add_items": {"items": [...]}}`. A message without fields is its name as a string: `"request_action_catalog"`. Optional fields can be left out. Ignore messages you don't handle: a newer Daemon may send more kinds.

**When the Daemon refuses a message**, it drops that message, keeps the connection, and sends `{"rejected": {"reason": "Item `a` has no Actions; it needs at least its primary Action"}}`. The reason is written for you, the author. Log it to stderr, where it lands in the Daemon's log. Nothing answers it, and an Extension that ignores it loses nothing. The Daemon rejects:

- a message that doesn't decode, such as an Item missing its kind's field, or with no Actions;
- an Item of another Extension's kind;
- `request_action_catalog` without `action_catalog = true`.

It also says when it takes a message but drops part of it, and keeps the rest:

- an Action bound to a key the menu keeps for itself (see [Actions](#actions)): the Action is kept without the binding, and the reason names the Action and the key. A Contributed action bound to one is a problem of the Manifest, listed with the user's problems;
- a Live result marked `answer` from an Extension whose `debounce_ms` isn't 0: it is kept as a plain result.

`kotodama extension check` counts each of these as a problem.

## Items

```json
{"add_items": {"items": [
  {"id": "groceries", "kind": "notes.note", "title": "Groceries", "subtitle": "milk, eggs",
   "keywords": ["shopping"], "actions": [{"id": "open", "title": "Open"}]}
]}}
```

Items are what the search finds as you type. The Daemon indexes them and ranks them with no round trip to the Extension. Fields:

- `id` is unique among the Extension's Items. Sending an id again replaces that Item, which is how an Item is updated. `{"remove_items": {"ids": [...]}}` takes Items away.
- `kind`. A shared kind needs its one field:
  - `app` needs `desktop_id`;
  - `file`, `folder` and `project` need `path`;
  - `window` needs `address`;
  - `url` needs `url`.

  Other Extensions offer Contributed actions by kind, so a file is better as a `file` than as a kind of your own. Any other kind is named `<name>.<kind>`, as in `notes.note`.
- `title`, and optionally `subtitle` and `icon`.
- `keywords`: more words to find the Item by. One of them typed whole ranks as a word of the title would, not as the whole title: an Item that lists an app's name among them ranks below the app until the user picks it more. `keyword`: a word typed before the Item's Arguments, as `timer` in `timer 5`.
- `weak_keywords`: words the Item is still found by, but which never rank it as a title would. The query matches one only as letters together inside a word, and an Item found by them alone stays below every Item whose title starts with the query, however often the user picks it. For a name the user seldom types but may look for, such as the Apps Extension's desktop ids: a Chrome web app's is `chrome-<app id>-Default`, which as a keyword would tie the web app with Chrome itself.
- `actions`: at least one. The first is the primary Action, which Enter runs.
- `identity`: names the thing itself when other Extensions may offer it too, as in `app:firefox.desktop`, `file:/etc/hosts` or `url:https://example.org`. Items with the same identity share their History, so picking one raises the others. Leave it out for anything only your Extension offers.
- `hidden: true` keeps an Item out of the search. Only `kotodama open --view <extension>/<id>` reaches it, which runs its primary Action, usually one that pushes a View. This is how an Extension with only Live queries makes a View reachable from a keybind.
- `app_only: true` shows the Item inside the Extension's app alone.

## Actions

When the user runs an Action, the Extension gets:

```json
{"run_action": {"invocation": 7, "action": "open",
                "item": {"id": "groceries", "kind": "notes.note"}, "arguments": {}}}
```

Answer every `run_action` with `action_done` and the same `invocation`. The menu waits for the answer.

```json
{"action_done": {"invocation": 7, "outcome": "close"}}
```

The outcome:

- `"close"`: done, and the menu closes.
- `"keep_open"`: done, and the menu stays open, as after pushing a View.
- `{"type": {"text": "..."}}`: the menu closes, and the Daemon types the text into the window that had focus.
- `{"paste": {"text": "..."}}`: the same, through the clipboard.
- `{"copy": {"text": "..."}}`: puts it on the clipboard. An image is `{"image": {"mime": "image/png", "data": ...}}`.
- `{"failed": {"message": "..."}}`: the menu stays open and shows the message.

The Daemon does the typing, pasting and copying, so don't call `wtype` or `wl-copy` yourself.

An Action can also carry:

- `arguments`: each with a `name`, an optional `placeholder`, and `optional: true` if it may be left out. With an Item `keyword`, the user types them after it. Each Argument takes one word, and the last takes the rest. The Daemon asks for a missing required one in a form, so `run_action` always has them. An Argument that is a path says so with `kind`: `"file"`, with `accept` listing the file extensions (`"conf"`, without the dot) or MIME types it takes, none for any, or `"folder"`; the default is `"text"`, and any other kind is refused with the Items that carry it. Its form field is then a file field, which a Front end may give a picker, and which the Daemon completes as it is typed (`complete_path`: the entries of the directory typed, or with nothing typed the newest files the field takes from the download, documents and desktop directories, `[files] recent_dirs` in the config), so the Extension does nothing for it. Its value reaches `run_action` as an absolute path: the Daemon expands a leading `~` or `~/` (not `~user`), turns a `file://` URI into its path, and takes a relative path from the home directory, the one a person's paths start from. In a Manifest: `arguments = [{ name = "path", kind = "file", accept = ["conf"], placeholder = "~/Downloads/work.conf" }]`; in Rust, `Argument::new("path").file(["conf"])`.
- `shortcut`: a key binding such as `"ctrl+o"`, which runs the Action without opening the Action panel. It needs Ctrl, Alt or Super, since a bare key types into the search. The menu keeps `ctrl+k`, `ctrl+n`, `ctrl+p`, `ctrl+z` and `ctrl+shift+backspace` for itself, and the Daemon drops an Action's binding to one of them and rejects it (see [Talking to the Daemon](#talking-to-the-daemon)).
- `confirm: true`: for an Action that can't be undone, such as deleting something or powering off. The menu asks in its footer and runs the Action on a second Enter only, so you need no form of your own.

**Withdrawing a pick.** Running an Action counts as picking its Result, which raises it in the ranking. When the run turns out to be a mistake, as when an app the Action launched closed again at once, send `{"withdraw_pick": {"invocation": 7}}` within a minute. The History is then as if the user had never picked it.

**Contributed actions.** An Action under `[[contributed_actions]]` in the Manifest is offered on every Item of that kind, whichever Extension owns it. It arrives as an ordinary `run_action`, carrying the other Item's id, kind and kind field, which is all you ever see of another Extension's Items.

## Live queries

Items suit what can be listed ahead of time. For what depends on the query, such as a calculator, a converter or a web search, the Daemon sends the query to the Extension as a Live query. Which queries it sends depends on `[live_query]`:

- `prefixes`: a query that starts with one is sent, without the prefix. The message's `prefix` field says which prefix it was.
- `triggers`: a query that matches one of these regular expressions (Rust's `regex` syntax) is sent whole, with `prefix` set to `null`.
- `fallback = true`: the Extension is also sent any query whose real matches leave rows on screen empty. Its results, except an answer, rank below every real match. Web search is the example.
- Inside the Extension's app, every query is sent, as if typed after its first prefix.

```json
{"live_query": {"id": 12, "query": "hello world", "prefix": "case "}}
```

Answer with `live_results`, in one batch or several, and mark the last one `done`, even when there are no results:

```json
{"live_results": {"live_query": 12, "done": true, "results": [
  {"item": {"id": "upper:hello world", "kind": "text-case.text", "title": "HELLO WORLD",
            "actions": [{"id": "copy", "title": "Copy"}]}, "score": 1.0}
]}}
```

- `score` runs from 0 to 1: how well the result matches. The Daemon maps it onto the scale it ranks Items on.
- `answer: true` marks the one result that answers the query outright, such as a sum. The Daemon puts it in the top row, and its History learns nothing from it. Only an Extension whose `debounce_ms` is 0 may send one; the Daemon rejects the mark otherwise, and keeps the result.
- `debounce_ms` is how long the query must stay unchanged before it is sent. 0 sends one on every keystroke, which suits work that takes microseconds. Something slow, or something that costs money per request, wants 100 or more.
- `{"cancel_live_query": {"id": 12}}` means the query changed or the menu closed. Stop working on it; anything sent for it afterwards is dropped.
- The Daemon keeps at most 500 results per query.

Results that arrive late never move a row the user is looking at once the selection has moved or 150 ms have passed.

## Views

An Action can push a View over the search: `{"push_view": {"id": "...", "title": "...", "content": ...}}`, answering its `run_action` with `"keep_open"`. The content is one of:

- `list`: Items, which typing filters as the search does. To filter it yourself, as for results from a web service, set `filters_itself: true`. You then get `filter_view` with the whole text on each keystroke, and answer with an `update_view`.
- `form`: fields of `text`, `checkbox`, `select` or `file` input. A `file` input asks for a path, to a folder with `folder: true`; it is completed as a file Argument's field is, and its value arrives as an absolute path, as a file Argument's does. Submitting it sends `submit_form` with the values by field id.
- `detail`: Markdown, with optional metadata beside it.
- `progress`: follows a job.

`update_view` replaces an open View, and `pop_view` closes it. To go from one View to the next, as from a form to its progress, use `replace_view`, so the search never shows in between. Esc on your View sends `view_popped`, which you can take as "cancel". Closing the menu sends nothing, and the View is still there when the menu opens again.

**Jobs.** Report long work with `job_progress`: an `id`, a `title`, a `state` (`running`, `succeeded` or `failed`), and optionally a `progress` from 0 to 1 and a `message`. A job runs on when the menu closes, and the Daemon sends a desktop notification when it finishes.

## Apps, and where Items show

Every Extension is an app. The search shows one entry Item for it, found by its `title` and `word`. Tab or Enter on that Item enters the app, where the query searches the Extension's Items alone, and a chip names the app. Esc goes back. `kotodama open --app <word>` opens the menu in the app. On entering, before anything is typed, the app shows:

- its `about` and `examples`, from the Manifest;
- the Items listed in `commands`;
- then its most used Items, then the rest.

Each Item shows in the search, or in its app alone. `app_only: true` puts one Item in the app alone, and `in_search = false` in the Manifest does it for all of them. The user's own `[extensions.<name>]` table can override `word` and `in_search`.

**Saying whether it can do anything.** The search shows an app's entry Item only while the Extension can do something, so nobody enters an app with nothing in it. An Extension counts as able once it has sent Items, or from the start if it asks for Live queries. One that can't, such as an Extension for one compositor running under another, or one missing a setting, sends:

```json
{"status": {"usable": false, "note": "Set a token in [extensions.notes]"}}
```

It sends `{"status": {"usable": true}}` once it can. Meanwhile it gets no Live queries; once it is usable again, it is sent what is typed. The app's intro card shows the `note`. A usable Extension may send a note too, such as that it is paused. The Daemon forgets the status when the Extension stops, so send it again after every start. In Rust, `kotodama_extension::SentStatus` sends each change once.

## Configuration

The user configures an Extension in any `*.toml` file in `~/.config/kotodama/`:

```toml
[extensions.notes]
folder = "~/Notes"
word = "n"
```

The Daemon sends the table right after the handshake, and again whenever it changes, as `{"config": {"folder": "~/Notes"}}`. It leaves out the keys it reads itself: `enable`, `command`, `env`, `live_query`, `word`, `prefix`, `in_search` and `boost`. Never read config files yourself. A missing table arrives as `{}`, so give every setting a default. Log a setting of the wrong shape, and say what's missing with `status`.

## Compatibility

Protocol 1.0 is frozen at the Kotodama v1 release. From then on:

- **A minor version only adds.** It adds an optional field at the end of a message, a new message, or a new variant, such as a new Item kind. Nothing is removed, renamed, retyped or reordered, and nothing that was valid is refused. An Extension written for 1.0 keeps working with every 1.x Daemon. The Daemon never sends it a message, field or variant that 1.0 lacks.
- **A new major version keeps the old ones.** When 2.0 comes, the Daemon will still speak 1.x to an Extension that asks for it.
- **The handshake says what's wrong.** An Extension that needs a newer protocol than the Daemon speaks is turned away, and the Daemon logs "update Kotodama". An Extension should do the same for a Daemon older than it needs.

So write for the version you test against, ignore what you don't know, and leave optional fields out rather than filling them with placeholders. The Rust crates and `kotodama.proto` are pinned to the protocol version, and a test fails on any change that would break this promise.

## Trust

An Extension is a program running as you, with your files, your network and your session. Kotodama doesn't sandbox it. Install one as you would any other program, from someone you trust.

- **Some Extensions see what you type.** One with a trigger such as `'.*'`, or with `fallback = true`, is sent much of what you type into the menu, including what you type looking for something else. One with only prefixes sees a query only after its prefix, or inside its app.
- **The Action catalog shows every Action to an Extension that asks.** An Extension with `action_catalog = true` reads the title, Item kind and Arguments of every Action on offer, though never the Items. The AI Extension uses this to propose an Action for a request in plain words, and sends the catalog to the model it asks. So your Actions' titles and Arguments may leave the machine; keep secrets out of them. A proposed Action runs only once the user confirms it.
- **A Contributed action sees the Items it runs on.** It gets the id, kind and kind field of another Extension's Item, such as a file's path, when the user runs it, and nothing more.
- **The Daemon checks what an Extension says.** It rejects Items of another Extension's kind, and catalog reads the Manifest didn't ask for. It doesn't limit what the program itself does.
