# 16. A Fallback ranks below every real match, and is asked only when rows are left empty

Status: accepted (2026-10-20)

Web search (#14) should show only when nothing else matches well, and never among real matches (V3). A Live query's Results rank like Indexed items (ADR 0003, 0006), so "Search Google for fire" would match `fire` at a word start and rank above a Firefox that matched inside a word. With Frecency, a search the user often picks would climb higher still.

A Manifest may now mark its Live query a Fallback: `[live_query] fallback = true`. The Daemon then:

- **ranks the Fallback's Results below every real match**, Near misses included (amending ADR 0006 and 0013). It compares this before anything except an answer, so no Tier, Frecency, Learned choice or Alias lifts one. Frecency only orders the Fallbacks among themselves.
- **never makes a Fallback's Result a Learned choice.** A pick of one counts for its Frecency alone. Otherwise picking "Search Google for fox" a few times would take over from the app the user had taught `fox`, and change the order of the real matches.
- **asks the Fallback the query only when the real matches leave rows on screen empty**, the rule Near misses follow, and shows its Results only then. With the rows full they would sit below them unseen, so a keystroke whose real matches fill the rows costs nothing. Its triggers and prefixes still send it what they match on any keystroke, so web search's triggers send what may be an address, and its answer, "Open example.com", takes the top row as the calculator's does.
- **lets a real match that comes late go above a Fallback's Results on screen**, an exception to ADR 0003. Those rows move down, never off the screen ahead of a real match, so Enter on the top row still runs what the user saw there.

This changes no message, only the Manifest, so the protocol version stays (ADR 0012).

## Considered options

- **A dedicated class of Result in the protocol**, such as `LiveResult.fallback`. Rejected: a new protocol version for something the Manifest says once, and each Result of a Fallback would repeat it.
- **A score of 0.** Rejected: the score breaks ties only within a Tier, so a search would still rank above a Scattered match or a Near miss.
- **The Extension deciding by itself whether to answer.** Rejected: it can't see the real matches, and asking it on every keystroke is a round trip the menu doesn't need.
- **A threshold on match quality**, such as "no match better than Scattered". Rejected: a query with a single weak match still gets searches below it under the rows-left-empty rule, and a query that fills the screen with weak matches is better served by a different query than a search nobody scrolls to.
