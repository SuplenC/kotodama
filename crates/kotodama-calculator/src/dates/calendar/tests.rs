use jiff::civil;

use super::*;

/// Tuesday 20 October 2026.
const TODAY: Date = civil::date(2026, 10, 20);

fn read(query: &str, lean: Lean) -> Option<Date> {
    read_with(query, lean, &[])
}

fn read_with(query: &str, lean: Lean, named: &[NamedDate]) -> Option<Date> {
    let cleaned = words::clean(query);
    let words = words::words(&cleaned);
    let (date, used) = date(&words, TODAY, lean, named)?;
    (used == words.len()).then_some(date)
}

#[test]
fn easter_falls_on_its_sunday() {
    assert_eq!(easter(2024), Some(civil::date(2024, 3, 31)));
    assert_eq!(easter(2026), Some(civil::date(2026, 4, 5)));
    assert_eq!(easter(2027), Some(civil::date(2027, 3, 28)));
    assert_eq!(easter(2038), Some(civil::date(2038, 4, 25)));
}

#[test]
fn a_holiday_is_the_next_or_the_last_as_the_form_leans() {
    assert_eq!(read("christmas", Lean::Next), Some(civil::date(2026, 12, 25)));
    assert_eq!(read("christmas", Lean::Last), Some(civil::date(2025, 12, 25)));
    assert_eq!(read("easter", Lean::Next), Some(civil::date(2027, 3, 28)));
    assert_eq!(read("easter", Lean::Last), Some(civil::date(2026, 4, 5)));
    assert_eq!(read("halloween", Lean::Next), Some(civil::date(2026, 10, 31)));
}

#[test]
fn next_and_last_override_the_lean() {
    assert_eq!(read("next christmas", Lean::Last), Some(civil::date(2026, 12, 25)));
    assert_eq!(read("last christmas", Lean::Next), Some(civil::date(2025, 12, 25)));
    assert_eq!(read("last dec 25", Lean::Next), Some(civil::date(2025, 12, 25)));
}

#[test]
fn the_longest_holiday_name_wins() {
    assert_eq!(read("christmas eve", Lean::Next), Some(civil::date(2026, 12, 24)));
    assert_eq!(read("new years eve", Lean::Next), Some(civil::date(2026, 12, 31)));
    assert_eq!(read("new years day", Lean::Next), Some(civil::date(2027, 1, 1)));
}

#[test]
fn a_yearly_date_takes_a_year_after_it() {
    assert_eq!(read("christmas 2030", Lean::Next), Some(civil::date(2030, 12, 25)));
    assert_eq!(read("easter 2024", Lean::Next), Some(civil::date(2024, 3, 31)));
}

#[test]
fn a_leap_day_is_the_nearest_year_that_has_one() {
    assert_eq!(read("leap day", Lean::Next), Some(civil::date(2028, 2, 29)));
    assert_eq!(read("leap day", Lean::Last), Some(civil::date(2024, 2, 29)));
}

#[test]
fn a_weekday_is_the_first_after_today_or_the_last_before_it() {
    assert_eq!(read("friday", Lean::Next), Some(civil::date(2026, 10, 23)));
    assert_eq!(read("next fri", Lean::Last), Some(civil::date(2026, 10, 23)));
    assert_eq!(read("tuesday", Lean::Next), Some(civil::date(2026, 10, 27)));
    assert_eq!(read("last tuesday", Lean::Next), Some(civil::date(2026, 10, 13)));
    assert_eq!(read("wednsday", Lean::Next), Some(civil::date(2026, 10, 21)));
}

#[test]
fn a_month_is_read_through_one_typo_in_its_full_name() {
    assert_eq!(read("febuary 14", Lean::Next), Some(civil::date(2027, 2, 14)));
    assert_eq!(read("25 decmber", Lean::Next), Some(civil::date(2026, 12, 25)));
}

#[test]
fn a_named_date_is_read_by_its_name_and_beats_a_holiday() {
    let named = [
        NamedDate::new("Mom's birthday", "mar 14").unwrap(),
        NamedDate::new("launch", "2027-05-01").unwrap(),
        NamedDate::new("christmas", "dec 26").unwrap(),
    ];

    assert_eq!(
        read_with("moms birthday", Lean::Next, &named),
        Some(civil::date(2027, 3, 14))
    );
    assert_eq!(read_with("launch", Lean::Last, &named), Some(civil::date(2027, 5, 1)));
    assert_eq!(
        read_with("christmas", Lean::Next, &named),
        Some(civil::date(2026, 12, 26))
    );
}

#[test]
fn a_named_date_must_be_a_date_and_have_a_name() {
    assert!(NamedDate::new("anniversary", "14 june").is_ok());
    assert!(NamedDate::new("party", "may 1 2027").is_ok());
    assert!(NamedDate::new("leap", "feb 29").is_ok());
    assert_eq!(
        NamedDate::new("party", "soon"),
        Err(DateError::NotADate("soon".to_owned()))
    );
    assert_eq!(
        NamedDate::new("party", "feb 30"),
        Err(DateError::NotADate("feb 30".to_owned()))
    );
    assert_eq!(NamedDate::new("the", "mar 1"), Err(DateError::NoName("the".to_owned())));
}
