# 35. Weak keywords find an Item but never rank it as a title

Status: accepted (2026-10-28); amends ADR 0032

The Apps Extension put each app's desktop id among its `keywords`, so an app is found by the name its `.desktop` file has. A Chrome web app's desktop id is `chrome-<app id>-Default`, so typing `chrome` matched every web app at a word start, the same Tier as "Google Chrome" (ADR 0034), and a web app the user opens more ranked above Chrome itself. Cutting browser prefixes out of the id would need a list of browsers and their naming schemes, which Kotodama doesn't keep: it knows nothing about any one vendor.

## What it does

- **The protocol's `Item` gains `weak_keywords`**, a list of strings, empty by default. It is added to protocol 1.0 in place, as `Action::confirm` was: 1.0 isn't released yet (ADR 0026), so `tests/layouts/1.0.txt` is pinned again. It is the last field of the Item on the wire, so an Extension that doesn't know it sends none and reads none.
- **A Weak keyword matches only as the query inside a word**: `Tier::WeakKeyword`, which counts 2 in ADR 0032's points as a match inside a word does, and loses a tie to one. It has no word starts, so it is never exact, a word start or initials, a query of one letter never matches it, and Near misses don't look at it. Letters scattered across it match nothing: an id holds enough letters to scatter most short queries across, and a scattered match's points may reach a word start's (ADR 0032). A query whose words are found partly in the keywords and partly inside a Weak keyword, as `file nautilus` is for Files, matches as `Tier::WeakKeyword` too.
- **Habit never lifts it to a word start.** Its points are held below the word start band's floor, `3 × TIER − 1`, squeezed as the other ceilings are. However often the user picks an Item found by its Weak keywords alone, a Result whose title (or keyword) starts with the query stays above it, unless the user sets that Result's Extension a Boost below 1. A match inside a title can still be passed by habit, as before.
- **A better match elsewhere on the Item wins**: a Weak keyword is only looked at when the title and the other keywords match no better than inside a word.
- **The Apps Extension sends the desktop id as a Weak keyword**, not a keyword. `chrome` still finds Google Chat's web app, below Google Chrome; `chat` finds it by its title.

## How the Index keeps them

A row's search keys (`Keys` in `segment.rs`) are 56 bytes with no padding left, and most Items have no Weak keywords, so they get no field of their own. They are appended to the row's keywords text after a marker character, `\u{1e}`, so nucleo scores the keywords and Weak keywords in one call as before. Where they begin is one more `u32` in the arena of word starts, after the keywords' word starts, flagged by the top bit of the row's count of them, which a count never reaches. An Item without Weak keywords costs nothing more; one with them costs its text, a character for the marker and four bytes.

Measured on the `kotodamad` benches, base against this change, with the 10k Items of `corpus.rs`, none of which have Weak keywords:

| Bench | Base | This change |
|---|---|---|
| Index heap, 10k Items | 4,614,145 bytes | 4,614,145 bytes |
| Index heap, emoji | 1,205,863 bytes | 1,205,863 bytes |
| Daemon resident, 10k Items | 17,473,536 bytes | 17,489,920 bytes |
| first keystroke, instructions | 2,403,055 | 2,429,601 (+1.1%) |
| next keystroke | 1,419,956 | 1,437,364 (+1.2%) |
| two words pasted | 2,935,146 | 2,938,466 (+0.1%) |
| Empty query, 2k picked | 33,857 | 34,470 (+1.8%) |
| emoji in the root search | 3,718,756 | 3,764,763 (+1.2%) |
| emoji, app scoped out | 2,397,648 | 2,424,320 (+1.1%) |
| emoji, app searched | 2,057,401 | 2,096,999 (+1.9%) |

The instructions added are a few per matched row: splitting the keywords text at the marker and asking whether there are Weak keywords to rank. The resident difference is 16 KiB, within a run's noise.

As the Apps Extension ships, every app carries its id. With each made-up app's id, `org.example.App<n>`, as a keyword and then as a Weak keyword (not committed: the corpus stays as it was, so CI compares like with like):

| Bench | Id as a keyword | Id as a Weak keyword |
|---|---|---|
| Index heap, 10k Items | 5,023,745 bytes | 5,023,745 bytes |
| first keystroke | 2,483,247 | 2,586,090 (+4.1%) |
| next keystroke | 2,374,877 | 1,453,454 (−38.8%) |
| two words pasted | 2,951,452 | 2,982,601 (+1.1%) |
| Empty query, 2k picked | 35,755 | 37,602 (+5.2%) |
| emoji in the root search | 3,832,170 | 3,805,052 (−0.7%) |

A Weak keyword costs what a keyword did in memory: the marker takes the separator's place, and the entry for where they begin takes the place of the id's word starts. A one-letter query costs a little more, as a row whose title and keywords match no better than inside a word now also looks inside its Weak keywords; a longer query costs less, as the ids no longer match it at a word start and fewer rows are ranked.

## Considered options

- **Drop the browser's prefix and profile from the id** (`chrome-`, `-Default`, and the same for Chromium, Brave and Edge). Rejected: a list of vendors' naming schemes to keep up, and Kotodama knows nothing about any one vendor.
- **Drop the desktop id from the search.** Rejected: a user who knows an app by its `.desktop` name, such as `org.gnome.Nautilus`, should still find it.
- **Cap a Weak keyword at `Tier::Inside` and nothing more.** Rejected: under ADR 0032, a week of daily use lifts a match inside a word past a word start with no habit, so the web app would still pass Chrome for a user who opens it every day.
- **Hold a Weak keyword below every title match**, inside a word included. Rejected: it would need a band below a match inside a word, and a scattered title match would then beat an id the user typed nearly whole.
- **A field of its own in each row's keys.** Rejected: every row would grow by at least 8 bytes for what few Items have.
