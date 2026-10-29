# 39. A placeholder steps aside while an Item of its Identity that doesn't yield is indexed

Status: accepted (2026-10-29); follows ADR 0038

ADR 0037 and 0038 made one Result per thing possible: "Google Chrome", with Remove in its Action panel and `remove chrome` choosing it. What is left is a thing the user can't have yet: an OS offers Google Chrome before it is installed, with "Install and open" as its primary Action, and that Item must give way once the apps Extension indexes the real Chrome, and come back if Chrome is removed. The OS's Extension can't tell when to withdraw it: Extensions never see each other's Items (ADR 0002).

## The field

- **`Item::yields`.** An Item with `yields = true` is a Placeholder for the thing its Identity names. While any Item with the same Identity that doesn't yield is indexed, from any Extension, its own included, the Placeholder is hidden entirely: no search, Learned choice or Empty query finds it, and none of its Actions is offered, by verb or otherwise. An Alias the user bound to the Placeholder itself shows it as an Unavailable Result meanwhile, as for any Item gone. Once none is, it comes back.
- **History needs nothing carried over.** It already follows the Identity (ADR 0008): what the user picked through the Placeholder, Frecency and Learned choices, counts for the real Item, and the Empty query shows the real Item where it showed the Placeholder.
- **On the wire.** `yields` goes at the end of `Item`, in protocol 1.0 in place, as `weak_keywords` did: 1.0 isn't released (ADR 0026), so `tests/layouts/1.0.txt`, `kotodama.proto` and `docs/protocol.md` are pinned again. `false` by default, ignored outside `add_items`.

## Edge cases

- **`yields` with no Identity doesn't decode.** It has nothing to yield to, and an Item that says it yields expects to leave once the real one comes: shown for ever, it would sit beside the real Item with an Action that no longer makes sense, with nothing to tell the author why. Decoding refuses it, as it refuses an empty Identity, so the Extension is answered `Rejected` with the reason. Ignoring the field was the alternative; it keeps the batch, but hides the mistake.
- **Two Placeholders of one Identity and no real Item both show,** as several Results of one Identity do (ADR 0008). A Placeholder never makes another one yield: only an Item that doesn't yield does.
- **The real Item from the Placeholder's own Extension** counts as any other, in whichever order the two arrive, in one batch or apart. An Item sent again with `yields` flipped turns from one into the other.
- **A hidden real Item doesn't make the Placeholder yield.** A Hidden item (ADR 0015) isn't indexed: only `kotodama open --view` reaches it, so nothing would show in the Placeholder's place. A hidden Placeholder is hidden, as any Hidden item.
- **An Item its app alone shows does make it yield** (`app_only`, ADR 0027): it is indexed, and the user put the thing behind its app; a Placeholder offering to install it in the search would be wrong.
- **The real Item's Extension stopping** brings the Placeholder back, as its Items leave the Index.

## When it is checked

When Items are indexed, never per keystroke. A Segment keeps each Placeholder whole beside its rows, with whether it stands aside; standing aside empties its row, as removing an Item does, and coming back takes one. After each change, where every Segment is at hand (`index.rs:settle`, called by `Index::edit` and, off the keystroke path, by the indexing thread, which holds every Segment), each Placeholder is checked against every Segment's Identities (`Segment::claims`), and those whose state is wrong step aside or come back in the same version as the change. The indexing thread sends the Placeholder's Segment as a version of its Extension's, so another Extension's change can move it.

- **Cost per row.** None for an Item that doesn't yield: no field in a row's keys or in what a Segment holds for it. A Segment with no Placeholder holds an empty map. The check is one test per Segment per change while no Segment has a Placeholder, and one hash lookup per Segment per Placeholder otherwise.
- **Memory.** A Placeholder is kept in memory whole, shared (`Arc<Item>`) so the copy of a Segment the indexing thread makes to change it copies none, as well as on its Shelf while it has a row, so that it can come back without its Extension sending it again. Placeholders are few: an OS's offers of what isn't installed. The check after a change looks at every Placeholder, whatever the change touched; re-checking only the Identities a change added or removed would make it proportional to the change, worth doing if an Extension ever sends thousands.
- **Across Extensions.** On the indexing thread, the real Item's version and the Placeholder's are put in as two events, one per Extension, back to back. A keystroke handled between them sees both Results, or neither, once.

## The open menu

The real Item arriving or leaving changes what a search reads, its own row and the Placeholder's, so every menu ranks its query again, once, as a late update (ADR 0020), as for any Item added or removed; the Placeholder's row goes and the real one comes in the same update. What ADR 0037 made cheap stays cheap: the real Item sent again with the same search keys, its Actions reordered, moves no Placeholder, and the menus showing it get their rows again without a search; a Placeholder sent again while it stands aside changes nothing a search reads, and no menu hears of it.

## Cost

`instructions` bench, base (main) against this change, both counted in the same directory:

| Bench | Base | This change |
|---|---|---|
| first keystroke | 2,481,042 | 2,483,164 (+0.09%) |
| next keystroke | 1,447,369 | 1,447,502 (+0.01%) |
| two words pasted | 2,953,825 | 2,955,009 (+0.04%) |
| a verb and its subject pasted | 3,746,035 | 3,746,028 (0.00%) |
| Empty query, 2k picked | 36,345 | 36,405 (+0.17%) |
| emoji in the root search | 3,923,436 | 3,925,496 (+0.05%) |
| emoji, app scoped out | 2,441,151 | 2,443,211 (+0.08%) |
| emoji, app searched | 2,119,146 | 2,120,905 (+0.08%) |
| an Item resent, Actions swapped | 198,060 | 198,576 (+0.26%) |

No Item in the bench yields, so this is what every Daemon pays: a test per Segment after each change, off the keystroke path, and the `yields` byte of every Item encoded on the Shelf and on the wire. The Index heap gains an empty map per Segment, nothing per row.

## Considered options

- **Check per keystroke**, skipping a Placeholder whose Identity is claimed as rows are ranked. Rejected: a lookup per matched row on every keystroke, for something that changes only when Items do.
- **Merge the Placeholder into the real Item's Result.** Rejected, as ADR 0008 rejects merging: whose title, icon and Actions would it show? The real Item's Actions are what the user wants once it is there.
- **The OS's Extension withdraws the Placeholder itself.** Rejected: it would have to know which Items other Extensions index, which ADR 0002 rules out, and it would lag behind them.
- **Keep the Placeholder's row and only blank its letters.** Rejected: its keyword, Identity and Actions would still be found by a keyword, an Alias, the Empty query and a verb, each needing its own exception; leaving the row hides it from all of them at once.
