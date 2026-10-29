# 37. Contributed actions can target Identities, and every Action has a role

Status: accepted (2026-10-29); amends ADR 0002

The menu is full of Results that are really Actions on another Result: "Google Chrome" and, under it, "Remove Google Chrome", an Item of its own. The aim is one Result per thing, with what can be done to it in its Action panel. ADR 0002 lets an Extension offer an Action on every Item of a kind, which is too wide for this: a package manager can remove the apps it installed, not every app. This is the first of three steps. The next lets the query pick which Action Enter runs (`remove chrome`), using each Action's keywords; the last lets a placeholder Item step aside for the real one.

## Targeted Contributed actions

- **Declared in the Manifest, as ADR 0002's are.** A `[[contributed_actions]]` entry has `targeted = true` in place of `kind` (exactly one of the two; `ContributedTo::Kind` or `ContributedTo::Identities`). Declaring it there keeps what ADR 0002 gives: a shortcut bound to a Reserved key is a problem of the Manifest, `kotodama extension check` reads it, and the Daemon knows every Action an Extension may contribute before it runs.
- **Aimed while the Extension runs.** `FromExtension::TargetAction { action, identities }` names the whole set of Identities the Action applies to, and replaces the set sent before for that Action; `[]` withdraws it. Sending the whole set, rather than additions and removals, means an Extension that lost track of what it sent recovers by sending again, and the Daemon never has to reconcile two halves of a change. A set lasts while the Extension runs: its run ending drops it, and it sends the set again after it starts, as it sends its Items. An Action the Manifest doesn't declare `targeted` is rejected (`ToExtension::Rejected`).
- **Matched by hash lookup, with no predicates.** `targets.rs:Targets` keeps, for each Identity named, the Actions on it. Building a row looks its Identity up there once, and only when some Action is targeted at all, so an Item without an Identity, or a Daemon with no targeted Action, costs nothing more. A predicate the Daemon evaluates per Item was the alternative the issue raised; it would run for every row shown, and needs a language for it.
- **By Identity, so whichever Extension offers the thing.** A targeted Action applies to every Result with that Identity (ADR 0008): the apps Extension's Google Chrome and a Folder source's `google-chrome.desktop` both get "Remove". As with Actions by kind, it is never offered on the contributing Extension's own Items, which carry their own Actions. Extensions still never see each other's Items: the contributing Extension names Identities it knows from its own domain (the packages it installed), and is told nothing about which Items have them.
- **The open menu follows.** When a set changes, each menu showing a Result of an Identity named before or now gets its rows again, as they rank (`Update::Rows`), with no search.
- **Not yet in list Views, nor in the Action catalog.** A list View's Items share their Contributed actions by kind (`ToFrontEnd::PushView`'s `contributed`), and the catalog lists Actions by the kind they run on; a targeted Action has no kind. Neither needs one for the problem this solves.

## Every Action gets `keywords` and a `role`

- **`keywords`**, the verbs the Action is known by (`remove`, `uninstall`, `delete`), never the words of its title. This step only carries them; the next lets the query choose the Action by them.
- **`role: ActionRole`**, `Normal` by default, `Destructive` or `Form`. A variant rather than a flag, so it can grow, and so a Front end can draw each one its own way. An Action that is both destructive and a form is `Destructive`: what it does matters more than how it asks.
- **Who sets `Form`.** The Extension, for an Action that pushes a form View of its own. The Daemon, by itself, for an Action whose required Arguments the query hasn't filled (`Daemon::mark_forms`, the same fill `run_action` does), since running it now opens the Daemon's Argument form; never for a destructive one.
- **`confirm` stays apart.** It is behaviour, ask before running; `role` is how the Action looks. A destructive Action that can't be undone sets both.
- **The Action panel order.** The primary Action first, then the Item's other own Actions, then the Contributed ones, then every destructive Action but the primary. The primary stays first whatever its role, as Enter runs it and the panel opens on it. The protocol sends own and Contributed Actions apart, so the order is the Front end's to keep; kotodama-qml keeps it in `Row::actions`, and `Row::action` runs the entry at the same place. kotodama-qml shows `role` and `keywords` to QML on each panel entry and the primary's role on each row (`primaryRole`), and draws nothing new with them yet.
- **On the wire.** Both go at the end of `Action`, in protocol 1.0 in place, as `weak_keywords` did: 1.0 isn't released (ADR 0026), so `tests/layouts/1.0.txt`, `kotodama.proto` and `docs/protocol.md` are pinned again. A normal role is sent as nothing (`null`, an absent protobuf field), not as `"normal"`: nearly every Action is normal, and every Action of every Result on every keystroke carries one. `"normal"` and `""` are read as normal too.

