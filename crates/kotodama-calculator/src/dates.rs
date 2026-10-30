//! Dates and time zones, which fend can't do: its `today` has no clock to
//! read, and it knows no time zones. These forms, in English, each asked
//! several ways:
//!
//! - counting days: `days until dec 25`, `how many days til xmas?`, `days since last easter`;
//! - date arithmetic: `today + 3 weeks`, `christmas - 1 month 2 days`;
//! - a date relative to today: `in 3 weeks`, `10 days from now`, `a week ago`;
//! - a time in another zone: `5pm PT in Warsaw`, `now in Tokyo`, `what time is it in Tokyo`, `tokyo
//!   time`.
//!
//! Dates are read by `calendar.rs`. A zone is an abbreviation from
//! [`ABBREVIATIONS`], an IANA name, or the city an IANA name ends in. Zones
//! come from the system's time zone database. Every word of a query must be
//! read for it to be answered, so a query that only mentions a date
//! (`christmas playlist`) never takes the top row.
//!
//! # Owns
//!
//! - The forms above, and answering them as of a given instant.
//! - The zone abbreviations people type.
//!
//! # Not here
//!
//! - Cleaning the query, its filler words, synonyms and typos: `dates/words.rs`.
//! - Reading a date, holidays and the user's named dates: `dates/calendar.rs`.
//! - Durations and units that need no calendar (`3 weeks in days`): `math.rs`, through fend.
//! - The time zone database: the system's, read by jiff.

mod calendar;
mod words;

use jiff::Span;
use jiff::Zoned;
use jiff::civil::Date;
use jiff::civil::Time;
use jiff::tz::TimeZone;

pub use self::calendar::DateError;
use self::calendar::Lean;
pub use self::calendar::NamedDate;

/// Evaluates `query` as of `now`, if it has one of the forms above, reading
/// the user's `named` dates where it reads a date.
pub fn evaluate(query: &str, now: &Zoned, named: &[NamedDate]) -> Option<String> {
    let clean = words::clean(query);
    let words = words::words(&clean);
    count_days(&words, now, named)
        .or_else(|| relative(&words, now))
        .or_else(|| arithmetic(&words, now, named))
        .or_else(|| time_elsewhere(&words, now))
        .or_else(|| convert_time(&words, now))
}

/// `days until <date>` or `days since <date>`, also asked `how many days` or
/// `how long`.
fn count_days(words: &[&str], now: &Zoned, named: &[NamedDate]) -> Option<String> {
    let (["how", "many", "days", rest @ ..] | ["how", "long", rest @ ..] | ["days", rest @ ..]) = words else {
        return None;
    };
    let (lean, rest) = match rest {
        [word, rest @ ..] if words::until(word) => (Lean::Next, rest),
        [word, rest @ ..] if words::since(word) => (Lean::Last, rest),
        _ => return None,
    };
    let today = now.date();
    let (date, used) = calendar::date(rest, today, lean, named)?;
    if used != rest.len() {
        return None;
    }
    let days = match lean {
        Lean::Next => today.until(date),
        Lean::Last => date.until(today),
    }
    .ok()?
    .get_days();
    Some(if days.abs() == 1 {
        format!("{days} day")
    } else {
        format!("{days} days")
    })
}

/// A date relative to today: `in 3 weeks`, `3 weeks from now`, `2 days ago`.
fn relative(words: &[&str], now: &Zoned) -> Option<String> {
    let today = now.date();
    let date = match words {
        ["in", amounts @ ..] | [amounts @ .., "from", "now" | "today"] => shift(today, amounts, false)?,
        [amounts @ .., "ago"] => shift(today, amounts, true)?,
        _ => return None,
    };
    Some(long_date(date))
}

/// `<date>` followed by at least one `+` or `-` and an amount of time.
fn arithmetic(words: &[&str], now: &Zoned, named: &[NamedDate]) -> Option<String> {
    let (mut date, used) = calendar::date(words, now.date(), Lean::Next, named)?;
    let mut rest = &words[used..];
    if !matches!(rest.first(), Some(&("+" | "-"))) {
        return None;
    }
    while let [sign, more @ ..] = rest {
        let end = more
            .iter()
            .position(|word| matches!(*word, "+" | "-"))
            .unwrap_or(more.len());
        date = shift(date, &more[..end], *sign == "-")?;
        rest = &more[end..];
    }
    Some(long_date(date))
}

/// `date` moved by the amounts of time `words` write (`1 year 2 days`, `a week`), back if
/// `back`; at least one.
fn shift(mut date: Date, words: &[&str], back: bool) -> Option<Date> {
    let (pairs, []) = words.as_chunks::<2>() else {
        return None;
    };
    if pairs.is_empty() {
        return None;
    }
    for [amount, unit] in pairs {
        let amount: i64 = match *amount {
            "a" | "an" => 1,
            amount => amount.parse().ok()?,
        };
        let amount = if back { -amount } else { amount };
        let span = match *unit {
            "d" | "day" | "days" => Span::new().try_days(amount),
            "w" | "wk" | "wks" | "week" | "weeks" => Span::new().try_weeks(amount),
            "mo" | "month" | "months" => Span::new().try_months(amount),
            "y" | "yr" | "yrs" | "year" | "years" => Span::new().try_years(amount),
            _ => return None,
        }
        .ok()?;
        date = date.checked_add(span).ok()?;
    }
    Some(date)
}

fn long_date(date: Date) -> String {
    date.strftime("%A, %-d %B %Y").to_string()
}

