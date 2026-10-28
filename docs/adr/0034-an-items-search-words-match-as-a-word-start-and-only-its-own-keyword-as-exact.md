# 34. An Item's search words match as a word start, and only its own keyword as exact

Status: accepted (2026-10-28); amends ADR 0021 and ADR 0032

ADR 0021 ranked an exact keyword as exact, behind an exact title on a tie, and ADR 0032 counts it in the exact band. That held for both of an Item's keywords: `keyword`, the one word typed before its Arguments (`go` in `go 1.23`), and `keywords`, the search words an Extension lists so the Item is found by more than its title. Typing `chrome` with Google Chrome installed listed KubaOS's "Remove Google Chrome" and "Install Google Chrome" above the app: their `keywords` held `chrome`, an exact match, while the app's title "Google Chrome" only has a word starting with the query, a band lower. Whether an installed app ranks above the Items about it depended on how each Extension wrote its keywords.

## What it does

- **One of `keywords` equal to the query is a word start**, as if it were a word of the title. It counts 3 in ADR 0032's points, not 4, and ties with a word start, not ahead of it. On a tie the kind (ADR 0011) then puts an app before an Extension's Item about it, as ADR 0027 does for an app's entry Item.
- **The Item's own `keyword` equal to the query stays exact**, behind an exact title on a tie (ADR 0021). It is a word the Extension gives the Item to be named by, as an Alias, a Quicklink or an app's word is (ADR 0028), none of which change.
- **Habit still lifts either** as ADR 0032 says: an Item about an app that the user picks more than the app passes it.

## Considered options

- **Put the app-first tie-break before the finer match grade.** Rejected: it doesn't fix the case. The Items were a band above the app, so the points told them apart before any tie-break was reached. Moving the kind above the points instead would let an app nobody opens beat a file used every day, which ADR 0011 and ADR 0032 rule out.
- **Count an exact search word above a word start but below the exact band.** Rejected: on equal points the finer grade comes before the kind, so the Item about the app would still beat the app.
- **Have each Extension keep an app's name out of its keywords.** Rejected as the rule: KubaOS works around it so, but every Extension would have to know it, and a search word is what a user may well type.

## Consequences

- An Item found only by one of its search words no longer beats a title that starts with the query: they tie, and the kind, nucleo's score and the shorter title decide.
- The Index keeps one more flag per row, whether its keywords text starts with the Item's own keyword. It sits in padding, so a row costs no more memory, and a search reads it only for an exact match on the keywords.
