# 13. A typo makes a Near miss, a Tier below every real match, looked for only when rows are left empty

Status: accepted (2026-10-20)

A typo used to mean no Result at all: nucleo matches the query's letters in order, so `fierfox` and `calculater` found nothing. Users expect a typo to still find what they meant (T4–T7), yet a real match to come first (T8), and short queries not to match everything (T9).

A word of the query may now have one typo per 4 letters: a letter missing, extra or wrong, or two neighbouring letters swapped (the optimal string alignment distance). The word is compared with the start of each word of an Item's title and keywords, so `firefx` finds Firefox before the last letter is typed. A Result found only this way is a Near miss, a Tier below Scattered (amending ADR 0006). Frecency never lifts a Near miss, so every real match ranks above it, however much the Near miss is used. A Learned choice or an Alias still applies to it, because the user asked for it. A Near miss highlights the start of each word of its title that a word of the query nearly matches, so the user sees why it is there.

Speed comes first, so:

- **Near misses are looked for only when the real matches leave rows on screen empty.** A Near miss ranks below every real match, so with the rows full it would never be seen. Most keystrokes, short queries above all, skip the work.
- **The first letter is never a typo.** People rarely mistype it, and holding to it lets a dense array of each Item's word initials, kept beside the Index's Entries, rule out most Items without reading them. It also keeps a 4-letter query from nearly matching most words.
- **A query that adds to the last one looks only among the last one's matches and Near misses**, as long as each word allows as many typos. A longer word is never nearer the start of a text than its own start was, so nothing new can turn up. When a word reaches the next multiple of 4 letters, every Item is looked at again.

`query_10k_items/next_keystroke` types a 15-letter word that nothing in the corpus matches. It now searches the whole Index for Near misses three times per word (at 7, 8 and 12 letters), and runs about 30% slower than before over the whole word.

## Considered options

- **nucleo's own scoring.** Rejected: nucleo has no typo tolerance. It matches letters in order with gaps, so a swapped or wrong letter fails.
- **A fixed number of typos per query**, one or two. Rejected: one is too few for a long word such as `calculater` with a second slip, and two let a 4-letter query match nearly anything (T9).
- **A typo budget for the whole query** rather than for each word. Rejected: a long first word would lend typos to a short second one, which then matches too much.
- **Allowing a typo in the first letter.** Rejected for speed and for precision: without it, no index rules out an Item before its text is searched.
- **Looking for Near misses after the real matches are sent, as late Results** (ADR 0003). Rejected: the work is the same, and the Front end would draw twice for one keystroke.
- **Letting Frecency lift a Near miss to Scattered.** Rejected: a Near miss used daily would then rank above real matches, against T8.
