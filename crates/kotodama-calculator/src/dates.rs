//! Dates and time zones, which fend can't do: its `today` has no clock to
//! read, and it knows no time zones. Three forms, in English:
//!
//! - date arithmetic: `today + 3 weeks`, `dec 25 - 1 month 2 days`;
//! - counting days: `days until dec 25`, `days since 2026-01-01`;
//! - a time in another zone: `5pm PT in Warsaw`, `now in Tokyo`, `17:00 UTC to Europe/Lisbon`.
//!
//! A date is `today`, `tomorrow`, `yesterday`, `2026-12-25`, `dec 25` or
//! `25 december`, with or without a year. A zone is an abbreviation from
//! [`ABBREVIATIONS`], an IANA name, or the city an IANA name ends in. Zones
//! come from the system's time zone database.
//!
//! # Owns
//!
//! - Reading the three forms above, and answering them as of a given instant.
//! - The zone abbreviations people type.
//!
//! # Not here
//!
//! - Durations and units that need no calendar (`3 weeks in days`): `math.rs`, through fend.
//! - The time zone database: the system's, read by jiff.

use jiff::Span;
use jiff::Zoned;
use jiff::civil::Date;
use jiff::civil::Time;
use jiff::tz::TimeZone;

/// Evaluates `query` as of `now`, if it has one of the forms above.
pub fn evaluate(query: &str, now: &Zoned) -> Option<String> {
    let lower = query.to_lowercase();
    let words = words(&lower);
    count_days(&words, now)
        .or_else(|| arithmetic(&words, now))
        .or_else(|| convert_time(&words, now))
}

/// The query's words, with `+` and `-` as words of their own except inside a
/// date (`2026-12-25`) or a zone name (`america/port-au-prince`).
fn words(query: &str) -> Vec<&str> {
    let mut words = Vec::new();
    for word in query.split_whitespace() {
        let mut rest = word;
        while !rest.is_empty() {
            if let Some(sign) = rest.strip_prefix(['+', '-']) {
                words.push(&rest[..1]);
                rest = sign;
                continue;
            }
            if iso_date(rest).is_some() || rest.contains('/') {
                words.push(rest);
                break;
            }
            let Some(at) = rest.find(['+', '-']) else {
                words.push(rest);
                break;
            };
            words.push(&rest[..at]);
            rest = &rest[at..];
        }
    }
    words
}

/// Which way a date without a year leans: the next `dec 25`, or the last.
#[derive(Clone, Copy, PartialEq)]
enum Lean {
    Next,
    Last,
}

/// `days until <date>` or `days since <date>`.
fn count_days(words: &[&str], now: &Zoned) -> Option<String> {
    let (lean, rest) = match words {
        ["days", "until" | "till" | "to", rest @ ..] => (Lean::Next, rest),
        ["days", "since" | "from", rest @ ..] => (Lean::Last, rest),
        _ => return None,
    };
    let today = now.date();
    let (date, used) = date(rest, today, lean)?;
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

/// `<date>` followed by at least one `+` or `-` and an amount of time.
fn arithmetic(words: &[&str], now: &Zoned) -> Option<String> {
    let (mut date, used) = date(words, now.date(), Lean::Next)?;
    let mut rest = &words[used..];
    if rest.is_empty() {
        return None;
    }
    let mut negative = false;
    while !rest.is_empty() {
        match rest[0] {
            "+" => {
                negative = false;
                rest = &rest[1..];
            }
            "-" => {
                negative = true;
                rest = &rest[1..];
            }
            _ if rest.len() == words.len() - used => return None,
            _ => {}
        }
        let [amount, unit, more @ ..] = rest else {
            return None;
        };
        let amount: i64 = amount.parse().ok()?;
        let amount = if negative { -amount } else { amount };
        let span = match *unit {
            "d" | "day" | "days" => Span::new().try_days(amount),
            "w" | "wk" | "wks" | "week" | "weeks" => Span::new().try_weeks(amount),
            "mo" | "month" | "months" => Span::new().try_months(amount),
            "y" | "yr" | "yrs" | "year" | "years" => Span::new().try_years(amount),
            _ => return None,
        }
        .ok()?;
        date = date.checked_add(span).ok()?;
        rest = more;
    }
    Some(date.strftime("%A, %-d %B %Y").to_string())
}

/// A date at the start of `words`, and how many words it took.
fn date(words: &[&str], today: Date, lean: Lean) -> Option<(Date, usize)> {
    let first = *words.first()?;
    match first {
        "today" => return Some((today, 1)),
        "tomorrow" => return Some((today.tomorrow().ok()?, 1)),
        "yesterday" => return Some((today.yesterday().ok()?, 1)),
        _ => {}
    }
    if let Some(date) = iso_date(first) {
        return Some((date, 1));
    }
    // `dec 25` or `25 dec`, then perhaps a year.
    let second = *words.get(1)?;
    let (month, day) = match (month(first), month(second)) {
        (Some(month), None) => (month, day(second)?),
        (None, Some(month)) => (month, day(first)?),
        _ => return None,
    };
    if let Some(year) = words.get(2).and_then(|word| year(word)) {
        return Some((Date::new(year, month, day).ok()?, 3));
    }
    let date = Date::new(today.year(), month, day).ok()?;
    let date = match lean {
        Lean::Next if date < today => Date::new(today.year() + 1, month, day).ok()?,
        Lean::Last if date > today => Date::new(today.year() - 1, month, day).ok()?,
        _ => date,
    };
    Some((date, 2))
}

/// A date written `2026-12-25`, or `2026-1-5`.
fn iso_date(word: &str) -> Option<Date> {
    let mut parts = word.splitn(3, '-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 || !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?).ok()
}

/// The month, 1 to 12, that a word names with at least its first three
/// letters (`dec`, `sept.`, `december,`).
fn month(word: &str) -> Option<i8> {
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let word = word.trim_end_matches(['.', ',']);
    if word.len() < 3 {
        return None;
    }
    let at = MONTHS
        .iter()
        .position(|month| month.starts_with(word) || (word == "sept" && *month == "september"))?;
    Some(at as i8 + 1)
}

/// A day of the month: `25`, `25th`, `1st,`.
fn day(word: &str) -> Option<i8> {
    let word = word.trim_end_matches(',');
    let digits = ["st", "nd", "rd", "th"]
        .iter()
        .find_map(|suffix| word.strip_suffix(suffix))
        .unwrap_or(word);
    let day: i8 = digits.parse().ok()?;
    (1..=31).contains(&day).then_some(day)
}

fn year(word: &str) -> Option<i16> {
    (word.len() == 4).then(|| word.parse().ok()).flatten()
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
