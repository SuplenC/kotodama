# 24. A Prefix keeps an Extension's Items out of the search, behind one entry Item

Status: accepted (2026-10-21)

The emoji Extension indexes over 2,000 Items. In the search they crowd out apps and files: `fire` shows 🔥 and 🧑‍🚒 among Firefox, and `editor` shows 📝. Most of the time nobody searching for an app wants an emoji. When they do want one, they know it.

An Extension can now have a Prefix: a word, such as `emoji`, that its Items sit behind. Its Manifest declares one with `prefix`, plus an optional `title` and `icon` for its entry Item. The user's `[extensions.<name>] prefix` overrides it, and `prefix = ""` takes it away. The Daemon reads the Prefix, so the Extension itself never sees it, and the key is taken out of the table the Extension gets.

## What the search does

- **The whole search leaves the Items out.** Nothing about them is removed: they stay indexed, in their Segment, ranked by Frecency as before. The Index keeps one flag per Segment, and every search over Segments (`search`, near misses, keywords) skips a flagged Segment once, at the Segment and not per row. Skipped too are the Extension's Live queries and Fallbacks, and its Items among the Empty query's most used.
- **One entry Item stands for them.** It is one of the Daemon's own Items, `prefix:<extension>`, titled by the Manifest's `title`. It is found by its title and its Prefix, as a keyword, and ranked like any other Result. Typing the Prefix exactly makes it an exact keyword match, so `emoji` ranks it first (ADR 0021). Running it types the Prefix and a space for the user (`ToFrontEnd::SetQuery`), so the menu stays open behind the Prefix.
- **`emoji fire` searches the Extension alone.** The query is behind a Prefix when its first word is the Prefix, in any case, followed by whitespace. The rest of the query is searched as any query is, over that one Segment. Own Items, Aliases and Quicklinks stay out, so a word after the Prefix means only the Extension's own keywords. The Extension's Live queries get the rest of the query alone.
- **`emoji ` with nothing after it** lists the Extension's most picked Items, then the rest in the order the Extension sent them.
- **Learned choices are kept by the whole query**, `emoji fire` apart from `fire`. A pick behind the Prefix teaches nothing about the same words outside it.
- **The Front end shows the Prefix as a chip.** `Results` carries the `Scope`: the Prefix, title and icon. The Front end takes the Prefix and its space out of the box and shows the chip before the box. It still sends the whole query, Prefix and all, so the Daemon alone decides what a Prefix is. Backspace on the empty box leaves the Prefix with the Prefix typed, and `kotodama open --prefix emoji` opens the menu behind it.

Going behind a Prefix rebuilds nothing. It changes which Segments one search reads, and it forgets the last search so the next keystroke doesn't narrow from Results of another Segment. Setting the flags happens when the Extensions or the config change, and bumps the Index version, which re-ranks the Empty query ahead (ADR 0023). The search over the other Segments is unchanged: the flag is read once per Segment per keystroke.

## How it relates to Hidden items and Views

A **Hidden item** (ADR 0015) is never searched at all: it is kept out of the Index, and only `kotodama open --view` reaches it. An Item behind a Prefix is indexed and searched like any other. Only the whole search leaves it out. Hiding is per Item and is the Extension's choice about one entry point. A Prefix is per Extension, and the user can undo it.

A **View** is a screen an Extension pushes and owns: a list, a form, a detail. A list View's filter only narrows the Items the View holds. A search behind a Prefix is not a View. Nothing is pushed, Esc closes the menu as it does from the search, and the rows are Results with everything Results have: Frecency, Learned choices, Contributed actions and the Action panel. The Extension sends nothing new and answers nothing new. Putting its Items behind a Prefix is one line in its Manifest.

## Considered options

- **A View holding the Extension's Items, pushed by the entry Item.** Rejected: the Items would have to be sent again as the View's, a list View's filter has no Frecency or Learned choices, and the Extension would have to answer a new Action. It would also make `emoji fire`, typed from the search, mean something other than the same words typed in the View.
- **Leave the Items in the search and rank them lower by Item kind.** `[ranking] kinds` already allows this. Rejected as the answer: on an exact match they still fill the rows (`fire`), and the emoji rows are still searched on every keystroke.
- **A Front end that strips the Prefix and sends a scope in `FromFrontEnd::Query`.** Rejected: every Front end would have to know the Prefixes, and `kotodama open --query "emoji "` would need its own path. The Daemon reading the text keeps a Front end that knows nothing of Prefixes working: the box shows `emoji fire` and the Results are right.
- **Hiding the Items with `hidden`.** Rejected: Hidden items are never searched, and the point is to search them, on demand.

## Later

The Index already reads a set of Segments per search. A user-defined group of Extensions behind one word, such as `dev` for projects and scripts ("custom apps"), would be a `Reach` over several Segments and a config table naming them. Nothing here builds that.
