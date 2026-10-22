# 11. The Item kind decides between equal matches, after Frecency

Status: accepted (2026-10-20); its order superseded by ADR 0032 (the kind stays a tie-break below the points)

On an equal match, users expect an app before a project, a project before a bookmark, and a bookmark before a file (M9), yet a file they use often should still beat an app they never open (M10). Within a Tier (ADR 0006), Results now rank by Frecency, then initials over a word start, then an Alias's Result last on a tie, then the Item kind, then nucleo's score and the shorter title. This amends the order ADR 0006 gives. The kind is a tie-break: it never beats a better match, and any use beats it.

The order is a list of kinds in the config's `[ranking]` table, by default `["app", "project", "url", "*", "file"]`. Bookmarks are `url` Items. `*` stands for every kind the list doesn't name, so a window, a folder or an Extension's own kind ranks between bookmarks and files; without `*`, they rank last. Files come last because a Folder source can offer thousands of them, most never wanted.

Of the Results of one Identity, the one picked through most still leads (ADR 0008): that reordering comes after ranking, so the kind only orders the Results of one thing the user hasn't picked through.

## Considered options

- **The kind before Frecency.** Rejected: a file picked every day would lose to an app never opened until it crossed the lift threshold, a week of daily use.
- **The kind before initials or an Alias.** Rejected: initials are a better match than a word start, and an exact title wins its tie with an Alias by the user's own expectation (A2), whatever their kinds.
- **A weight per kind added to Frecency.** Rejected for the reason ADR 0006 gives against blending scores: no weight is easy to reason about.