/// The current time in a zone, asked as `time in tokyo`, `what time is it in
/// tokyo` or `tokyo time`.
fn time_elsewhere(words: &[&str], now: &Zoned) -> Option<String> {
    let (["what" | "whats" | "current", "time", "in", zone @ ..] | ["time", "in", zone @ ..] | [zone @ .., "time"]) =
        words
    else {
        return None;
    };
    if zone.is_empty() {
        return None;
    }
    let asked: Vec<&str> = ["now", "in"].into_iter().chain(zone.iter().copied()).collect();
    convert_time(&asked, now)
}

/// `<time> [<zone>] in|to <zone>`, where the time is `now`, `17:00`, `5pm`,
/// `5:30 pm` or `5 pm`, and a missing first zone is the local one.
fn convert_time(words: &[&str], now: &Zoned) -> Option<String> {
    let (time, used) = time(words)?;
    let rest = &words[used..];
    let at = rest.iter().rposition(|word| matches!(*word, "in" | "to"))?;
    let (from, to) = (&rest[..at], &rest[at + 1..]);
    if to.is_empty() {
        return None;
    }
    let to = zone(&to.join(" "))?;
    let from = if from.is_empty() {
        now.time_zone().clone()
    } else {
        zone(&from.join(" "))?
    };
    let instant = match time {
        None => now.clone(),
        Some(time) => {
            let date = now.with_time_zone(from.clone()).date();
            date.to_datetime(time).to_zoned(from).ok()?
        }
    };
    let there = instant.with_time_zone(to);
    Some(there.strftime("%H:%M %Z, %A %-d %B").to_string())
}

/// A time of day at the start of `words` (`None` for `now`), and how many
/// words it took. A bare number isn't a time: `5 km in mi` is a unit.
fn time(words: &[&str]) -> Option<(Option<Time>, usize)> {
    let first = *words.first()?;
    if first == "now" {
        return Some((None, 1));
    }
    let (clock, meridiem, used) = match ["am", "pm"].iter().find(|m| first.ends_with(*m)) {
        Some(meridiem) => (&first[..first.len() - 2], Some(*meridiem), 1),
        None => match words.get(1) {
            Some(&meridiem @ ("am" | "pm")) => (first, Some(meridiem), 2),
            _ => (first, None, 1),
        },
    };
    let (hour, minute) = match clock.split_once(':') {
        Some((hour, minute)) if minute.len() == 2 => (hour, minute.parse().ok()?),
        None if meridiem.is_some() => (clock, 0),
        Some(_) | None => return None,
    };
    let hour: i8 = hour.parse().ok()?;
    let hour = match meridiem {
        Some(_) if !(1..=12).contains(&hour) => return None,
        Some("am") => hour % 12,
        Some(_) => hour % 12 + 12,
        None => hour,
    };
    Some((Some(Time::new(hour, minute, 0, 0).ok()?), used))
}

/// Abbreviations people type for a zone, and the zone each stands for. Each
/// stands for its region's zone all year: `PST` in July is Pacific time.
pub const ABBREVIATIONS: [(&str, &str); 37] = [
    ("utc", "UTC"),
    ("gmt", "UTC"),
    ("z", "UTC"),
    ("pt", "America/Los_Angeles"),
    ("pst", "America/Los_Angeles"),
    ("pdt", "America/Los_Angeles"),
    ("mt", "America/Denver"),
    ("mst", "America/Denver"),
    ("mdt", "America/Denver"),
    ("ct", "America/Chicago"),
    ("cst", "America/Chicago"),
    ("cdt", "America/Chicago"),
    ("et", "America/New_York"),
    ("est", "America/New_York"),
    ("edt", "America/New_York"),
    ("akst", "America/Anchorage"),
    ("hst", "Pacific/Honolulu"),
    ("bst", "Europe/London"),
    ("uk", "Europe/London"),
    ("wet", "Europe/Lisbon"),
    ("cet", "Europe/Berlin"),
    ("cest", "Europe/Berlin"),
    ("eet", "Europe/Athens"),
    ("eest", "Europe/Athens"),
    ("msk", "Europe/Moscow"),
    ("ist", "Asia/Kolkata"),
    ("pkt", "Asia/Karachi"),
    ("ict", "Asia/Bangkok"),
    ("sgt", "Asia/Singapore"),
    ("hkt", "Asia/Hong_Kong"),
    ("jst", "Asia/Tokyo"),
    ("kst", "Asia/Seoul"),
    ("awst", "Australia/Perth"),
    ("acst", "Australia/Adelaide"),
    ("aest", "Australia/Sydney"),
    ("aedt", "Australia/Sydney"),
    ("nzst", "Pacific/Auckland"),
];

/// The zone a name stands for: an abbreviation, an IANA name, or the city
/// an IANA name ends in (`warsaw`, `new york`).
fn zone(name: &str) -> Option<TimeZone> {
    if let Some(&(_, iana)) = ABBREVIATIONS.iter().find(|(short, _)| *short == name) {
        return jiff::tz::db().get(iana).ok();
    }
    let underscored = name.replace(' ', "_");
    if let Ok(zone) = jiff::tz::db().get(&underscored) {
        return Some(zone);
    }
    let city = jiff::tz::db().available().find(|iana| {
        let iana = iana.as_str();
        iana.contains('/')
            && iana
                .rsplit('/')
                .next()
                .is_some_and(|city| city.eq_ignore_ascii_case(&underscored))
    })?;
    jiff::tz::db().get(city.as_str()).ok()
}
