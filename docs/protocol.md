<!-- Written by crates/kotodama-protocol/tests/protocol_doc.rs from tests/layouts/1.0.txt; don't edit. Rerun it with KOTODAMA_BLESS_LAYOUT=1 after the layout changes. -->

# The Kotodama protocol 1.0, message by message

Every message of protocol 1.0, as JSON, with a sample of each. The samples are the ones pinned for this version, so they are exactly what the Daemon sends and reads. Each sets every field, even the optional ones. [extensions.md](extensions.md) is the guide to writing an Extension; this is the reference beside it.

- A message is an object with one key, its name, holding its fields: `{"remove_items": {"ids": ["a"]}}`. A message without fields is its name as a string: `"request_action_catalog"`.
- Each message travels in one frame: its length in bytes as decimal digits, a newline, then the message.
- The same messages in Protocol Buffers are in [`kotodama.proto`](../crates/kotodama-protocol/kotodama.proto), MessagePack has the JSON's shape, and postcard is for Rust peers on the Daemon's own version.

## From the Daemon to an Extension: `ToExtension`

What an Extension reads. Ignore a message you don't handle: a newer Daemon may send more, though never a message or a field newer than the version your hello names.

### `config`

The Extension's `[extensions.<name>]` table from the user's config, the first message after the handshake and again whenever the config changes. Never read config files yourself.

```json
{
  "config": {
    "enabled": true,
    "limit": 7
  }
}
```

### `live_query`

A query for an Extension whose Manifest asks for Live queries, once it has been still for the Manifest's `debounce_ms`. `query` is the text without the `prefix` that routed it here; `prefix` is `null` for a trigger or a fallback. Answer with `live_results` naming the same `id`.

```json
{
  "live_query": {
    "id": 3,
    "query": "1+1",
    "prefix": "="
  }
}
```

### `cancel_live_query`

The query changed or the menu closed: results for Live query `id` are no longer wanted. Stop work on it; results sent for it anyway are dropped.

```json
{
  "cancel_live_query": {
    "id": 4
  }
}
```

### `run_action`

Runs one of the Extension's Actions on an Item: its own, or another Extension's for a Contributed action, of which `item` is all it sees. `arguments` holds the Arguments by name. Answer with `action_done` and the same `invocation`.

```json
{
  "run_action": {
    "invocation": 5,
    "action": "open",
    "item": {
      "id": "id",
      "kind": "project",
      "desktop_id": null,
      "path": "/a/project",
      "address": null,
      "url": null
    },
    "arguments": {
      "version": "1.23"
    }
  }
}
```

### `submit_form`

The user submitted a form View the Extension pushed: the values by field id.

```json
{
  "submit_form": {
    "view": "view",
    "values": {
      "enabled": true,
      "limit": 7
    }
  }
}
```

### `view_popped`

The user left one of the Extension's Views, as with Esc. Closing the menu never sends it, so it may mean "cancel".

```json
{
  "view_popped": {
    "view": "view"
  }
}
```

### `filter_view`

The whole filter typed into a list View that has `filters_itself`, empty once cleared. Answer with an `update_view` holding the Items that match.

```json
{
  "filter_view": {
    "view": "view",
    "text": "fir"
  }
}
```

### `action_catalog`

Every Action on offer, without any Item, for an Extension whose Manifest has `action_catalog = true`, after it sent `request_action_catalog` and again whenever the catalog changes.

```json
{
  "action_catalog": {
    "entries": [
      {
        "extension": "shell",
        "kind": "project",
        "action": {
          "id": "open",
          "title": "Open",
          "shortcut": "ctrl+o",
          "arguments": [
            {
              "name": "version",
              "placeholder": "1.23",
              "optional": true
            }
          ],
          "confirm": true
        }
      }
    ]
  }
}
```

### `rejected`

The Daemon refused something the Extension sent, and says why in words for its author: a message that doesn't decode, an Item of another Extension's kind, a request it may not make. What was refused is dropped and the connection goes on. Nothing answers it; log it to stderr.

```json
{
  "rejected": {
    "reason": "Item `a` has no Actions"
  }
}
```

## From an Extension to the Daemon: `FromExtension`

What an Extension writes. An optional field can be left out; [extensions.md](extensions.md) says which fields each message needs.

### `add_items`

Indexes Items. An Item whose id was sent before replaces the old one, which is how an Item is updated.

