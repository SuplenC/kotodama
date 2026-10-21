# 27. Every Extension is an app, and each Item is in the search or in the app alone

Status: accepted (2026-10-21). Supersedes ADR 0024. Amended (2026-10-21): see [Amendments](#amendments).

ADR 0024 put one Extension's Items behind a Prefix. That was all or nothing: the emoji were all behind `emoji`, and the apps all in the search. Some Extensions have Items of both sorts. Hyprland's windows belong in the search, but its binds, one per key combination, crowd it. Clipboard clips are private and noisy, while the History View should stay reachable. The owner asked for Raycast's model: every Extension is an app. You enter it from the search, it introduces itself, and it searches its own Items. Each Item is in the search, or in its app only.

## What it does

- **Every Extension has an app.** The app's word is the config's `prefix`, else the Manifest's `prefix`, else the Extension's name. A word another app already has falls back to the name. If the name is taken too, the Extension has no app. The search shows one entry Item for each app, `prefix:<extension>` of kind `kotodama.app` (`APP_ENTRY_KIND`). It is found by the app's title and word, as a keyword, and ranked like any other Result.
- **Each Item is in the search, or in its app only.** The Item says which with `app_only`. The config's `[extensions.<name>] in_search` overrides it for the whole Extension: `true` puts every Item in the search, `false` puts none there. The Manifest's `in_search` does the same, below the config. The Bundled defaults:
  - emoji and clipboard have `in_search = false`;
  - Hyprland marks its binds and its "Keyboard Shortcuts" Item `app_only`;
  - every other Extension keeps its Items in the search.
- **Entering an app is explicit.** Tab or Enter on the entry Item enters it. So does `FromFrontEnd::Open` with a query that starts with the word, which is what `kotodama open --prefix` sends. The Daemon remembers per session which app was entered, then sends `SetQuery` with the word and a space, and Results with the app's `Scope` in the same flush. A query that starts with the word but was typed in the search is just a query. `system monitor` searches for a system monitor; it never enters the System app. That matters more now than with ADR 0024's Prefixes, since every Extension has a word and words such as `system`, `web` and `apps` are common.
- **The intro card is in the first frame.** `Scope` carries `intro`: the Manifest's `about` and its `examples`, each a query and what it gives. It is set only while nothing is typed past the word. The Daemon has read the Manifest already, so showing the card needs no round trip to the Extension. Under it, in order:
  - the app's `commands`, Items of its own named in the Manifest, such as the clipboard's History;
  - then its most used;
  - then the rest, in the order the Extension sent them.
- **Typing in an app searches its Items, both sorts.** An Extension's Live queries get every query typed in its app, whether its triggers match or not, as if routed by its first Live query prefix. So the calculator evaluates anything typed in it, and the AI app asks without a `?`. Live queries and Fallbacks are asked in the search too, unless the Extension's search-side Segment is turned off (`in_search = false`).
- **Esc in an app goes back to the whole search**, with nothing typed. So does Backspace on the empty box, which leaves the word typed. Esc in the search closes the menu, as before. In a View, Esc still pops the View first.

## Two Segments per Extension

Speed first: whether a row is in the search must cost nothing per row on a keystroke. Each Extension has two Segments in the Index, at `extension * 2` (the Items the search shows) and `extension * 2 + 1` (those its app alone shows). `index::split` sends an Extension's batch to the Segment each Item's `app_only` names. An Item that changes sides is removed from the other Segment in the same batch. A batch with Items on one side only, nearly every batch, goes through as it came, with no copy.

What a search reads is a `Reach`, decided once per keystroke and tested once per Segment:
- the whole search reads every Segment the config leaves searched, side 0 by default;
- an app reads its Extension's two.

Near misses, keywords and the Empty query's most used follow the same Reach.

The Indexer hands over both Segments of an Extension at once (`Indexed`), so an Item moving between them never shows in both, or in neither, between two frames. A change that leaves both Segments as they were still makes a new version, so a caller waiting for it hears back.

Measured against `main`, the global search's instruction counts rise 0.06% to 0.27% (`scripts/bench-instructions`), from the one more entry Item per Extension. The 10k-Item Daemon's resident memory is the same within the bench's run-to-run spread.

## The protocol

This adds fields and changes no meaning:
- `Item.app_only`;
- `Scope.intro` (`Intro`, `Example`);
- the Manifest's `about`, `examples`, `commands` and `in_search`.

Protocol 1.0 is not frozen until the Kotodama v1 release (ADR 0026), and no release speaks it, so 1.0 is amended and its layout pinned again, rather than raised to 1.1.

## How it relates to Hidden items and Views

A **Hidden item** (ADR 0015) is never searched. Only `kotodama open --view` reaches it. An app-only Item is indexed and searched, in its app.

An app is not a View. Nothing is pushed, and the rows are Results with everything Results have: Frecency, Learned choices, Contributed actions and the Action panel. The Extension sends nothing new and answers nothing new. Its Manifest describes the app, and each Item says which side it is on.

## Considered options

- **Keep ADR 0024's implicit Prefix, where any query starting with the word searches the app.** Rejected: every Extension now has a word, and `apps`, `system` or `web` are ordinary words to search for.
- **A visibility flag checked per row.** Rejected: it is a branch on every row of every keystroke for something that changes only when an Item does.
- **The intro card sent by the Extension when the app is entered.** Rejected: it would cost a round trip before the first frame, and the Manifest already holds what the card says.
- **Raise the protocol to 1.1.** Rejected while 1.0 is unreleased: the rules of ADR 0026 allow pinning 1.0 again until v1.

## Later

A user-defined app over several Extensions, such as `dev` for projects and scripts, would be a `Reach` over their Segments and a config table naming them. Nothing here builds that.

## Amendments

- **The word is `word` (2026-10-21).** Before protocol 1.0 froze, the word's key became `word` in the Manifest and the config (`prefix`, its old name, is still read, with a warning), `kotodama open --prefix` became `--app` (`--prefix` is still read), `Scope.prefix` became `Scope.word`, and the entry Item's id became `app:<extension>`. Typing the word scopes nothing, so "prefix" named something Kotodama doesn't have.
- **The entry Item matches as a word start at best, and keeps what follows the word (2026-10-21).** The app's word was an exact keyword (ADR 0021), so `calc` ranked the Calculator app above GNOME Calculator and `system` ranked the System app above System Monitor; Enter then entered an app instead of launching one. The entry Item's title and word now match at most as a word start, so on a tie the Item kind puts the application first (ADR 0011), and a few picks make the entry Item the Learned choice for its word. Its word still finds it with text after it: for `emoji fire` the row reads `Search Emoji for “fire”`, and running it enters the app with `fire` still typed.
- **Every Extension keeps an app, and its word follows one precedence (2026-10-21).** A word another app had fell back to the name, and an Extension whose name was taken too had no app, by the order Extensions were found in. ADR 0028 puts Aliases, Quicklinks and words in one table with a fixed precedence; an app always gets a word.
