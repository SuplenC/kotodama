# 8. The same thing from several Extensions shares one History, by an Identity the Extensions set

Status: accepted (2026-10-19)

An app can come from the apps Extension and from a Folder source, and a web page from bookmarks and from browser history. The History kept picks by Extension and Item id, so a pick of one Result taught the other nothing. Items now carry an optional `identity`, a string naming the thing itself, such as `app:firefox.desktop` or `url:https://example.org`, which every Extension that offers it spells the same way. The History keeps an Item with an Identity under the Identity, with the Extension name `*`, which no Manifest can take, in place of its Extension's. Frecency and Learned choices then follow the thing to every Result of it.

Each Item still shows as a Result of its own: Kotodama doesn't merge them, because each Extension's Item has its own Actions. Each pick is also kept under the picked Item's Extension and id. After ranking, the Results of one Identity swap places among those they ranked in, so the one picked through most takes the best: the thing ranks by its best match, and leads with the user's way to it.

Picks made before an Extension set an Identity, or under an Identity it later changes, stay where they were kept, and count only for that Item's place among its Identity's Results.

Extensions spell Identities with these prefixes, each followed by the thing's name:

- `app:` and the desktop id, for an application (`app:firefox.desktop`).
- `url:` and the URL, for a web page (`url:https://example.org`).
- `folder:` and the absolute path, for a folder, such as a project (`folder:/home/me/Projects/kotodama`).
- `file:` and the absolute path, for a file, such as a script or a `.desktop` file kept as one (`file:/home/me/Scripts/backup.sh`).

An Extension offering a thing of one of these kinds uses its prefix, and a new kind adds its prefix here.

## Considered options

- **The Daemon derives the Identity from the Item kind's field** (`desktop_id`, `path`, `url`). Rejected for now: an app and the `.desktop` file a Folder source lists are different kinds, two Extensions may spell one URL differently, and a namespaced kind has no field at all. Only the Extension knows what its Item stands for.
- **Merge the Results into one.** Rejected: whose title, icon and Actions would it show? Each Extension's Actions differ, and hiding a Result hides them.
- **Order the Results of one Identity by their direct picks in the ranking itself.** Rejected: it only decides a tie, so an exact title from one Extension would still beat the Result the user always picks from another.
- **A new key type in `history.json`.** Rejected: the reserved name keeps the file's shape, so existing files still load.