```json
{
  "add_items": {
    "items": [
      {
        "id": "id",
        "kind": "app",
        "desktop_id": "firefox.desktop",
        "path": null,
        "address": null,
        "url": null,
        "identity": "app:firefox.desktop",
        "title": "Title",
        "subtitle": "Subtitle",
        "icon": "icon",
        "keyword": "go",
        "keywords": [
          "browser"
        ],
        "actions": [
          {
            "id": "open",
            "title": "Open",
            "shortcut": "ctrl+o",
            "arguments": [
              {
                "name": "version",
                "placeholder": "1.23",
                "optional": true
              }
            ],
            "confirm": true
          }
        ],
        "hidden": true,
        "app_only": true,
        "weak_keywords": [
          "org.mozilla.firefox"
        ]
      }
    ]
  }
}
```

### `remove_items`

Takes the Items with these ids out of the index.

```json
{
  "remove_items": {
    "ids": [
      "a",
      "b"
    ]
  }
}
```

### `live_results`

Results for Live query `live_query`, in as many batches as you like; `done` says none follow. `score` from 0 to 1 says how well each matches. `answer` marks the one Result that answers the query outright, such as a sum, which takes the top row; only an Extension whose `debounce_ms` is 0 may send one.

```json
{
  "live_results": {
    "live_query": 3,
    "results": [
      {
        "item": {
          "id": "id",
          "kind": "url",
          "desktop_id": null,
          "path": null,
          "address": null,
          "url": "https://example.org",
          "identity": "app:firefox.desktop",
          "title": "Title",
          "subtitle": "Subtitle",
          "icon": "icon",
          "keyword": "go",
          "keywords": [
            "browser"
          ],
          "actions": [
            {
              "id": "open",
              "title": "Open",
              "shortcut": "ctrl+o",
              "arguments": [
                {
                  "name": "version",
                  "placeholder": "1.23",
                  "optional": true
                }
              ],
              "confirm": true
            }
          ],
          "hidden": true,
          "app_only": true,
          "weak_keywords": [
            "org.mozilla.firefox"
          ]
        },
        "score": 0.75,
        "answer": true
      }
    ],
    "done": true
  }
}
```

### `action_done`

Answers `run_action` with the same `invocation` and how it finished. Send it for every run, soon: the menu waits for it.

```json
{
  "action_done": {
    "invocation": 5,
    "outcome": "keep_open"
  }
}
```

### `push_view`

Puts a View on top of the menu.

```json
{
  "push_view": {
    "id": "view",
    "title": "A View",
    "content": {
      "progress": {
        "job": "job"
      }
    }
  }
}
```

### `update_view`

Replaces the open View with the same id.

```json
{
  "update_view": {
    "id": "view",
    "title": "A View",
    "content": {
      "progress": {
        "job": "job"
      }
    }
  }
}
```

### `pop_view`

Takes the View with this id away.

```json
{
  "pop_view": {
    "view": "view"
  }
}
```

### `replace_view`

Pops the open View `view` and pushes `with` in one step, as a form gives way to its progress, so the search never shows in between. Pushes `with` alone if `view` isn't open.

```json
{
  "replace_view": {
    "view": "form",
    "with": {
      "id": "view",
      "title": "A View",
      "content": {
        "progress": {
          "job": "job"
        }
      }
    }
  }
}
```

### `job_progress`

Reports a job's whole state on each change: `progress` from 0 to 1 if known, and a `message`. A job outlives the menu, and the Daemon sends a desktop notification when it ends.

```json
{
  "job_progress": {
    "id": "job",
    "title": "Installing",
    "state": "failed",
    "progress": 0.75,
    "message": "out of space"
  }
}
```

### `request_action_catalog`

Asks for `action_catalog`; allowed only with `action_catalog = true` in the Manifest.

```json
"request_action_catalog"
```

### `withdraw_pick`

Withdraws the pick that running an Action made, as when the app it launched closed right away: the History becomes as if the user never picked it. Only for the Extension's own run, within a minute of `run_action`.

```json
{
  "withdraw_pick": {
    "invocation": 5
  }
}
```

### `status`

Whether the Extension can do anything now, sent whenever that changes. The search shows its app's entry Item only while it can. `note`, shown on the app's intro card, says why it can't, or what the user should know. Send it again after every start.

```json
{
  "status": {
    "usable": true,
    "note": "Paused for an hour"
  }
}
```

## Outcomes: `Outcome`

How an Action finished: the `outcome` of `action_done`.

### `close`

Done; the menu closes.

```json
"close"
```

### `keep_open`

Done; the menu stays open, as after pushing a View.

```json
"keep_open"
```

### `type`

The menu closes and the Daemon types `text` into the window that had focus.

```json
{
  "type": {
    "text": "typed"
  }
}
```

### `paste`

The menu closes and the Daemon pastes into the window that had focus.

```json
{
  "paste": {
    "text": "pasted"
  }
}
```

