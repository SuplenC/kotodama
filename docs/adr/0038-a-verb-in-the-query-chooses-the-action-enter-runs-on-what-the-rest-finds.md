# 38. A verb in the query chooses the Action Enter runs on what the rest finds

Status: accepted (2026-10-29); follows ADR 0037

ADR 0037 gave every Action `keywords`, its verbs, and let a package manager offer "Remove" on the apps it installed. This step lets the query use them: `remove chrome` shows Google Chrome with Remove as the Action Enter runs, so the menu needs no "Remove Google Chrome" Item of its own. The last step lets such a placeholder Item step aside for the real one.

## What a verb is

- **Only an Action's `keywords`**, its own or a Contributed one's, by kind or targeted. Never a word of its title: a title is written for reading ("Move to Trash"), and its words collide with the words of Item titles all the time.
- **A query word names a keyword** when it equals it, or starts it with at least three letters (`MIN_PREFIX`): `rem chrome` names `remove`, `re chrome` doesn't, as two letters start too many words to mean the verb. The word may stand anywhere: `chrome remove` works too. Of several words that name one, the first is the verb.
- **The subject must match.** The other words, the subject, must find the Item, so `remove` alone chooses nothing, and the search for it is today's: it never lists every removable Item above what the word really matches.

## How the Results come

- **The whole query is searched as before**, and so is the subject alone (`verbs.rs:Verbs::find`, `Daemon::add_verb_matches`). Of what the subject finds, the Indexed items that offer the verb join the Results, each ranked exactly as the subject alone ranks it: its Tier, nucleo's score and its Frecency are those of `chrome`. An Item the whole query found too takes the subject's rank. The rest of what the whole query found stays, so `rem chrome` still finds Chrome Remote Desktop by its title.
- **An Item offers the verb** when one of its own Actions, a Contributed action on its kind, or a targeted one on its Identity has a keyword the verb names; a Contributed action from the Item's own Extension doesn't count, as it is never offered there. The search checks this without reading an Item off its Shelf: a Segment keeps each Action it offers with its folded keywords (`Offered`), and each row the indexes of its Actions there (`Segment::named_action`).
- **Aliases.** When the subject is an Alias, the Alias names its Item and the verb its Action: `remove ff` shows Firefox, badged `ff`, with Remove primary. An Alias whose Item doesn't offer the verb is not named by the subject.
- **Which Action.** Of the Result's Actions, the first of its own with a keyword the verb names, else the first such Contributed one (`rows.rs:verb_choice`), worked out when the row is built, so the menu follows when the Actions change. The other Actions stay in the Action panel.
- **Live results** keep their own rank and get no verb: the Extension answering a Live query was asked the whole query, and can read the verb itself.

## On the wire

`ShownResult::primary`, an `ActionRef`, names the Action Enter runs when the verb chose one other than the Item's first own Action; `None` otherwise, so a Result without a verb costs one byte more on every keystroke. It names one of the Item's own Actions or one of `contributed`. As ADR 0037 did, it goes in protocol 1.0 in place, which isn't released: `tests/layouts/1.0.txt`, `kotodama.proto` and `docs/protocol.md` are pinned again.

kotodama-qml makes the chosen Action the primary entry of the Action panel, then the Item's own Actions, the usual first among them, then the Contributed ones, then the destructive ones; Enter runs it, and the footer names it. Each row has `primaryTitle`, `primaryRole` and `primaryChosen`, whether the query chose it in place of the usual first Action. How the menu draws that is left for the owner to choose from a mockup; `KotodamaMenu.qml` is unchanged.

## What a pick teaches

A pick of the Action the verb chose counts towards the query's Learned choice (`History::learn`), so `remove chrome` ranks Chrome first once the user has removed it that way often enough, and adds nothing to Frecency: the verb chose what ran, not how much the user uses the Result, and removing an app is no reason to rank it higher for `chrome`. Picking another Action of the same Result, through the Action panel, is an ordinary pick. Through an Alias, the verb wins: nothing is added to the Alias's Frecency either. In an app kept out of the search, a verb pick keeps nothing.

## Cost

The verb lookup is in `Verbs`, every Action keyword on offer, folded, sorted: a binary search per query word, with an ASCII word folded on the stack. It is built again only when the Actions on offer may have changed: a Segment put in with a keyword no version before it had, or emptied (`Index::verbs_version`), or the running Extensions' Contributed actions changing. Never by scanning Items. Only when a word names a keyword is the subject searched, and for each of its matches the Daemon checks whether it offers the verb.

`instructions` bench, base (main) against this change, both counted in the same directory. Every Setup now runs a targeted Contributed action with three keywords and aimed at nothing, so the table is full and each query is looked up in it:

| Bench | Base | This change |
|---|---|---|
| first keystroke | 2,473,433 | 2,480,711 (+0.3%) |
| next keystroke | 1,445,296 | 1,447,171 (+0.1%) |
| two words pasted | 2,947,349 | 2,953,059 (+0.2%) |
| a verb and its subject pasted, `remove fire` | (new) | 3,744,716 |
| Empty query, 2k picked | 35,655 | 36,322 (+1.9%) |
| emoji in the root search | 3,916,079 | 3,923,102 (+0.2%) |
| emoji, app scoped out | 2,435,176 | 2,440,821 (+0.2%) |
| emoji, app searched | 2,111,592 | 2,118,834 (+0.3%) |
| an Item resent, Actions swapped | 195,071 | 197,760 (+1.4%) |

The increase on a query without a verb is the `primary` field of every Result, cloned, encoded and dropped, and the table built on the first keystroke after a change. A query with a verb searches the Index twice: `remove fire` costs about what `fire tool` does plus a search for `fire`.

## Considered options

- **Words of the Action's title as verbs.** Rejected, as the issue decided: `open` or `move` in a title would make half the queries a verb.
- **Search the subject alone, dropping the whole query's matches.** Rejected: `rem chrome` would lose Chrome Remote Desktop, a real match of what was typed.
- **Rank the verb's Results by the whole query.** Rejected: the verb isn't in the Item's texts, so it would rank as a scattered match, and `remove chrome` would rank Chrome differently from `chrome`.
- **Rotate the chosen Action to the front of the Item's Actions**, as an Alias's Action is. Rejected: a Contributed action can't go there, and the Front end couldn't tell the chosen Action from the usual first one.
- **Count a verb pick towards Frecency too.** Rejected: removing an app, or anything done through a verb, says nothing about wanting it ranked higher next time.
