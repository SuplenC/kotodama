//! Reading a date from a query's words.
//!
//! A date is `today`, `tomorrow`, `yesterday`, `2026-12-25`, `dec 25` or `25 december`, a
//! weekday (`friday`), a holiday (`christmas`, `easter`), or a date the user named in the config
//! (`birthday`). One that comes back every year may have a year after it (`christmas 2027`);
//! without one it is the next such day or the last, as the form asks, or as `next` or `last`
//! before it says.
//!
//! # Owns
//!
//! - The ways of writing a date above, and the holidays known by name.
//! - [`NamedDate`]: a date the user named, and reading one from the config.
//!
//! # Not here
//!
//! - Cleaning and splitting the query into words, and recognising a typo: `words.rs`.
//! - What the forms do with a date: `dates.rs`.

use jiff::civil::Date;
use jiff::civil::Weekday;

use super::words;

/// Which way a date without a year leans: the next `dec 25`, or the last.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lean {
    Next,
    Last,
}

/// A day the calendar names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    /// The same month and day every year.
    Yearly {
        month: i8,
        day: i8,
    },
    Easter,
    /// One day, in one year.
    Once(Date),
}

/// Holidays known by name, without apostrophes, as [`words::clean`] leaves them. Only those kept
/// on the same day across the countries that keep them: a country's own (Thanksgiving, a national
/// day) is a [`NamedDate`] away.
const HOLIDAYS: [(&str, Day); 18] = [
    ("christmas", Day::Yearly { month: 12, day: 25 }),
    ("christmas day", Day::Yearly { month: 12, day: 25 }),
    ("xmas", Day::Yearly { month: 12, day: 25 }),
    ("christmas eve", Day::Yearly { month: 12, day: 24 }),
    ("xmas eve", Day::Yearly { month: 12, day: 24 }),
    ("new year", Day::Yearly { month: 1, day: 1 }),
    ("new years", Day::Yearly { month: 1, day: 1 }),
    ("new years day", Day::Yearly { month: 1, day: 1 }),
    ("new years eve", Day::Yearly { month: 12, day: 31 }),
    ("new year eve", Day::Yearly { month: 12, day: 31 }),
    ("halloween", Day::Yearly { month: 10, day: 31 }),
    ("valentine", Day::Yearly { month: 2, day: 14 }),
    ("valentines", Day::Yearly { month: 2, day: 14 }),
    ("valentine day", Day::Yearly { month: 2, day: 14 }),
    ("valentines day", Day::Yearly { month: 2, day: 14 }),
    ("easter", Day::Easter),
    ("easter sunday", Day::Easter),
    ("leap day", Day::Yearly { month: 2, day: 29 }),
];

/// A date the user named in the config, which every form reads where it reads a date: with
/// `birthday = "mar 14"`, `days until my birthday`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedDate {
    /// The name's words, as a query's are read.
    name: Vec<String>,
    day: Day,
}

/// Why a [`NamedDate`] can't be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateError {
    /// The name has no words a query could match.
    #[error("the date's name “{0}” has no words to type")]
    NoName(String),
    #[error("“{0}” isn't a date; write one `mar 14`, for every year, or `2027-05-01`")]
    NotADate(String),
}

impl NamedDate {
    /// The date `date` names, typed as `name`. `date` is a month and a day (`mar 14`, `14 march`)
    /// for a day every year, or a single day (`2027-05-01`, `may 1 2027`).
    ///
    /// # Errors
    ///
    /// [`DateError::NoName`] if `name` is only filler words such as `the`, and
    /// [`DateError::NotADate`] if `date` isn't written as above.
    pub fn new(name: impl AsRef<str>, date: impl AsRef<str>) -> Result<Self, DateError> {
        let name = name.as_ref();
        let date = date.as_ref();
        let cleaned = words::clean(name);
        let name_words: Vec<String> = words::words(&cleaned).into_iter().map(str::to_owned).collect();
        if name_words.is_empty() {
            return Err(DateError::NoName(name.to_owned()));
        }
        let cleaned = words::clean(date);
        let date_words = words::words(&cleaned);
        let day = match date_words[..] {
            [word] => iso_date(word).map(Day::Once),
            [_, _, year_word] => month_day(&date_words)
                .zip(year(year_word))
                .and_then(|((month, day), year)| Date::new(year, month, day).ok())
                .map(Day::Once),
            // A leap year, so that `feb 29` is a date.
            [_, _] => month_day(&date_words)
                .filter(|&(month, day)| Date::new(2024, month, day).is_ok())
                .map(|(month, day)| Day::Yearly { month, day }),
            _ => None,
        };
        let day = day.ok_or_else(|| DateError::NotADate(date.to_owned()))?;
        Ok(NamedDate { name: name_words, day })
    }
}

/// A date at the start of `words`, and how many words it took. `lean` decides a date that comes
/// back every year, unless `next` or `last` before it does.
pub fn date(words: &[&str], today: Date, lean: Lean, named: &[NamedDate]) -> Option<(Date, usize)> {
    let (lean, skipped) = match words.first() {
        Some(&("next" | "this" | "coming")) => (Lean::Next, 1),
        Some(&("last" | "previous" | "past")) => (Lean::Last, 1),
        _ => (lean, 0),
    };
    let (date, used) = unmarked(&words[skipped..], today, lean, named)?;
    Some((date, skipped + used))
}

