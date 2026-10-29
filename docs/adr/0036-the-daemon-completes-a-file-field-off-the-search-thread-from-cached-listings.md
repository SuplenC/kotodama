# 36. The Daemon completes a file field off the search thread, from cached listings

Status: accepted (2026-10-29)

A form can ask for a path (`FormInput::File`), and an Extension's Argument can be a file or a folder. A person typing a path wants the entries that complete it, and before typing anything, the file they most likely mean: the one they just downloaded. Each Front end could walk the file system itself, but then every Front end would need its own listing, matching and ranking, and they would rank paths differently from how Kotodama ranks everything else. The Daemon already has the matcher and the Tiers, so it does the completion. The constraint is the one every feature here has: the search's keystroke latency and the Index's memory must not grow for it.

## Messages

Both are in protocol 1.0, added in place, as `weak_keywords` and `confirm` were, since 1.0 isn't released.

- **`FromFrontEnd::CompletePath { extension, view, field, text }`**: the form View (by its Extension and id, `kotodama` for the Daemon's own Argument form), the file field by id, and what is typed in it. The Daemon looks the field up itself, so its `accept` and `folder` come from the View, not from the Front end. A request for a field that isn't a file field of an open form, or for an Argument form another Front end shows, gets no answer.
- **`ToFrontEnd::PathCompletions { extension, view, field, text, entries }`**: the answer, naming the request it answers, so a Front end drops one for text no longer typed. Each `PathCompletion` is `{ path, title, folder, modified, highlights }`. `path` is absolute, and a folder's has no trailing `/`. `modified` is in Unix seconds, or `None`. `highlights` are the title's matched characters, as a Result's are.

There is no request id. The field and the text already say what an answer is for, and a Front end only shows the answer to what is typed now.

## What it completes

- **A partial path.** The text up to its last `/` names a directory, and the rest is matched against the directory's entries. `~` and `~/` are the home directory, and a relative path starts there, as file Arguments do. A folder field offers folders only. A file field offers folders, since a person may still be walking towards the file, and the files `accept` takes. Hidden entries are offered only once the typed part starts with a dot. Entries are ranked by the search's own Tiers and nucleo's score, so `wor` puts `work.conf` above `network`. Within a tier, folders come before files, then the higher score. With nothing typed after the `/`, it is folders first, then names in order. The answer holds at most `MAX_COMPLETIONS` (50).
- **Nothing typed.** The newest files the field takes from `[files] recent_dirs`, at most `MAX_RECENT` (20), each with its modified time. A folder field gets the newest folders there. The default `recent_dirs` are the XDG user directories for downloads, documents and the desktop: read from `$XDG_CONFIG_HOME/user-dirs.dirs`, else `~/Downloads`, `~/Documents` and `~/Desktop`. A directory set to the home directory itself is switched off, as `xdg-user-dirs-update` writes it. Only the directories themselves are read, not their subdirectories: a recursive walk would cost a read per subdirectory on the first request, for files that are seldom the newest.
- **`accept`.** An extension (`conf`, `tar.gz`) matches the end of a name, ignoring case. A MIME type (`application/pdf`, or `image/*`) matches the extensions the shared MIME-info database's `globs2` files give it, from `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`. Every freedesktop system has that database, and it is not specific to any distro. A MIME type nothing is known by takes nothing, rather than everything.

## Off the search thread

- **The `completion` thread.** The Daemon hands each request to a thread of its own and goes back to its events. The thread answers with `Event::Completed`, which the Daemon passes on to the Front end. Requests that queue up while it works are coalesced: of several for the same field, only the last is answered. Until `kotodamad` starts the thread, and in tests, a request is answered as it is handled, so the state machine stays testable without threads.
- **Cached listings.** The thread keeps the last `CACHED_DIRS` (8) directories it listed: each entry's name, kind, modified time, and its name folded and split into words as the matcher reads it. It reads at most `MAX_LISTED` (4,096) entries of one directory. Each request checks a kept listing with one `stat` of the directory. An entry added, removed or renamed changes the directory's modified time, and the directory is then read again. A file rewritten in place doesn't change it, so its modified time, and its place among the newest, can be stale until the directory changes. A file watcher per cached directory was the alternative. It would answer that case, but it costs an inotify watch and a thread's attention per directory for a field that is open for seconds, and `stat` is one syscall.
- **Memory.** A listing costs about 150 bytes an entry, so at most a few MB across 8 directories of thousands of entries, and typically a few KB. It lives on the completion thread, never in the Index. An Item without file Arguments pays nothing. An `ArgumentForm` keeps the file fields of the form it shows, and only while it is open.

## Cost

Criterion, `kotodamad/benches/completion.rs`, on a directory of 1,000 entries:

| Bench | Time |
|---|---|
| a keystroke (`~/files/wor`, the listing kept) | 34 µs |
| the whole directory (`~/files/`) | 80 µs |
| the first keystroke, which reads the directory | 1.1 to 3.4 ms, as the disk cache has it |

The search's keystroke path doesn't change. `scripts/bench-instructions`, base (the file input, without completion) against this change:

| Bench | Base | This change |
|---|---|---|
| first keystroke | 2,430,994 | 2,431,127 (+0.01%) |
| next keystroke | 1,438,243 | 1,437,953 (−0.02%) |
| two words pasted | 2,938,336 | 2,939,194 (+0.03%) |
| Empty query, 2k picked | 34,595 | 34,600 (+0.01%) |
| emoji in the root search | 3,766,157 | 3,766,154 (−0.00%) |
| emoji, app scoped out | 2,425,716 | 2,425,715 (−0.00%) |
| emoji, app searched | 2,099,292 | 2,099,228 (−0.00%) |

The differences are code layout: nothing on the keystroke path changed.

## Considered options

- **Completion in each Front end.** Rejected: every Front end would walk the file system and rank paths its own way, and a Front end in QML has no matcher.
- **Answering on the Daemon's event thread.** Rejected: reading a directory of thousands of entries takes milliseconds, which every menu's search would wait behind.
- **A request id in the messages.** Rejected: the field and the text say what an answer is for. The Front end needs to compare the text anyway, to tell whether the answer is still wanted.
- **Folders always before files.** Rejected: a folder whose name only scatters the typed letters (`network` for `wo`) would sit above the file whose name starts with them. Tiers first, then folders first within a tier.
- **Recursive recent files.** Rejected for the cost above; `recent_dirs` can name subdirectories.
- **A watcher per cached directory.** Rejected for the cost above.
