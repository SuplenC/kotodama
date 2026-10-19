# Kotodama

A command menu for Wayland: one search box that finds and does anything, extended by separately running programs.

## Language

**Daemon**:
The long-running Kotodama process that owns the index, ranking, history and Extensions; every front end talks to it.
_Avoid_: Server, backend, core

**Front end**:
A user interface that shows the Daemon's results and sends the user's input back; the Quickshell module is the default one.
_Avoid_: Client, UI, frontend app

**Extension**:
A separately running program that contributes items, Actions and Views; the sources that ship with Kotodama are Extensions too.
_Avoid_: Plugin, module, provider

**Bundled extension**:
An Extension that ships with Kotodama itself, as opposed to one installed separately.
_Avoid_: Built-in, core extension

**Manifest**:
The file that declares an Extension: what it is, how to start it, and what it contributes.
_Avoid_: Config, descriptor

**Item**:
One searchable thing an Extension contributes, such as an app, a script, an emoji or a setting.
_Avoid_: Entry, result (for the stored thing), record

**Item kind**:
What sort of thing an Item is, such as `app`, `project` or `window`, which lets other Extensions offer Actions for it.
_Avoid_: Type, category, class

**Result**:
An Item as it appears for the current query, ranked and highlighted.
_Avoid_: Match, hit, row

**Indexed item**:
An Item the Extension sends ahead of time and keeps up to date, so the Daemon searches it without asking the Extension while the user types.
_Avoid_: Static item, cached item

**Live query**:
A query the Daemon forwards to an Extension as the user types, for sources that can't be indexed ahead of time, such as a calculator or web search.
_Avoid_: Dynamic search, remote query

**Action**:
Something the user can do to a Result; every Result has one primary Action and may have more, listed in the Action panel.
_Avoid_: Command, handler, verb

**Contributed action**:
An Action one Extension offers for Items of a given Item kind that another Extension owns, such as "Pin a version here" on a project.
_Avoid_: Foreign action, plugin action, extension action

**Action catalog**:
The description of every Action on offer (its title, Item kind, Arguments and Extension) that an Extension may ask to read; it never includes the Items themselves.
_Avoid_: Action registry, action list

**Action panel**:
The list of all Actions available for the selected Result.
_Avoid_: Context menu, actions menu

**View**:
A screen pushed on top of the search, such as a list, a form, a detail pane or a progress display; Esc pops it.
_Avoid_: Page, panel, mode, screen

**Argument**:
A value typed inline after an Item's keyword, such as `1.23` in `go 1.23`, passed to its Action.
_Avoid_: Parameter, input

**Frecency**:
The score that combines how often and how recently the user picked a Result.
_Avoid_: Popularity, history score

**Learned choice**:
A Result the user repeatedly picked for a given query, which that query then ranks first.
_Avoid_: Pin, favorite

**Alias**:
A user-defined keyword that stands for an Item or an Action.
_Avoid_: Shortcut, abbreviation

**Quicklink**:
A user-defined, possibly templated, URL or command opened by a keyword, such as `yt <query>`.
_Avoid_: Bookmark, web shortcut

**Folder source**:
A Bundled extension configured with a folder whose contents become Items, such as projects or scripts.
_Avoid_: Directory scanner, path provider

**Wire format**:
The encoding the Daemon and one Extension or Front end agree on at the handshake: JSON, MessagePack or postcard.
_Avoid_: Codec, serialization, protocol (for the encoding alone)