## An Item resent with its Actions reordered

An Extension that changes which Action is primary from its state (Wi-Fi on or off) resends the Item with its Actions in a new order, as before. Measured with the `instructions` bench `item_resent`, the 10k Items of `corpus.rs` and a menu showing `f`: putting such a version in cost 4,959,687 instructions on the thread that answers keystrokes, twice a first keystroke. Two costs made it:

- **Every open menu ranked its query again**, as for any change to Items, and a narrowed search was dropped. Now a Segment version notes whether anything a search reads changed (`Segment::keys_changed`). An Item sent again with the same kind, Identity, keyword and folded texts (`same_keys`) keeps its row and texts, and only its payload and Actions change (`reshelve_row`). A version that changed nothing searched leaves `Index::keys_version`, which narrowing goes by, and the menus showing that Extension's Results get their rows again, without a search (`Update::Rows`).
- **The Segment version it replaced was freed on that thread**: a free for each row's allocations, 93% of what was left. The indexing thread copies a Segment to change it, so the Index held the last reference to the old one. `Index::replace` now returns it, and the Daemon hands it back to the indexing thread to free (`Indexer::free`). This was the cost of every change to an Extension's Items, not only of this one.

| `item_resent`, instructions | |
|---|---|
| Base | 4,959,687 |
| Rows sent again, no search | 3,070,387 |
| And the old version freed off the thread | 196,703 |

Copying the Segment on the indexing thread, off the keystroke path, is unchanged: it is ADR 0017's design for every change.

## Cost

`instructions` bench, base (main) against this change, both counted in the same directory:

| Bench | Base | This change |
|---|---|---|
| first keystroke | 2,430,653 | 2,473,603 (+1.8%) |
| next keystroke | 1,438,088 | 1,445,309 (+0.5%) |
| two words pasted | 2,938,694 | 2,947,523 (+0.3%) |
| Empty query, 2k picked | 34,628 | 35,657 (+3.0%) |
| emoji in the root search | 3,823,915 | 3,918,508 (+2.5%) |
| emoji, app scoped out | 2,471,501 | 2,437,538 (−1.4%) |
| emoji, app searched | 2,084,852 | 2,111,637 (+1.3%) |
| an Item resent, Actions swapped | (new) | 196,703 |

The increase is the two new fields of every Action shown: cloning, encoding and dropping them, about 70 instructions an Action. It was +5.2% on the Empty query, whose Results each carry the Daemon's Forget Actions; building the "Clear the history of …" title without `format!` takes that back below 3%. The Index heap is the same (4,614,145 bytes for 10k Items, +24 bytes for the empty `Targets`), as the Index keeps no Action. An encoded Item is 2 bytes longer for each Action it has.

## Considered options

- **Send the targeted Action whole in the message, with no Manifest entry.** Rejected: the Manifest is where ADR 0002 declares Contributed actions, where `kotodama extension check` and the Reserved key check read them, and an Action's title and verbs don't change with the set of Items it applies to.
- **Add and remove Identities one at a time.** Rejected: a lost or repeated message leaves the set wrong until the Extension restarts. The whole set, resent, is always right.
- **Predicates the Daemon evaluates.** Rejected, as above: per-row work and a language to define.
- **A `destructive` flag.** Rejected: a variant can grow, and a form is a third case already.
- **Send `"normal"` for every normal Action.** Rejected: it made the Empty query 7.9% more expensive to encode, against 5.2% for nothing.
- **Order the Action panel in the Daemon.** Rejected: own and Contributed Actions travel apart, and a Front end needs both lists as they are, so the order is a rule Front ends keep.
