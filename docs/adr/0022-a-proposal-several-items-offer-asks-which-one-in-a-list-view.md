# 22. A Proposal that several Items offer asks which one, in a list View

Status: accepted (2026-10-20)

This amends ADR 0019. A confirmed Proposal runs on the one Indexed item of its kind that offers its Action. When several Items offered it, such as one Suite Item per Suite each with an `install` Action, the Daemon refused and told the user to pick one in the search. The user then had to find the Item, open its Action panel and type the Arguments again, though the Proposal had them already.

The Daemon now asks which one. When two or more Indexed items of that kind, from that Extension, offer the Action, it pushes a list View of its own to the menu that confirmed the Proposal:

- **The View lists the Items that offer the Action**, in the Extension's order, each with one Action: the proposed one, under its title. It goes to that menu only, as the Argument form does.
- **Typing filters it** with the search's matcher, as any list View the Daemon filters.
- **Picking an Item runs the Action on it with the Proposal's Arguments**, through the normal path: Frecency, Jobs and the Argument form, which asks for whatever the Proposal left out. The pick View goes first, so the form doesn't sit on top of it.
- **Esc, a new query, the menu's connection going, or the Extension stopping cancels it**, and nothing runs. A second Proposal confirmed meanwhile replaces it.

When no Item offers the Action, the Daemon refuses as before, and a Contributed action still needs the user to pick its Item in the search.

The protocol doesn't change. The View, its filter and the pick go by the messages a Daemon-owned View already uses: `PushView`, `FilterView` and `FilteredView`, `RunAction`, `ViewPopped` and `PopView`, all under the Daemon's reserved Extension name.

Nothing is added to a keystroke in the search while no pick is open: the only check is whether the Daemon holds any pick at all.

## Considered options

- **The search narrowed to the candidates, as Results.** Rejected: the query box would still show the request, so what the rows are and why would be unclear, typing would end the pick rather than narrow it, and every keystroke would have to check for a pick in the search itself. A list View has a title saying what is being picked, filters as the user types, and Esc goes back to the Proposals.
- **Sending the candidates to the proposer, to ask for them in its own View.** Rejected for the reason ADR 0019 gives: it hands an Extension Items of another it never needed, and the Items would then reach the model.
- **A new message for the pick.** Rejected: a list View the Daemon owns does it with the messages there are, so no Extension or Front end changes.
