# 42. An Item is named by any of its names, and by its title when it takes Arguments

Status: accepted (2026-11-13). Amends ADR 0028 and ADR 0034.

The Daemon passed typed text on to an Item in one case only: the query started with the Item's single `keyword`. If the Item was found any other way, by its title or one of its search `keywords`, the row showed, but the text typed after the name was dropped, and Enter opened a form that asked for it again. An Extension's timer with the keyword `clock-timer` started from `clock-timer 25m`. `timer 25m`, a word in its `keywords`, and `start timer 25m`, its title, only opened a form. An Item with no keyword at all, such as "Holidays", could never take its Argument from the search.

There were two causes. An Item had at most one keyword, so only one of the things people call it could carry a value. And the title, the words on the row, never counted as a name.

## What it does

### An Item has a list of names

- **`keyword: Option<String>` becomes `names: Vec<String>`,** on the wire and in Rust.
  - Protocol 1.0 isn't frozen until the v1 release, so its layout is pinned again rather than raising the version (ADR 0026).
  - In protobuf, field 11 goes from `optional string` to `repeated string`, which reads the same bytes.
  - The name `keyword` sat next to `keywords`, the search words, and the two were easy to confuse.
- **Each name works as the keyword did (ADR 0028, #271).** It is named whole:
  - in any case;
  - with spaces, hyphens, underscores or nothing between its words;
  - for an Item that takes Arguments, with as many typos as the search forgives.
- **Any of them names the Item, and what follows is its Arguments.** A name typed exactly wins over one with typos, whatever the order of the list.
- **Each name is its own key** in the Segment's keyword table.
- **Any name typed alone ranks the Item as exact** (`Tier::Keyword`, ADR 0034), not only the first. The row's `names` count says how many parts of its keywords text are names.

### The title of an Item that takes Arguments names it too

The row you see becomes the command you type: `start timer 25m`, `lock in 10m`, `keyboard set layouts us`. Titles weren't written as keywords, so stricter rules apply:

- **Only for an Item whose own Actions take Arguments** (`naming::takes_arguments`, the test that already decides on typos). For any other Item, words after the title would be more of the search, not an Argument.
- **Only the title's letters and digits count** (`naming::title_key`). Punctuation and symbols are left out along with the separators, such as the `…` of "Lock in…" or the `:` of "Keyboard: set layouts". The query's words are squashed the same way, so typing the colon does no harm.
- **The whole title, typed exactly.** Case and separators don't matter, but there are no typos and no prefixes. A short title with typos would swallow ordinary searches.
- **Last in the precedence of ADR 0028.** A title names nothing when its words, as typed, are held by any of these:
  - an Item's name;
  - an Alias;
  - a Quicklink;
  - an app's word.
- **Only a title that is unique.** A title names nothing when another Item within reach that takes Arguments has the same key.
  - Titles were never meant to be unique, so a clash isn't a configuration error and isn't reported.
  - Both rows are still found by the search as before, and neither takes the Argument.
  - A keyword clash is different: someone chose each keyword to be typed, so a clash there is the Extension's mistake.
- **It needs an Argument after it in the search.** A title typed alone is already an exact title match.
- **It ranks as a word start** (`Tier::WordStart`), a band below a name or an exact title. When the query names one Item by a name and another by its title, the name comes first.
- An Item a name already names isn't ranked again by its title. Its Argument still follows whichever of the two covers more of the query: with the name `keep-awake`, `keep awake for 1h` typed for the title "Keep awake for" passes on `1h`, not `for 1h`.

The Segment keeps a second table, from title key to rows, for the Items that take Arguments. `Index::titled` looks it up. The Items in it are kept resident with those named by a keyword, so typing a title never waits for a read. A row's names and its title key share one boxed slice in `Held`, the size the old `Option<Arc<str>>` was, so a row costs no more memory.

What the query typed for an Item's Arguments is worked out in one place, `Daemon::typed_for`:

- what follows one of its names;
- if it takes Arguments and its title names it under the rules above, what follows the title;
- when both do, what follows the longer of the two, so a name that starts the title never takes the rest of the title as its Argument.

The search, the rows' form roles (`mark_forms`) and the Arguments Enter runs with (`run_action`) all read it, so they always agree.

## Cost

A keystroke looks up the title table only when an Extension has an Item that takes Arguments. It then does one hash lookup per leading word, up to the longest title. The uniqueness and precedence checks run only for a title that was found.

## Considered options

- **Keep one keyword and let Extensions put the rest in `keywords`.** Rejected: that is the gap. A search word finds the row but can't pass on what follows, because it may sit anywhere in the query.
- **Let every search keyword pass on what follows.** Rejected: search keywords are many and loosely chosen. An app's name among them would turn `chrome tabs` into an Argument for whichever Item lists `chrome`.
- **Accept a single string as a list of one under the old name.** Rejected: postcard and protobuf decode by position, so the shape can't depend on the value. Before v1 a clean rename costs little.
- **Make a clash between two titles a reported problem.** Rejected: two Extensions may well both offer "Search", and neither did anything wrong.
- **Settle a title clash by Frecency or Extension name.** Rejected: which Item gets the Argument would then change with habit or with the Extensions installed. The search stays predictable when a clash names neither.
- **Forgive typos in titles as in names.** Rejected: titles are often one or two common words, and with typos they would claim ordinary searches.
- **Require two words or a required Argument before a title names its Item.** Rejected: the precedence and uniqueness rules already keep a one-word title from taking a word that something else holds. "Holidays pl" should just work.