### `copy`

The menu closes and the Daemon puts this on the clipboard, without pasting.

```json
{
  "copy": {
    "text": "copied"
  }
}
```

### `failed`

The Action failed; the menu stays open and shows `message`.

```json
{
  "failed": {
    "message": "no"
  }
}
```

### `run_catalog_action`

Runs an Action from the Action catalog, as if the user had picked it; only for an Extension whose Manifest reads the catalog, as the outcome of an Action the user ran.

```json
{
  "run_catalog_action": {
    "extension": "kubaos",
    "kind": "kubaos.suite",
    "action": "new-project",
    "arguments": {
      "name": "demo"
    }
  }
}
```

## Clipboard contents: `Clipboard`

What `paste` and `copy` put on the clipboard.

### `text`

Text.

```json
{
  "text": "text"
}
```

### `image`

An image: its MIME type, and its bytes, base64 in JSON and MessagePack.

```json
{
  "image": {
    "mime": "image/png",
    "data": "iVA="
  }
}
```

## Items, by kind: `Item`

An Item is flat: its kind's one field sits beside `kind`. `id`, `kind`, `title` and at least one Action are required; the shared kinds also need their field. Leave out the fields of other kinds, or send them as `null`.

### `App`

An application, by its desktop file id.

```json
{
  "id": "id",
  "kind": "app",
  "desktop_id": "firefox.desktop",
  "path": null,
  "address": null,
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `File`

A file, by its absolute path.

```json
{
  "id": "id",
  "kind": "file",
  "desktop_id": null,
  "path": "/a/file",
  "address": null,
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `Folder`

A folder, by its absolute path.

```json
{
  "id": "id",
  "kind": "folder",
  "desktop_id": null,
  "path": "/a/folder",
  "address": null,
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `Project`

A project's folder, by its absolute path.

```json
{
  "id": "id",
  "kind": "project",
  "desktop_id": null,
  "path": "/a/project",
  "address": null,
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `Window`

An open window, by the compositor's address for it.

```json
{
  "id": "id",
  "kind": "window",
  "desktop_id": null,
  "path": null,
  "address": "0x1",
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `Url`

A web address.

```json
{
  "id": "id",
  "kind": "url",
  "desktop_id": null,
  "path": null,
  "address": null,
  "url": "https://example.org",
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

### `Namespaced`

Any other kind belongs to one Extension and is named `<extension>.<kind>`, without a field of its own.

```json
{
  "id": "id",
  "kind": "kubaos.suite",
  "desktop_id": null,
  "path": null,
  "address": null,
  "url": null,
  "identity": "app:firefox.desktop",
  "title": "Title",
  "subtitle": "Subtitle",
  "icon": "icon",
  "keyword": "go",
  "keywords": [
    "browser"
  ],
  "actions": [
    {
      "id": "open",
      "title": "Open",
      "shortcut": "ctrl+o",
      "arguments": [
        {
          "name": "version",
          "placeholder": "1.23",
          "optional": true
        }
      ],
      "confirm": true
    }
  ],
  "hidden": true,
  "app_only": true,
  "weak_keywords": [
    "org.mozilla.firefox"
  ]
}
```

## View contents: `ViewContent`

The `content` of a View an Extension pushes, updates or replaces.

### `list`

Items to pick from, whose Actions run like a Result's. Typing filters them in the Daemon, unless `filters_itself` is set: then the Extension gets `filter_view`.

```json
{
  "list": {
    "items": [
      {
        "id": "id",
        "kind": "file",
        "desktop_id": null,
        "path": "/a/file",
        "address": null,
        "url": null,
        "identity": "app:firefox.desktop",
        "title": "Title",
        "subtitle": "Subtitle",
        "icon": "icon",
        "keyword": "go",
        "keywords": [
          "browser"
        ],
        "actions": [
          {
            "id": "open",
            "title": "Open",
            "shortcut": "ctrl+o",
            "arguments": [
              {
                "name": "version",
                "placeholder": "1.23",
                "optional": true
              }
            ],
            "confirm": true
          }
        ],
        "hidden": true,
        "app_only": true,
        "weak_keywords": [
          "org.mozilla.firefox"
        ]
      }
    ],
    "filters_itself": true
  }
}
```

### `form`

Fields to fill in, and the submit button's label; submitting sends `submit_form`.

```json
{
  "form": {
    "fields": [
      {
        "id": "name",
        "label": "Name",
        "input": {
          "checkbox": {
            "default": true
          }
        }
      }
    ],
    "submit": "Go"
  }
}
```

### `detail`

Markdown, with label and value pairs beside it.

```json
{
  "detail": {
    "markdown": "# Hi",
    "metadata": [
      {
        "label": "Size",
        "value": "3 MB"
      }
    ]
  }
}
```

### `progress`

Follows the job with this id as the Extension reports it.

```json
{
  "progress": {
    "job": "job"
  }
}
```

## Form inputs: `FormInput`

The `input` of a form field, which also decides the type of its submitted value.

### `text`

A line of text; `secret` hides what is typed. Submitted as a string.

```json
{
  "text": {
    "placeholder": "name",
    "default": "kuba",
    "secret": true
  }
}
```

### `checkbox`

A checkbox. Submitted as a boolean.

```json
{
  "checkbox": {
    "default": true
  }
}
```

### `select`

One of a list of options. Submitted as the option's `value`.

```json
{
  "select": {
    "options": [
      {
        "value": "a",
        "label": "A"
      }
    ],
    "default": "a"
  }
}
```

## Job states: `JobState`

The `state` of a job an Extension reports with `job_progress`.

### `running`

Still going.

```json
"running"
```

### `succeeded`

Done.

```json
"succeeded"
```

### `failed`

Gave up; the `message` says why.

```json
"failed"
```

## Config values: `Value`

What a config table and a submitted form hold: anything TOML can hold, untagged in JSON. Dates arrive as strings.

### `Bool`

A boolean.

```json
true
```

### `Integer`

A whole number.

```json
-40
```

### `Float`

A number with a fraction.

```json
0.5
```

### `String`

A string.

```json
"hi"
```

### `Array`

A list of values.

```json
[
  1,
  "two"
]
```

### `Table`

A table of values by key.

```json
{
  "enabled": true,
  "limit": 7
}
```

## From a Front end to the Daemon: `FromFrontEnd`

Extensions never see these; they are here for whoever writes a Front end.

### `query`

The search text changed.

```json
{
  "query": {
    "text": "fir"
  }
}
```

### `viewport`

Which Results are on screen: `rows` of them from `top`.

```json
{
  "viewport": {
    "top": 2,
    "rows": 9
  }
}
```

### `selection_moved`

The user moved the selection off the top row.

```json
"selection_moved"
```

### `run_action`

Runs an Action of a Result, naming the `Results` version on screen when the user chose it.

```json
{
  "run_action": {
    "result": {
      "extension": "apps",
      "id": "firefox"
    },
    "shown": 7,
    "action": {
      "extension": "shell",
      "id": "open"
    },
    "arguments": {
      "version": "1.23"
    }
  }
}
```

### `submit_form`

The user submitted a form View.

```json
{
  "submit_form": {
    "extension": "shell",
    "view": "view",
    "values": {
      "enabled": true,
      "limit": 7
    }
  }
}
```

### `view_popped`

The user left a View, as with Esc.

```json
{
  "view_popped": {
    "extension": "shell",
    "view": "view"
  }
}
```

### `filter_view`

The user typed into a list View to filter it.

```json
{
  "filter_view": {
    "extension": "shell",
    "view": "view",
    "text": "fir"
  }
}
```

### `closed`

Answers `close`: the window is gone and focus is back where the user was.

```json
"closed"
```

### `open`

Asks a menu to open, as `kotodama open` does.

```json
{
  "open": {
    "query": "fir",
    "view": "view"
  }
}
```

### `toggle`

Asks a menu to open or close, as `kotodama toggle` does.

```json
"toggle"
```

### `undo`

Undoes the change `offer_undo` offered.

```json
{
  "undo": {
    "change": 7
  }
}
```

### `shown`

The menu became visible.

```json
"shown"
```

## From the Daemon to a Front end: `ToFrontEnd`

Extensions never see these either.

### `results`

The whole ranked list for the query, best first, and the app it is in.

```json
{
  "results": {
    "version": 7,
    "results": [
      {
        "extension": "apps",
        "item": {
          "id": "id",
          "kind": "app",
          "desktop_id": "firefox.desktop",
          "path": null,
          "address": null,
          "url": null,
          "identity": "app:firefox.desktop",
          "title": "Title",
          "subtitle": "Subtitle",
          "icon": "icon",
          "keyword": "go",
          "keywords": [
            "browser"
          ],
          "actions": [
            {
              "id": "open",
              "title": "Open",
              "shortcut": "ctrl+o",
              "arguments": [
                {
                  "name": "version",
                  "placeholder": "1.23",
                  "optional": true
                }
              ],
              "confirm": true
            }
          ],
          "hidden": true,
          "app_only": true,
          "weak_keywords": [
            "org.mozilla.firefox"
          ]
        },
        "contributed": [
          {
            "extension": "shell",
            "action": {
              "id": "open",
              "title": "Open",
              "shortcut": "ctrl+o",
              "arguments": [
                {
                  "name": "version",
                  "placeholder": "1.23",
                  "optional": true
                }
              ],
              "confirm": true
            }
          }
        ],
        "highlights": [
          0,
          2
        ],
        "alias": "ff",
        "unavailable": "apps isn't running"
      }
    ],
    "scope": {
      "word": "emoji",
      "title": "Emoji",
      "icon": "face-smile",
      "intro": {
        "about": "Emoji and symbols, typed where you were.",
        "examples": [
          {
            "query": "fire",
            "result": "🔥"
          }
        ],
        "note": "Paused for an hour"
      }
    }
  }
}
```

### `status`

The Extensions that are down, and the problems the user can fix.

```json
{
  "status": {
    "down": [
      "shell"
    ],
    "problems": [
      {
        "about": "history.half_life_days in ~/.config/kotodama/config.toml",
        "message": "should be a number of days above 0"
      }
    ]
  }
}
```

### `close`

Close the menu, then answer `closed`.

```json
"close"
```

### `action_failed`

An Action failed; show `message` and stay open.

```json
{
  "action_failed": {
    "message": "no"
  }
}
```

### `push_view`

Push an Extension's View, with the Contributed actions on its Items.

```json
{
  "push_view": {
    "extension": "shell",
    "view": {
      "id": "view",
      "title": "A View",
      "content": {
        "progress": {
          "job": "job"
        }
      }
    },
    "contributed": {
      "file": [
        {
          "extension": "pinner",
          "action": {
            "id": "open",
            "title": "Open",
            "shortcut": "ctrl+o",
            "arguments": [
              {
                "name": "version",
                "placeholder": "1.23",
                "optional": true
              }
            ],
            "confirm": true
          }
        }
      ]
    }
  }
}
```

### `update_view`

Replace the open View with the same id.

```json
{
  "update_view": {
    "extension": "shell",
    "view": {
      "id": "view",
      "title": "A View",
      "content": {
        "progress": {
          "job": "job"
        }
      }
    },
    "contributed": {
      "file": [
        {
          "extension": "pinner",
          "action": {
            "id": "open",
            "title": "Open",
            "shortcut": "ctrl+o",
            "arguments": [
              {
                "name": "version",
                "placeholder": "1.23",
                "optional": true
              }
            ],
            "confirm": true
          }
        }
      ]
    }
  }
}
```

### `replace_view`

Pop one View and push another in one step.

```json
{
  "replace_view": {
    "extension": "shell",
    "view": "form",
    "with": {
      "id": "view",
      "title": "A View",
      "content": {
        "progress": {
          "job": "job"
        }
      }
    },
    "contributed": {
      "file": [
        {
          "extension": "pinner",
          "action": {
            "id": "open",
            "title": "Open",
            "shortcut": "ctrl+o",
            "arguments": [
              {
                "name": "version",
                "placeholder": "1.23",
                "optional": true
              }
            ],
            "confirm": true
          }
        }
      ]
    }
  }
}
```

### `filtered_view`

The Items of a list View that match its filter, best first.

```json
{
  "filtered_view": {
    "extension": "shell",
    "view": "view",
    "text": "fir",
    "matches": [
      {
        "item": 3,
        "highlights": [
          0,
          2
        ]
      }
    ]
  }
}
```

### `pop_view`

Take a View away.

```json
{
  "pop_view": {
    "extension": "shell",
    "view": "view"
  }
}
```

### `job_progress`

A job's state, for its progress View.

```json
{
  "job_progress": {
    "extension": "shell",
    "job": {
      "id": "job",
      "title": "Installing",
      "state": "failed",
      "progress": 0.75,
      "message": "out of space"
    }
  }
}
```

### `open`

Open the menu, as another Front end asked.

```json
{
  "open": {
    "query": "fir",
    "view": "view"
  }
}
```

### `toggle`

Open the menu if it is closed, else close it.

```json
"toggle"
```

### `offer_undo`

The History changed and can be undone: show `title` on the Toast.

```json
{
  "offer_undo": {
    "change": 7,
    "title": "Forgot “Firefox”"
  }
}
```

### `withdraw_undo`

The change can't be undone any more: hide its offer.

```json
{
  "withdraw_undo": {
    "change": 7
  }
}
```

### `set_query`

Replace the search text, as on entering an app.

```json
{
  "set_query": {
    "text": "emoji "
  }
}
```

### `reached`

How many menus an `open` or `toggle` reached.

```json
{
  "reached": {
    "menus": 1
  }
}
```
