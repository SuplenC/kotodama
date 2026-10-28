# 32. Results rank by points that blend Tier and habit, within bounds

Status: accepted (2026-10-22); supersedes the order of ADR 0006 and ADR 0011; amended by ADR 0034 (an Item's search words count 3, as a word start) and ADR 0035 (a Weak keyword never reaches a word start)

ADR 0006 ranked Results by Tier, then Frecency, and let usage lift a Result one Tier at most. ADR 0011 put the Item kind after Frecency. Together they fixed a chain of comparisons: standing, class, Tier after the lift, Frecency, fine Tier, Alias, kind, nucleo's score, length. Habit could move a Result one Tier and no further, however often it was picked, and there was nothing for a per-Extension boost to multiply. ADR 0006 rejected a blend because no weights kept both "an exact title wins" and "habit wins". The ranking cases are tests now (`crates/kotodamad/tests/ranking.rs`), so those promises become bounds on the weights, each with a test of its own, rather than following from the shape of the comparison.

## What it does

A Result's **points** blend how well it matches with how much the user picks it:

```
points = squeeze((coarse Tier × TIER + habit) × boost) into the Tier's band
```

- **A Tier is worth `TIER` = 4,096 points.** The coarse Tiers count 0 (Near miss), 1 (scattered), 2 (inside a word), 3 (word start or initials) and 4 (exact title or keyword).
- **Habit adds points along a curve that grows with evidence:** `habit = TIER × ln(1 + Frecency) / ln(1 + lift_threshold)`, and at least 1 for any Frecency above 0. A Frecency of `lift_threshold` (5 by default, about a week of daily use) is worth one Tier. Two Tiers take `(1 + 5)² − 1 = 35`, three take 215: each further Tier takes `1 + lift_threshold` times as much use as the last. Habit climbs Tiers as picks accumulate, ever more slowly.
- **An Extension's Boost multiplies the sum.** It is `[extensions.<name>] boost`, any number above 0, 1 by default. The Daemon's own Results (app entry Items, its commands) and Unavailable Results count with 1.
- **The band** is the Tier the points reach, `min(points / TIER, 4)`. A Result in band 4 ranks with the exact matches: Class Named, which a Learned choice can beat if it was picked over it (ADR 0006, unchanged).

The order is then one `u64` per Candidate, highest first, computed once per Candidate before the sort: standing (answers first, Fallbacks last, ADR 0016), class (an Alias, the exact matches and the Learned choice), the points, then the tie-breaks: an exact title before an exact keyword, initials before a word start (ADR 0021), an Alias's Result last, the kind (ADR 0011), nucleo's score (or a Live query result's 0–1 score), the shorter title. Ties go to where the Result came from.

### The bounds

Each Tier's points have a floor and a ceiling: a Near miss `[0, TIER − 1]`; a scattered match, a match inside a word, and a word start or initials with less than a Tier of habit `[TIER, 4 × TIER − 1]`; the rest `[TIER, 2²⁶ − 1]`, all the key holds. Past `ceiling − room` the points are squeezed into the room left, `ceiling − room² / (room + excess)`, rising ever more slowly towards the ceiling and never reaching it. The room is half a Tier, or 4,096 Tiers in the top band, so a boost of 1,000 still counts in full. A hard clamp would tie every Result past the ceiling, and those are the most used ones, the Results the blend is for; squeezed, they keep their order by use as far as a point tells them apart. Below the floor, which only a Boost below 1 reaches, the points are held at the floor.

The floors and ceilings hold these whatever the Frecency and the Boost:

- **A Near miss stays below every real match**, and every real match above every Near miss: a Near miss's points stay in `[0, TIER − 1]`, a real match's start at `TIER` (ADR 0013).
- **A match inside a word or scattered never reaches the exact band**, on habit or Boost: its points stay below `4 × TIER`. Heavy use lifts it at most to a word start.
- **A word start or initials reaches the exact band only by habit**: once its habit is worth a Tier (Frecency of at least `lift_threshold`). There it passes an exact title once its habit outweighs that title's by a Tier. A Boost alone never takes it there.
- **Any pick beats no pick on an otherwise equal match**: a pick is worth at least one point, and the kind, nucleo's score and the title's length all rank below the points. So a well-used file beats a never-used app (M10), and a pick is never forgotten, only weaker, since Frecency halves but never reaches 0.
- **A Boost above 1 ranks an Extension's Results above equal matches**, and a Boost below 1 below them. A Boost below 1 is the user demoting the Extension, so it may take a Result below a better match from another Extension, down to the scattered band at worst.

Each bound has a test: `crates/kotodamad/src/points/tests.rs` over every Tier, a range of Frecencies and Boosts from 0.01 to 1000, and `crates/kotodamad/tests/ranking.rs` end to end.

### Why these weights

- **4,096 points a Tier** makes a habit point a 4,096th of a Tier, fine enough that a pick made months ago still counts for something, and it leaves room: the key keeps 26 bits of points, 16,383 Tiers' worth, and the top band's squeeze keeps any Boost within them, while a Frecency at `f32::MAX` is worth about 50.
- **A logarithmic curve** gives every pick a say and no streak of picks a runaway lead. A curve linear in Frecency would let a month of use bury every better match. Anchoring one Tier at `lift_threshold` keeps the setting's meaning and the old rule's first step: a week of daily use lifts a match one Tier, as ADR 0006 had it. The ranking tests pass unchanged.
- **The kind, nucleo's score and the length stay out of the points.** Weighed into them, each would have to be worth less than one habit point, or a kind would beat a pick, which M10 rules out. Below the points they do exactly that, at no cost. The same goes for a Live query result's 0–1 score: an Extension rating its own Results shouldn't outweigh what the user picks. The Boost is the user's lever for that.
- **Standing and class stay hard bands above the points.** An answer, a Fallback, an Alias and a Learned choice are promises about which Result ranks first, not evidence to weigh (ADR 0006, 0016).
- **Direct picks stay out of the points.** The shared Frecency of an Identity (ADR 0008) already counts every pick of the thing. The Result the user picks it through leads among that thing's Results after ranking, as before.

## Considered options

- **Keep the chain and let habit lift more Tiers.** Rejected: each Tier would need a threshold of its own, and a Boost would still have nothing to multiply.
- **A hard clamp at each ceiling.** Rejected: every Result past it ties, and the most used Results lose the order their use gives them.
- **A blend with no ceilings, the bounds left to the weights.** Rejected: a large enough Boost or Frecency would break any bound weights alone keep, and nobody could tell from the config which ones it breaks.
- **Exempt a Result picked for this very query from the exact-title bound.** Rejected for now: per-query picks already have the Learned choice, which outranks the exact title it was picked over.
- **Boost as an addition rather than a multiplier.** Rejected: an addition the size of a Tier moves a Result whatever its habit, while a multiplier scales the evidence the Result already has.

## Consequences

- Late Results never reorder the rows past ADR 0003 and ADR 0020's rules: that's unchanged.
- A Candidate stays 48 bytes, the key is computed once per Candidate, and ranking compares two `u64`s. The `instructions` benches cost no more than with the chain.
- The order a test sees is the key's order. `crates/kotodamad/src/daemon/tests.rs` checks the key against the chain of comparisons it packs, on generated Candidates.
