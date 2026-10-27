# 33. Frecency learns weekdays from weekends, beside the time of day

Status: accepted (2026-10-27); amends ADR 0009 and ADR 0023

ADR 0009 taught Frecency the time of day: each picked Result keeps how its picks fall across eight three-hour parts of the day, and a query reads the two parts around its time. Nothing told Monday from Sunday. A Result opened every weekday at 9:00 ranked as high on a Saturday at 9:00, and the Empty query's most used near now showed the workday tools on the weekend. ADR 0023 left the weekday for later.

## What it does

**Each picked Result keeps its shares of the day's parts twice: once for the picks made on weekdays, Monday to Friday, and once for those made on weekends.** Sixteen `u16` shares in place of eight, out of 10,000 together, 16 more bytes per picked Result; the History file grows by about 60 bytes per Result. The two kinds of day, not seven days: seven would give each day a seventh of the evidence, so a habit would take months to show, as ADR 0009 argued against finer parts of the day. Weekdays and weekends are the split most routines follow, and two kinds learn from the same picks as fast as the parts do.

**A pick counts towards both kinds of day by how near it is to the boundary**, as it counts towards the two parts of the day around it. The boundary is the user's local midnight, Friday to Saturday and Sunday to Monday, and a pick fades from one kind to the other over as long as it fades between two parts: from an hour and a half before midnight to an hour and a half after. So Friday night and Saturday morning fade into each other rather than cut. The day of the week is read from the same local clock as the time of day (`When`), so the week turns at the user's midnight, in the user's time zone.

**The weight scales the count and never gates it.** Frecency for a typed query is the decayed number of picks, scaled by how usual this time of day is for the Result and by how usual this kind of day is, each with its own weight:

```
frecency = count × (1 − time_of_day × (1 − usual time)) × (1 − day_of_week × (1 − usual kind))
```

How usual a kind of day is compares the share of the picks each kind has *per day of it*: a Result picked at the same rate every day has five parts in seven on weekdays, and both kinds are as usual as each other. A kind picked at three quarters of the other's daily rate or more counts as fully usual. The shares lean towards the kind the last pick fell on, by up to a fifth at the default half-life, since they decay with the picks; without the allowance a Result used every day would count a few percent less for half of every week. A kind never picked on counts 0. Both kinds read as usual when the shares aren't known.

The Empty query's "near now" reads the same way: the picks near this time of day on this kind of day. It still ranks by all the picks second, so on a Monday morning it shows what was used over the weekend when nothing else was.

**`[history] day_of_week`**, 0 to 1, is how much less a pick counts on the kind of day the Result is seldom picked on, as `time_of_day` is for the time of day. 0 turns it off. The default is 0.3, below `time_of_day`'s 0.5: two kinds of day are told apart from fewer picks than eight parts of a day are, so the kind of day is the weaker evidence, and the ranking tests hold at it. At 0.3, a tool picked two weekday mornings a week ranks first on a weekday morning and below a tool picked both weekend mornings on a Saturday morning, though the weekday tool was picked more recently; at 0, the more recent one ranks first on either.

**Older History files load with their picks counting in full on either kind of day**, as ADR 0009 had them count at any time of day: a file that kept one set of shares loads them split five parts to two, by the days each kind has, so both kinds read as usual exactly, and the next pick brings the shares back to their scale.

**Hot Items and the rankings made ahead** (ADR 0023) plan for the next part of the day as before. A part that starts on the other kind of day ranks with that kind's shares, since the moment it starts at is read with its own day of the week, fading at the boundary as any moment does.

## Consequences

- `Picks` grows from 32 to 48 bytes and `history.json` by about 60 bytes per picked Result: 120 KB for the 2,000 Results it keeps at most.
- `Frecent`, the struct of arrays the Empty query and a search read Frecency from, reads two more columns per Result in its vectorised loop: how usual each kind of day is, kept as `f32`. The loop keeps no branch, call or early exit. Its columns of parts are the two kinds' shares summed, kept as `f32` because the shares an old file loads with stand in proportion only and may sum past a `u16`.
- The unit tests of the time of day read a Result on a weekday after weekday picks, where they read it on whatever day fell five days after the first.
- The quantisation of the shares to 10,000 (ADR 0009) lets a share below half the decayed score stop decaying at hundreds of picks a day, since its decrement per pick rounds away. That was harmless for the parts, whose reads are normalised by the largest, and the allowance above keeps it harmless for the kinds. Finer shares, up to what a `u16` holds, would shrink it.

## Considered options

- **Seven days of the week.** Rejected: each day would learn from a seventh of the picks, so a weekly habit would take months to show, and the time-of-day shares would take 56 `u16`s.
- **A hard cut at midnight.** Rejected: a pick at 23:50 on Friday and one at 00:10 on Saturday are the same evening; the parts of the day fade for the same reason.
- **The day of the week gating the count**, so a Result never picked on weekends never shows on one. Rejected: a Result used only on weekdays is still the most used Result on a quiet Saturday, and nothing else may have been picked at all. The weight scales the count, like `time_of_day`, and Frecency never counts more than the picks.
- **Reading each kind's own parts for the time of day.** Rejected: the time of day and the kind of day would then be one factor from a quarter of the evidence each, and the loop would read four columns of parts and two of their largest shares, in place of two more columns.
- **One set of shares plus a single weekend share.** Rejected: it tells how much a Result is used on weekends but not when, so a Result used weekday mornings and weekend evenings would read its morning habit on a Saturday morning. The sixteen shares keep that open at 16 bytes.
- **One weight for both the time and the kind of day.** Rejected: the kind of day is thinner evidence, so it deserves a weight of its own, and a user who wants only one of the two can turn the other off.
