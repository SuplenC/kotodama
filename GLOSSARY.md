# Kotodama

A command menu for Wayland: one search box that finds and does anything, extended by separately running programs.

## Language

**Daemon**:
The long-running Kotodama process that owns the index, ranking, History and Extensions; every front end talks to it.
_Avoid_: Server, backend, core

**Front end**:
A user interface that shows the Daemon's results and sends the user's input back; the Quickshell module is the default one.
_Avoid_: Client, UI, frontend app

**Extension**:
A separately running program that contributes items, Actions and Views; the sources that ship with Kotodama are Extensions too.
_Avoid_: Plugin, module, provider

**Bundled extension**:
An Extension that ships with Kotodama itself, as opposed to one from elsewhere: its code lives in this repository, and it is a package of its own, versioned and released apart from the core, which a user may leave out.
_Avoid_: Built-in, core extension

**Manifest**:
The file that declares an Extension: what it is, how to start it, and what it contributes. An `[extensions.<name>]` table with a `command`, for an Extension that has no Manifest file, holds the same keys.
_Avoid_: Config, descriptor

**Item**:
One searchable thing an Extension contributes, such as an app, a script, an emoji or a setting.
_Avoid_: Entry, result (for the stored thing), record

**Item kind**:
What sort of thing an Item is, such as `app`, `project` or `window`, which lets other Extensions offer Actions for it.
_Avoid_: Type, category, class

**Identity**:
The name an Extension gives the thing an Item stands for, such as `app:firefox.desktop`, so the same thing offered by several Extensions shares one History.
_Avoid_: Canonical id, dedup key, uid

**Result**:
An Item as it appears for the current query, ranked and highlighted.
_Avoid_: Match, hit, row

**Indexed item**:
An Item the Extension sends ahead of time and keeps up to date, so the Daemon searches it without asking the Extension while the user types.
_Avoid_: Static item, cached item

**Hot item**:
An Indexed item the History predicts the user will want around now, kept whole in memory; every other Indexed item is cold, with only what a search reads in memory and the rest read back when a Result shows it.
_Avoid_: Cached item, preloaded item

**Hidden item**:
An Item an Extension sends ahead of time that the search never shows; only `kotodama open --view` reaches it, to run its primary Action, which usually pushes a View.
_Avoid_: Private item, internal item

**App**:
The search over one Extension's Items alone, which every Extension has. The search shows its entry Item, found by the App's title and its Word; Tab or Enter on it enters the App, where an intro card from the Manifest shows until something is typed; Esc with nothing typed goes back to the whole search. Each Item is in the search, or in its App only. Unlike a Hidden item, an App only Item is still searched, in its App; unlike a View, the search in an App is the search, with Results. A program installed on the desktop is an application, never an App: its Item kind is `app`, and the `apps` Extension's App is the Applications App.
_Avoid_: Prefix, scope, mode, namespace, trigger; app for a desktop application

