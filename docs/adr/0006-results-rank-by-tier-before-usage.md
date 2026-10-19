# 6. Results rank by Tier before usage, and usage only ever lifts one Tier

Status: accepted (2026-10-19)

nucleo's score alone ranked `Determinate` above `Terminal` for `term`, and a Frecency boost added to it let a well-used app with the query's letters scattered in its name beat one whose title was the query. Users expect the kind of match to come first, so Results now rank by Tier (exact, initials or word start, inside a word, scattered), then Frecency, then nucleo's score, then the shorter title. Usage still matters, but only by lifting a Result one Tier, and never above an exact title.

A Learned choice outranks everything except an Alias's Result, but only the Results that ranked above it when it was picked (the "passed over" ones); a Result added since, such as a new app titled exactly as the query, isn't one of them and wins. A Learned choice also applies to the queries that extend its query or that it extends, so it learns in both directions. Two picks of another Result since it was last picked take over from it, and one stray pick only breaks a tie.

Considered: blending Tier, Frecency and nucleo's score into one number. It's cheaper to tune per case but impossible to reason about, and no weight kept both "an exact title wins" and "habit wins" from the user's own expectations. Late Results reordering the visible rows (ADR 0003) is unchanged.