/// [`date`], without a `next` or `last` before it.
fn unmarked(words: &[&str], today: Date, lean: Lean, named: &[NamedDate]) -> Option<(Date, usize)> {
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
    if let Some(weekday) = weekday(first) {
        let nth = if lean == Lean::Next { 1 } else { -1 };
        return Some((today.nth_weekday(nth, weekday).ok()?, 1));
    }
    let (day, used) = month_day(words)
        .map(|(month, day)| (Day::Yearly { month, day }, 2))
        .or_else(|| by_name(words, named))?;
    if let Day::Once(date) = day {
        return Some((date, used));
    }
    if let Some(year) = words.get(used).and_then(|word| year(word)) {
        return Some((day.in_year(year)?, used + 1));
    }
    Some((day.around(today, lean)?, used))
}

impl Day {
    fn in_year(self, year: i16) -> Option<Date> {
        match self {
            Day::Yearly { month, day } => Date::new(year, month, day).ok(),
            Day::Easter => easter(year),
            Day::Once(date) => Some(date),
        }
    }

    /// This year's, or the one before or after it that `lean` asks for.
    fn around(self, today: Date, lean: Lean) -> Option<Date> {
        let year = today.year();
        match (lean, self.in_year(year)) {
            (Lean::Next, Some(date)) if date >= today => Some(date),
            (Lean::Last, Some(date)) if date <= today => Some(date),
            // Missing this year (`feb 29`), or on the wrong side of today: the nearest year that
            // has one, within the eight a leap day may skip.
            _ => {
                let step = if lean == Lean::Next { 1 } else { -1 };
                (1..=8).find_map(|n| self.in_year(year.checked_add(step * n)?))
            }
        }
    }
}

/// The day a user's or a holiday's name at the start of `words` stands for, and how many words
/// the name took. The longest name wins: `christmas eve` over `christmas`.
fn by_name(words: &[&str], named: &[NamedDate]) -> Option<(Day, usize)> {
    let mut best: Option<(Day, usize)> = None;
    // The user's come first, so one of theirs wins a tie with a holiday.
    let mut consider = |used: Option<usize>, day: Day| {
        if let Some(used) = used
            && best.is_none_or(|(_, longest)| used > longest)
        {
            best = Some((day, used));
        }
    };
    for date in named {
        consider(starts_with(words, date.name.iter().map(String::as_str)), date.day);
    }
    for (name, day) in HOLIDAYS {
        consider(starts_with(words, name.split(' ')), day);
    }
    best
}

/// How many words `name` takes at the start of `words`, if `words` start with it, each word
/// perhaps with a typo.
fn starts_with<'a>(words: &[&str], name: impl Iterator<Item = &'a str>) -> Option<usize> {
    let mut used = 0;
    for part in name {
        if !words::like(words.get(used)?, part) {
            return None;
        }
        used += 1;
    }
    Some(used)
}

/// Easter Sunday in the Gregorian calendar, by the anonymous algorithm (Meeus, *Astronomical
/// Algorithms*, chapter 8).
#[expect(
    clippy::many_single_char_names,
    reason = "the letters are those the algorithm is published with, so it can be checked against it"
)]
fn easter(year: i16) -> Option<Date> {
    let y = i32::from(year);
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    Date::new(year, i8::try_from(month).ok()?, i8::try_from(day).ok()?).ok()
}

/// A date written `2026-12-25`, or `2026-1-5`.
pub fn iso_date(word: &str) -> Option<Date> {
    let mut parts = word.splitn(3, '-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if year.len() != 4 || !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?).ok()
}

/// The month and day the first two words write, as `dec 25` or `25 dec`.
fn month_day(words: &[&str]) -> Option<(i8, i8)> {
    let (first, second) = (*words.first()?, *words.get(1)?);
    match (month(first), month(second)) {
        (Some(month), None) => Some((month, day(second)?)),
        (None, Some(month)) => Some((month, day(first)?)),
        _ => None,
    }
}

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

/// The month, 1 to 12, that a word names with at least its first three letters (`dec`,
/// `sept.`, `december,`), or in full with one typo (`febuary`).
fn month(word: &str) -> Option<i8> {
    let word = word.trim_end_matches(['.', ',']);
    if word.len() < 3 {
        return None;
    }
    let at = MONTHS
        .iter()
        .position(|month| month.starts_with(word) || (word == "sept" && *month == "september"))
        .or_else(|| MONTHS.iter().position(|month| words::like(word, month)))?;
    i8::try_from(at + 1).ok()
}

/// The weekday a word names, in full (perhaps with one typo) or shortened (`fri`, `thurs`).
fn weekday(word: &str) -> Option<Weekday> {
    const WEEKDAYS: [(&str, &[&str], Weekday); 7] = [
        ("monday", &["mon"], Weekday::Monday),
        ("tuesday", &["tue", "tues"], Weekday::Tuesday),
        ("wednesday", &["wed"], Weekday::Wednesday),
        ("thursday", &["thu", "thur", "thurs"], Weekday::Thursday),
        ("friday", &["fri"], Weekday::Friday),
        ("saturday", &["sat"], Weekday::Saturday),
        ("sunday", &["sun"], Weekday::Sunday),
    ];
    WEEKDAYS
        .iter()
        .find(|(full, short, _)| words::like(word, full) || short.contains(&word))
        .map(|&(_, _, weekday)| weekday)
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

#[cfg(test)]
mod tests;
