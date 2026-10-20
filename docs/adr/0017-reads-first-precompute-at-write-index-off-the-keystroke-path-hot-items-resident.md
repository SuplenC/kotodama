# 17. Reads first: precompute at write, index off the keystroke path, keep only hot Items whole in memory

Status: accepted (2026-10-20). Amended by ADR 0023.

The Index is read thousands of times for every time an Extension writes to it, yet the Daemon did the work on the read side. Each keystroke folded each Item's texts again and found their words again. An Extension's batch was indexed on the event loop, so a keystroke behind it had to wait. Every Item stayed whole on the heap, though a search reads only its texts and a keystroke shows at most 50 Results. The Daemon now does that work when an Item is written, so that a read costs as little as possible.

**Precompute at write.** Each Extension's Indexed items are one Segment, built when the Items arrive. It holds dense arrays of each row's letters and word initials, and the folded texts of every row in one arena. The arena takes one byte per character, or one `char` per character for the few texts that aren't ASCII. It also holds where each text's words start. A search runs down the arrays and the arena without following a pointer per Item, and ranking never reads the Item itself.

A search passes over a row on its letters and word initials alone, both dense. For the rows left, it reads only the title and keywords to score them, and nucleo turns most of them down. Only for a match does it read the rest of the row's keys, and ranking takes those from the search without looking the row up again. Each Segment numbers its kinds of Item, and each search works out once where every kind ranks (ADR 0011), so ranking looks a kind's place up instead of comparing kind names for every Result. Looking for near misses, the Daemon checks a row's letters before it reads the row's texts.

A Slot is now an Extension's Segment plus a row in it. The last tie-break (ADR 0006, ADR 0011) is the Extension, then the order Items arrived in within it, whatever order the Extensions' batches arrive in. A row emptied by one change is taken only by the change after it, so a new Item never takes a shown Item's place on screen.

**Index off the keystroke path.** An indexing thread makes Extensions' changes into a new version of the changed Extension's Segment, copied on write. The Daemon swaps the new version in whole between two events, so a keystroke never waits for a change and never sees half of one. Changes that queue up while the thread works are made together, as one version per Segment. A version built before its Extension stopped is dropped.

The Daemon and the thread share the Segments by `Arc`. Only the Segment being changed exists twice, and only until it is swapped in, so the resident Index is never doubled. A change shows once its version is in: about 1.5 ms after a batch of 1,000 clips, and about 12 ms after the 10k apps are scanned again.

**Hot and cold Items.** A Segment keeps in memory only what a search and ranking read: the search keys above, and each Item's id, Identity, keyword and the Actions it offers. The whole Item is encoded with postcard onto a Shelf. A Shelf is an unlinked file in `$XDG_CACHE_HOME/kotodama` that only its user can read, or memory when there is no file to use (in tests, or after a failed write).

A Shelf is only ever appended to, so a later version never overwrites what an earlier version still reads. Each change writes its Items in one write. When at least half of a Shelf is garbage, the live payloads move to a new Shelf in one read and one write. An Item is read back off the Shelf when a Result shows it. It stays resident while it is hot, while a keyword or an Alias names it, or while it is among the 256 Items read last. Every other Item is cold. The indexing thread reads the Items a keyword or an Alias names when it builds the version holding them, and sends them along with it, so a keyword typed never waits for a read.

The hot Items are the ones the History predicts. They are worked out every 5 minutes, and a second after the Index or the History changes. The Daemon sends the indexing thread a copy of the picked Items' picks; the thread looks at those whose time-of-day habit (ADR 0009) is above zero anywhere from 30 minutes before now to 15 minutes after, and takes the 256 with the strongest habit. It reads their payloads off the Segments the Daemon holds, and sends them back for the Daemon to keep, so an app picked at 8:30 every morning is resident by 8:15 and goes back to cold in the afternoon. Neither the planning nor the reads run on the thread that answers keystrokes.

When the menu opens, the Daemon reads one element per cache line of each Segment's arrays, and reads the hot Items. That way the first keystroke doesn't find the CPU cache taken by other programs.

**Being hot is only about payloads.** Every search still scans every Item's keys, because ranking compares Tier before Frecency (ADR 0006). A cold Item that matches the query exactly outranks a hot one that matches it scattered. A search that looked at hot Items first could never stop early, or its Results would differ from those of a full search. At every point, a search returns the Results that a full search of every Item would return. A property test holds the Daemon to that, through random changes, picks, hours and keystrokes.

**Long titles.** nucleo's optimal match takes time that grows with the text times the query. A text past 64 characters is now scored by nucleo's greedy match instead, whose time grows with the text alone. Both match the same texts; only the score, a late tie-break, may differ. That lets the clipboard Extension search the first 100 characters of a text, up from 60.

A list View's filter (ADR 0015) uses the same matcher, but runs on the View's own Items, which aren't indexed.

## Consequences

- The heap per Item drops from 983 bytes to 461 for the 10k apps corpus, and from 1,271 to 533 for the emoji. Each payload, about 155 bytes, sits in a file that the kernel pages in and out.
- Showing a cold row costs a `pread` and a decode, a few microseconds. A keystroke shows at most 50 rows, and the ones it showed last stay resident. Decoding takes about 4,000 instructions an Item, most of them allocations and UTF-8 checks. On the 10k apps corpus, against an Index that kept every Item whole on the heap: a first keystroke with none of its 50 rows resident runs about 3% more instructions, and two words pasted, with 20 cold rows, about 5% more. With their rows resident, the first keystroke runs about 8% fewer and two words about as many.
- A Learned choice, an Alias's keyword, Identity grouping and the catalog of Actions by kind all read only what the Segment keeps in memory. For each kind and Action id, the catalog offers the Action of the first row whose Item has it.
- A batch takes longer to index, because each Item is also encoded and written once.
- The hot set comes only from the time-of-day habit. The History has no weekday, so weekdays and weekends share one routine. The Empty query's rows are still ranked when the menu opens, not ahead of each window. ADR 0023 makes Learned choices hot and ranks the Empty query ahead; the weekday is left for later.

## Considered options

- **A hot segment scanned first, with cold keys loaded on demand.** Rejected: because ranking goes by Tier first, the hot segment can't end the search early (see above). Keys loaded on demand would also put disk reads on the keystroke path.
- **Memory-map the payload file.** Rejected for now: a `pread` of at most 50 rows per keystroke is cheap, and a map counts against resident memory while its pages are touched.
- **Index on the event loop, but in smaller slices.** Rejected: a keystroke still waits for a slice, and the Index is half changed between slices.
- **Double-buffer the whole Index.** Rejected: it doubles resident memory. A version per Segment, shared by `Arc`, copies only what changed.
