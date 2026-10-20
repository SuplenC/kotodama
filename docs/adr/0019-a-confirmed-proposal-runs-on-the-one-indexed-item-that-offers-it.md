# 19. A confirmed Proposal runs on the one Indexed item that offers it

Status: accepted (2026-10-20), amended by ADR 0022 (several Items offering it: the user picks one in a list View)

The AI Extension (#15) reads the Action catalog and proposes catalog Actions for a request in plain words, such as "set up a go project". Once the user confirms a Proposal, that Action has to run. It belongs to another Extension, though, and runs on an Item, and the catalog holds no Items by design: nothing about the user's Items ever reaches the model.

An Extension's Action may now answer with a new Outcome, `RunCatalogAction { extension, kind, action, arguments }`. The Daemon then:

- **accepts it only from an Extension whose Manifest sets `action_catalog = true`**, and only as the Outcome of an Action the user ran. It is always the answer to a confirm, so nothing runs on its own.
- **runs it on the one Indexed item of that Item kind, from that running Extension, that offers that Action**, as if the user had picked it. It goes through the same path as any Action: Frecency, Jobs and the Argument form. The form asks for any Arguments the Proposal left out.
- **refuses when several Items offer it, or none does**, and says what to pick in the search instead. It never guesses which project or window was meant. A Contributed action has no Item of its own, so it always needs the user to pick one.
- **refuses an Extension that names itself**, so the AI can't run its own Actions in a loop.

This adds a message, so the protocol version goes up to 4 (ADR 0012).

## Considered options

- **Items in the catalog, so the model picks the Item too.** Rejected: it sends the user's Items to the model, which the Action catalog was designed never to include.
- **The AI asking the Daemon for the matching Items, then sending a `ResultRef`.** Rejected: it hands an Extension other Extensions' Items it never needed, and adds a round trip to each confirm.
- **The Proposal putting the target Item in the search, for the user to pick.** Rejected: in the common case of one Item per kind, such as a Suite, that is an extra step the confirm already covered. The refusals fall back to it.
