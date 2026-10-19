# 9. Frecency learns the time of day in parts of the day, and never counts more than the picks

Status: accepted (2026-10-19)

Something used every morning should rank higher in the morning, and the empty query should show what is used at this time of day. Keeping every pick's time would grow the History without bound. Instead, each Result keeps how its picks fall across eight three-hour parts of the day, next to its decayed number of picks: eight small integers, each part's share of the picks out of 10,000. They decay with the picks, so they need no time of their own, and they are fine enough that one more pick still moves them at hundreds of picks a day. A pick counts towards the two parts whose middles it falls between, by how near it is to each. A time of day reads the parts the same way, so a habit at 11:50 still counts at 12:10. The time of day is local, read from the system's clock and time zone at each pick and query, so time spent suspended and daylight saving time both count.

Frecency is the decayed number of picks, scaled by how usual this time of day is for the Result. Picks count in full at the times it is picked most, and as little as half at times it never is (the `time_of_day` setting). Frecency never counts more than the picks, so the lift threshold keeps its meaning and a Result used at all hours ranks as before. History files from before load with their picks counting in full at any time.

The empty query asks a narrower question: what is used near this time of day. It ranks by the picks near now first and by all of them second. Two things differ from ranking a typed query, because the empty query lists things the user uses, not matches:

- It shows each thing once. Of the Results of one Identity, it shows the one picked through most. This contradicts ADR 0008, which shows each Extension's Result of one thing as its own. That rule is right for a query, where each Result's Actions differ and none should hide. It is wrong for a short list of the most used, where the same thing twice would take two of three places.
- An Alias's picks count for the Result it names, though for a typed query they count for the Alias alone (R8). A Result opened every day through an Alias is among the most used.

## Considered options

- **Keep every pick's time.** Rejected: the file would grow with every pick, and ranking would read all of the times on every keystroke.
- **Hard parts of the day, with each pick in one part.** Rejected: a habit would stop counting at a part's end.
- **Six four-hour parts, or shares out of 255.** Rejected: with six parts, habits four hours apart blurred into one. With shares out of 255, a heavily used Result stopped learning new times.
- **Boost picks made at this time of day above their number.** Rejected: a Result used at one time of day would cross the lift threshold in a few days instead of a week, and one used all day would lose against it.
- **The time of day in UTC.** Rejected: a habit is kept by the user's clock.