**Word**:
The one word that names an App, such as `emoji`, `projects` or `calc`: the Manifest's `word`, which the user's `[extensions.<name>] word` overrides, else the Extension's name. An Alias, a Quicklink or another App may hold it first; the App then goes by its Extension's name, or the name and `-2`. Typing it finds the App's entry Item, ranked as a word start; it enters nothing by itself. `kotodama open --app <word>` opens the menu in the App.
_Avoid_: Prefix, keyword (an Item's own), trigger

**Live query**:
A query the Daemon forwards to an Extension as the user types, for sources that can't be indexed ahead of time, such as a calculator or web search.
_Avoid_: Dynamic search, remote query

**Late Result**:
A Result that comes after the list for its query was first sent, such as a Live query's. Until the user moves the selection or 150 ms after the keystroke it reranks the rows on screen; after that it goes below them. Enter runs one only once a frame has shown it.
_Avoid_: Delayed result, async result

**Action**:
Something the user can do to a Result; every Result has one primary Action and may have more, listed in the Action panel.
_Avoid_: Command, handler, verb

**Contributed action**:
An Action one Extension offers for Items of a given Item kind that another Extension owns, such as "Pin a version here" on a project.
_Avoid_: Foreign action, plugin action, extension action

**Action catalog**:
The description of every Action on offer (its title, Item kind, Arguments and Extension) that an Extension may ask to read; it never includes the Items themselves.
_Avoid_: Action registry, action list

**Proposal**:
An Action from the Action catalog that the AI Extension suggests for a request in plain words, with any Arguments it could fill in; nothing runs until the user confirms it.
_Avoid_: Suggestion, AI action, recommendation

**Action panel**:
The list of all Actions available for the selected Result.
_Avoid_: Context menu, actions menu

**Reserved key**:
A key binding the Front end keeps for itself, such as Ctrl+K for the Action panel or Ctrl+Z for undo; the Daemon drops it from any Extension's Action that asks for it.
_Avoid_: System shortcut, global key

**Toast**:
A short notice the Front end shows after Forget or "Clear history", with the key that undoes it; it never takes the keys or the selection, and goes once the change is undone or too old to be.
_Avoid_: Snackbar, notification, popup

**View**:
A screen pushed on top of the search, such as a list, a form, a detail pane or a progress display; Esc pops it once the text is cleared. Typing into a list filters it.
_Avoid_: Page, panel, mode, screen

**Argument**:
A value typed inline after an Item's keyword, such as `1.23` in `go 1.23`, passed to its Action.
_Avoid_: Parameter, input

**Job**:
Long work an Extension reports on while it runs, such as an install; a progress View follows it, it keeps running when the menu closes, and the desktop is notified when it ends.
_Avoid_: Task, operation, process

**History**:
What the Daemon keeps of the user's picks, per Result and per Identity, kept on disk between runs; Frecency and Learned choices are read from it.
_Avoid_: History file, frecency store, usage data

**Frecency**:
The score that combines how often and how recently the user picked a Result, and how usual this time of day is for picking it.
_Avoid_: Popularity, history score

**Learned choice**:
A Result the user repeatedly picked for a given query, which that query, and the queries that extend it or that it extends, then rank first, over the Results it was picked over.
_Avoid_: Pin, favorite

**Empty query**:
The search with nothing typed, and what the menu shows for it: the Results most used at this time of day, then the Fixed menu, or without one the Apps' entry Items.
_Avoid_: Blank query, empty search, home screen

**Fixed menu**:
The Results the user lists in the config for the Empty query to show after the most used ones, in that order.
_Avoid_: Pinned menu, favorites, default list

**Tier**:
How well a Result matches a query, ranked before anything else: exact, initials or word start, inside a word, letters scattered, or a Near miss.
_Avoid_: Match quality, bucket

**Near miss**:
A Result that matches the query only with typos in it, at most one per 4 letters of each word; it ranks below every real match.
_Avoid_: Fuzzy match, typo match

**Fallback**:
An Extension the Daemon asks only what real matches leave rows on screen empty for, such as web search, and whose Results rank below every real match.
_Avoid_: Catch-all, default provider

**Usage log**:
An opt-in record, kept on the user's machine, of the queries typed and the Results picked, for tuning ranking from real use.
_Avoid_: Telemetry, analytics, query log

**Alias**:
A user-defined keyword that stands for an Item or an Action.
_Avoid_: Shortcut, abbreviation

**Quicklink**:
A user-defined, possibly templated, URL or command opened by a keyword, such as `yt <query>`.
_Avoid_: Bookmark, web shortcut

**Opener**:
The program, with its first arguments, that a Quicklink's URL opens with, such as a browser with a profile; `xdg-open` unless the Quicklink or the config names one.
_Avoid_: Browser, handler

**Unavailable Result**:
A Result shown greyed out that can't run, with the reason: an Alias whose Item is gone, or whose Extension is down.
_Avoid_: Broken result, disabled item

**Folder source**:
A Bundled extension configured with a folder whose contents become Items, such as projects or scripts.
_Avoid_: Directory scanner, path provider

**Wire format**:
The encoding the Daemon and one Extension or Front end agree on at the handshake: postcard (Rust peers on the same Protocol version), protobuf, MessagePack or JSON.
_Avoid_: Codec, serialization, protocol (for the encoding alone)

**Problem**:
Something wrong that the user can fix, which the Daemon lists for every Front end and `kotodama check` prints: a config file that doesn't parse, a setting that isn't one or has a bad value, a refused Manifest, a keyword two things ask for, or an Extension that can't start or keeps stopping.
_Avoid_: Error, warning, diagnostic

**Protocol version**:
The version of the messages a peer was built for, `major.minor`, named in its hello. A minor version only adds; the Daemon speaks every version from 1.0 on and sends a peer nothing its version lacks.
_Avoid_: API version, layout version
