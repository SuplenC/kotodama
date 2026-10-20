# 10. Undo puts back what a change to the History replaced, unless it changed since

Status: accepted (2026-10-20)

Forget, clearing one Extension's history and "Clear history" can be undone for a minute, and a launch that turns out to be a mistake should be withdrawn (#42). Each change to the History returns what it replaced: every entry it touched, as it was before and after. The Daemon keeps these in memory, by id, until they expire, and undoing one puts back each entry whose value is still what the change left. An entry changed since, say by a pick made after "Clear history", stays as it is now. So one mechanism undoes a Forget, a whole clear or a single pick, in any order, and never loses a later pick.

The user undoes through an Undo Result of the Daemon's own, found by typing `undo`, which undoes the latest change still kept. It needs nothing new from Front ends.

Clearing one Extension's history also forgets the Identities of its Items, though another Extension may offer the same thing: the History doesn't record which Extension a pick of an Identity came through, and keeping it would leave the cleared Results ranked by it.

## Considered options

- **Keep a copy of the whole History before each change.** Rejected: a copy per pick would cost memory for every launch, and undoing would drop every pick made since.
- **A Front end toast with an Undo button.** Not now: it needs a new message and work in every Front end. The Undo Result works in all of them as they are.
- **Keep the Identities another Extension also offers.** Rejected: which Extension offers a thing changes as they come and go, so what clearing kept would change with them.
