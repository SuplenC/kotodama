# 10. Undo puts back what a change to the History replaced, unless it changed since

Status: accepted (2026-10-20), amended (2026-10-20): Front ends offer the latest change as a Toast; amended (2026-10-20): an Extension withdraws a pick, which takes off that one pick

Forget, clearing one Extension's History and "Clear history" can be undone for a minute, and a launch that turns out to be a mistake should be withdrawn (#42). Each change to the History returns what it replaced: every entry it touched, as it was before and after. The Daemon keeps these in memory, by id, until they expire, and undoing one puts back each entry whose value is still what the change left. An entry changed since, say by a pick made after "Clear history", stays as it is now. So one mechanism undoes a Forget, a whole clear or a single pick, in any order, and never loses a later pick.

The user undoes through an Undo Result of the Daemon's own, found by typing `undo`, which undoes the latest change still kept. It works in every Front end as it is.

The Daemon also offers each change to every Front end as it happens (`OfferUndo`, naming the change by id), and withdraws the offer once the change is undone or its minute runs out (`WithdrawUndo`). The Quickshell Front end shows the latest offer as a Toast, whose key (Ctrl+Z) sends `Undo` for that id. The minute is the Daemon's, so the Toast and the Undo Result go together, and a Front end that connects within it is offered the latest change too.

A pick made by running an Extension's Action is kept the same way, under the run's `invocation`, but only that Extension may undo it, with `WithdrawPick`, as the apps Extension does when the app it launched exits within a few seconds. The user is never offered it: it has no Toast and no Undo Result, and Ctrl+Z still undoes the latest Forget or clear. A pick is the one change whose entries are often touched again within its minute, as when an app is launched twice. So where an entry changed since, undoing a pick takes that one pick off it: its weight, decayed to the entry's latest pick, comes off the score and the parts of the day it went to, and off the Learned choice's strength or the contender's count. A Learned choice the pick made stays the choice. Every other change still leaves such an entry as it is.

Clearing one Extension's History also forgets the Identities of its Items, though another Extension may offer the same thing: the History doesn't record which Extension a pick of an Identity came through, and keeping it would leave the cleared Results ranked by it.

## Considered options

- **Keep a copy of the whole History before each change.** Rejected: a copy per pick would cost memory for every launch, and undoing would drop every pick made since.
- **A Front end toast with an Undo button, instead of the Undo Result.** Rejected: a Front end that doesn't show the offer would have no way to undo. The Toast came later, next to the Undo Result, because typing `undo` was hard to discover.
- **A Toast with a timer of its own in the Front end.** Rejected: it would drift from the Daemon's minute, offering to undo what can't be undone any more, or hiding while it still can.
- **Keep the Identities another Extension also offers.** Rejected: which Extension offers a thing changes as they come and go, so what clearing kept would change with them.
