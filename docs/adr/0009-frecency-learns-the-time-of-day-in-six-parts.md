# 9. Frecency learns the time of day in six parts of the day, and never counts more than the picks

Status: accepted (2026-10-19)

Something used every morning should rank higher in the morning, and the empty query should show what is used at this time of day. Keeping every pick's time would grow the History without bound, so each Result keeps, next to its decayed number of picks, how those picks fall across six four-hour parts of the day: six small integers, each part's share of the picks out of 255. They decay with the picks, so they need no time of their own. A pick counts towards the two parts whose middles it falls between, by how near it is to each, and a time of day reads them the same way, so a habit at 11:50 still counts at 12:10.

Frecency is the decayed number of picks, scaled by how usual this time of day is for the Result: in full at the times it is picked most, and down to half (the `time_of_day` setting) at times it never is. It never counts more than the picks, so the lift threshold keeps its meaning, a Result used at all hours ranks as before, and History files from before load with their picks counting in full at any time. The empty query asks the narrower question, what is used near this time of day, so it ranks by the picks near now first and by all of them second.

## Considered options

- **Keep every pick's time.** Rejected: the file would grow with every pick, and ranking would read all of them on every keystroke.
- **Hard parts of the day, each pick in one.** Rejected: a habit would stop counting at a part's end.
- **Boost picks made at this time of day above their number.** Rejected: a Result used at one time of day would cross the lift threshold in a few days instead of a week, and one used all day would lose against it.
- **The time of day in UTC.** Rejected: a habit is kept by the user's clock, so the time of day is read in the local time zone, daylight saving time included.
