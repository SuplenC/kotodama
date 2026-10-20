# 20. Late Results may rerank the rows until the selection moves or 150 ms pass

Status: accepted (2026-10-20). Supersedes ADR 0003.

ADR 0003 put every Result that comes late below the rows on screen, so a list never moved under the user's cursor. A Live query is often slower than the Index, so its best Result, even an exact title, lands below the rows the first search filled, and the user has to scroll for it (V4, V5, Z4). Most Live queries answer within a few dozen milliseconds of the keystroke, before the user has read the list.

The Daemon now ranks Late Results with the rest, the rows on screen included, until either:

- **the user moves the selection** off the top row, by key or by pointer, which the Front end tells the Daemon with `SelectionMoved`, once per query; or
- **150 ms have passed since the keystroke** (`RERANK_FOR`), timed from the `Query` that changed the search, on the Daemon's clock.

After that, Late Results go below the rows on screen and any scrolled past, as ADR 0003 had it. A new query opens the window again. An answer, such as the calculator's, still takes the top row whenever it comes (ADR 0006, 0016), and a Fallback still ranks below every real match.

The rule covers every late update, not only Live query Results: an Index change that lands within 150 ms of a keystroke reranks the same way, since it moves the rows just as much. A Live query whose debounce and answer take longer than 150 ms never reranks; its Results go below the rows on screen.

## Enter runs only what the user has seen

A rerank can land between the frame the user read and their Enter. The Front end therefore confirms which list it ran from:

- Each `Results` message carries a `version`, which counts up per Front end.
- `RunAction` carries `shown`, the version of the Results in the last frame on screen. The QML Front end notes the version on its window's `afterAnimating`, just before a frame takes the model, and confirms it on that frame's `frameSwapped`.
- For the current query, the Daemon notes each Result a late update added or moved up a row, with the last version that did. It refuses a `RunAction` for such a Result when that version is later than `shown`, with `ActionFailed`: the row the user ran held something else on screen. Nothing runs, and the next Enter, on the list now shown, does.

A Result that a late update moved down, or left where it was, still runs. The list a query first shows is exempt: the user may type and press Enter before its frame, as before, and the top row then is the best match for what they typed.

Running by Result id was already what the Front end confirmed; the version adds which frame it saw. The protocol version goes up (ADR 0012).

## Considered options

- **Keep ADR 0003.** Rejected: an exact match from a Live query stays below weaker Indexed matches, which is what the user expectations (V4, V5) ask to change.
- **Always rerank.** Rejected: a list that changes after the user has started moving through it makes Enter run something other than what they chose.
- **Rerank only rows below the selection.** Rejected: the selection starts on the top row, so this would never move the Result the user is most likely to run.
- **Enter runs whatever is selected, with no confirmation.** Rejected: the Front end may have taken a rerank into its model and not drawn it yet when Enter arrives.
- **The Daemon swaps in the Result that was at that row before the rerank.** Rejected: the Daemon would run something other than the Result the Front end named, and would need the row and every earlier list.
- **A longer window, such as 300 ms.** Rejected: by then a user has found the top row, and a move under the eye is more noticeable than a better Result arriving below.
