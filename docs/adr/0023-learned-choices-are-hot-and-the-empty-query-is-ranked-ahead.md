# 23. Learned choices are hot, and the Empty query is ranked ahead of each part of the day

Status: accepted (2026-10-21). Amends ADR 0017; amended by ADR 0033 (the day of the week it left for later).

ADR 0017 made only the time-of-day habit hot. A Learned choice stayed cold unless it had been shown lately, so the query that taught it read its row off the Shelf: a `pread` and a decode on the keystroke path. The Empty query ranked its most used Results on the Daemon's thread each time the menu opened, or when a Shown found the part of the day changed (ADR 0009, #118). On the 10k apps corpus with 2,000 Items picked, that cost about 400,000 instructions per open.

Both now happen on the indexing thread, as part of planning the hot Items (`Plan`, every `HOT_EVERY` and `HOT_SETTLE` after a change).

## The hot set shares `HOT_CAPACITY` (256) in this order

1. **The Items the Empty query shows**, for now and for the next part of the day. That is at most twice `[empty_query] most_used`.
2. **The strongest Learned choices**, decayed to now, up to a quarter of the capacity (`LEARNED_SHARE`, 64 Items). A choice that names an Identity makes each of the Identity's Items hot.
3. **The time-of-day habit**, as ADR 0017 had it: the Items picked most from 15 minutes before now to 30 minutes after, strongest first.
4. **The other Learned choices**, strongest first, while there is room.

Each Item counts once. The bound on resident Items is unchanged: at most 256 hot, the 256 read last, and those a keyword or an Alias names. Alias targets need nothing new, since they are already pinned resident (ADR 0017).

The habit keeps the most room, because it predicts both the Empty query and the first keystrokes. A quarter is about the number of queries a user types often enough to teach a choice (three picks). It also keeps a long History of choices from crowding out the habit.

## The Empty query is ranked ahead

The plan ranks the Empty query's most used Results twice: at the planning time, and at the start of the next part of the day (`history::next_part`). The Daemon keeps both rankings (`Ahead`). A menu that opens, or a Shown, takes the ranking that stands for its time and ranks nothing.

A ranking stands only when all of these hold:

- it was made from the same Index version, History version and `ranks_version`. The last counts every other change the ranking depends on: own Items, Aliases, settings and the clock.
- it falls in the same part of the day as the query.
- it is at most `HOT_EVERY` (5 minutes) old, and not in the future.

Otherwise the Daemon ranks inline as before, and counts it in `Daemon::ranked_here`. The Empty query therefore never shows rows ranked from Items or picks that have since changed, nor rows from another part of the day. Within a part, the habit's weights move smoothly with the time (ADR 0009). Rows ranked at most five minutes earlier can differ from rows ranked now only on a near tie. The plan runs every five minutes, so a ranking for the next part is always made shortly before that part starts.

The ranking needs nothing a Live query gives, since the Empty query asks none. It reads the picked Items through a shared copy (`Arc<Picked>`) that the Daemon already kept for Frecency, so planning copies no picks.

## Consequences

- On the 10k apps corpus, the Empty query after 2,000 picks runs 32,139 instructions, down from 403,354. The first keystroke, the next keystroke and two words pasted run no more instructions than before. `Candidate::rank` now compares lazily, which pays for a code-generation shift in the search's selection.
- A query whose Learned choice is among the 256 hot Items reads nothing off the Shelf.
- Resident memory grows by these amounts:
  - the slots of the Items the Learned choices name, 8 bytes each (one per query that learned a choice, at most 1,000, except where a choice names an Identity).
  - an Identity's first slot per picked Item, 12 bytes each.
  - the two rankings, a few Results each.
  
  For 2,000 picked Items that is under 40 KB. The hot set's bound is unchanged. Planning no longer copies the picks every five minutes.
- The History still has no weekday, so weekdays and weekends share one routine. That is still left for later.

## Considered options

- **Rank the Empty query at the menu's open, as before.** Rejected: it is the slowest thing an open does, and it repeats work that changes only with the Index, the History or the part of the day.
- **Rank every part of the day ahead.** Rejected: only now and the next part are ever asked for between two plans, and the History changes in between.
- **Learned choices before the Empty query's Items, or with no share.** Rejected: the menu's first frame shows the Empty query's Items, and a user with many choices would leave no room for the habit.
- **A separate capacity for Learned choices.** Rejected: it would raise the bound on resident Items for users who have taught few choices.
