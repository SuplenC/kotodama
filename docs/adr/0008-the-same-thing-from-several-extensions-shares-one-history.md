# 8. The same thing from several Extensions shares one History, by an identity the Extensions set

Status: accepted (2026-10-19)

An app can come from the apps Extension and from a Folder source, and a web page from bookmarks and from browser history. The History kept picks by Extension and Item id, so a pick through one row taught the other nothing. Items now carry an optional `identity`, a string naming the thing itself, such as `app:firefox.desktop` or `url:https://example.org`, which every Extension that offers it spells the same way. The History keeps an Item with an identity under the identity, with the Extension name `*`, which no Manifest can take, in place of its Extension's. Frecency and Learned choices then follow the thing to every row of it.

Both rows still show: Kotodama doesn't merge them, because each Extension's row has its own Actions. Each pick is also kept under the row's own Extension and id, and that breaks the tie between rows of one identity, so the row the user picks through comes first.

## Considered options

- **The Daemon derives the identity from the Item kind's field** (`desktop_id`, `path`, `url`). Rejected for now: an app and the `.desktop` file a Folder source lists are different kinds, two Extensions may spell one URL differently, and a namespaced kind has no field at all. Only the Extension knows what its Item stands for.
- **Merge the rows into one.** Rejected: whose title, icon and Actions would the row show? Each Extension's Actions differ, and hiding a row hides them.
- **A new key type in `history.json`.** Rejected: the reserved name keeps the file's shape, so existing files still load.
