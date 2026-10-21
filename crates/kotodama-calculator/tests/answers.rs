//! What the calculator answers, and what it leaves alone, for queries as the
//! Daemon sends them.

use std::time::Duration;
use std::time::Instant;

use jiff::Zoned;
use kotodama_calculator::Calculator;
use kotodama_calculator::DecimalSeparator;
use kotodama_calculator::Evaluation;
use kotodama_calculator::Rates;

/// Tuesday 20 October 2026, 10:00 in Warsaw.
fn now() -> Zoned {
    "2026-10-20T10:00:00+02:00[Europe/Warsaw]".parse().unwrap()
}

fn answer(query: &str) -> Option<String> {
    answer_with(&mut Calculator::new(Rates::default()), query)
}

fn answer_with(calculator: &mut Calculator, query: &str) -> Option<String> {
    result(calculator.evaluate(query, false, &now()))
}

/// What `query` comes to where decimals are written with a comma.
fn with_comma(query: &str) -> Option<String> {
    let mut calculator = Calculator::new(Rates::default());
    calculator.set_decimal_separator(DecimalSeparator::Comma);
    answer_with(&mut calculator, query)
}

/// What a query after the `=` prefix comes to.
fn routed(query: &str) -> Option<String> {
    result(Calculator::new(Rates::default()).evaluate(query, true, &now()))
}

fn result(evaluation: Evaluation) -> Option<String> {
    match evaluation {
        Evaluation::Answer(answer) => Some(answer.value),
        Evaluation::NeedsRates | Evaluation::Nothing => None,
    }
}

#[test]
fn simple_arithmetic_is_answered() {
    assert_eq!(answer("1+1").as_deref(), Some("2"));
    assert_eq!(answer("42*2").as_deref(), Some("84"));
    assert_eq!(answer("(3 + 4) * 2").as_deref(), Some("14"));
    assert_eq!(answer("2^10").as_deref(), Some("1024"));
    assert_eq!(answer("1/3").as_deref(), Some("0.3333333333"));
}

// The example the Manifest's intro card shows.
#[test]
fn a_duration_converts_to_seconds() {
    assert_eq!(answer("15min in sec").as_deref(), Some("900 secs"));
}

#[test]
fn input_that_is_not_math_gives_nothing() {
    for query in ["fi", "firefox", "a", "e", "version", "sin", "hello world", "2 +"] {
        assert_eq!(answer(query), None, "{query}");
    }
}

#[test]
fn a_bare_number_or_quantity_gives_nothing() {
    for query in ["42", "-5", "5 kg", "3d", "1e3", "10%", "1password", "7zip"] {
        assert_eq!(answer(query), None, "{query}");
    }
}

#[test]
fn the_prefix_answers_anything_that_evaluates() {
    assert_eq!(routed(" pi").as_deref(), Some("3.1415926536"));
    assert_eq!(routed("42"), None, "nothing to show but the query");
    assert_eq!(routed("hello"), None);
}

#[test]
fn units_convert() {
    assert_eq!(answer("5 km in mi").as_deref(), Some("3.1068559612 mi"));
    assert_eq!(answer("5 km to mi").as_deref(), Some("3.1068559612 mi"));
    assert_eq!(answer("5 ft in cm").as_deref(), Some("152.4 cm"));
    assert_eq!(answer("3 weeks in days").as_deref(), Some("21 days"));
    assert_eq!(answer("100 F to C").as_deref(), Some("37.7777777778 °C"));
}

#[test]
fn an_evaluation_that_runs_too_long_gives_nothing() {
    let started = Instant::now();
    assert_eq!(answer("10^10^10"), None);
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn currency_waits_for_rates_then_converts_with_them() {
    let rates = Rates::default();
    let mut calculator = Calculator::new(rates.clone());
    assert_eq!(
        calculator.evaluate("100 usd to eur", false, &now()),
        Evaluation::NeedsRates
    );
    assert_eq!(answer_with(&mut calculator, "1+1").as_deref(), Some("2"));

    rates.set([("USD".to_owned(), 1.25), ("PLN".to_owned(), 4.0)].into());
    assert_eq!(
        answer_with(&mut calculator, "100 usd to eur").as_deref(),
        Some("80 EUR")
    );
    assert_eq!(answer_with(&mut calculator, "10 eur in pln").as_deref(), Some("40 PLN"));
    assert_eq!(answer_with(&mut calculator, "100 xyz to eur"), None);
}

#[test]
fn dates_take_days_weeks_months_and_years() {
    assert_eq!(answer("today + 3 weeks").as_deref(), Some("Tuesday, 10 November 2026"));
    assert_eq!(
        answer("tomorrow - 1 month").as_deref(),
        Some("Monday, 21 September 2026")
    );
    assert_eq!(answer("dec 25 + 1 week").as_deref(), Some("Friday, 1 January 2027"));
    assert_eq!(
        answer("2026-12-25 - 3 days").as_deref(),
        Some("Tuesday, 22 December 2026")
    );
    assert_eq!(
        answer("today + 1 year 2 days").as_deref(),
        Some("Friday, 22 October 2027")
    );
    assert_eq!(answer("today"), None, "nothing done to it");
}

#[test]
fn days_until_and_since_count_to_the_next_or_from_the_last() {
    assert_eq!(answer("days until dec 25").as_deref(), Some("66 days"));
    assert_eq!(answer("days until 25 december").as_deref(), Some("66 days"));
    assert_eq!(answer("days until jan 1").as_deref(), Some("73 days"));
    assert_eq!(answer("days since jan 1").as_deref(), Some("292 days"));
    assert_eq!(answer("days until tomorrow").as_deref(), Some("1 day"));
    assert_eq!(answer("days until 2026-10-20").as_deref(), Some("0 days"));
}

#[test]
fn times_convert_between_time_zones() {
    assert_eq!(
        answer("5pm PT in Warsaw").as_deref(),
        Some("02:00 CEST, Wednesday 21 October")
    );
    assert_eq!(answer("now in Tokyo").as_deref(), Some("17:00 JST, Tuesday 20 October"));
    assert_eq!(
        answer("17:00 UTC to Europe/Warsaw").as_deref(),
        Some("19:00 CEST, Tuesday 20 October")
    );
    assert_eq!(
        answer("9am in new york").as_deref(),
        Some("03:00 EDT, Tuesday 20 October")
    );
    assert_eq!(answer("5pm PT in Atlantis"), None);
}

#[test]
fn an_approximate_answer_has_no_zero_padding() {
    assert_eq!(answer("42 km in mi").as_deref(), Some("26.097590074 mi"));
    assert_eq!(answer("1000000/3").as_deref(), Some("333333.333333"));
    assert_eq!(answer("2/3 m").as_deref(), Some("0.6666666667 m"));
}

#[test]
fn a_thousands_separator_is_read_as_one() {
    assert_eq!(answer("1,000 + 1").as_deref(), Some("1001"));
    assert_eq!(answer("1,000 * 2").as_deref(), Some("2000"));
    assert_eq!(answer("12,345.5 * 2").as_deref(), Some("24691"));
    assert_eq!(routed("1,000").as_deref(), Some("1000"));
}

#[test]
fn a_decimal_comma_where_a_dot_is_the_separator_gives_nothing_rather_than_a_misreading() {
    assert_eq!(answer("3,5 * 2"), None);
    assert_eq!(answer("1,5 + 1"), None);
    assert_eq!(answer("1.000,5 + 1"), None);
}

#[test]
fn with_a_decimal_comma_answers_read_and_write_one() {
    assert_eq!(with_comma("3,5 * 2").as_deref(), Some("7"));
    assert_eq!(with_comma("1,5 + 1").as_deref(), Some("2,5"));
    assert_eq!(with_comma("1/3").as_deref(), Some("0,3333333333"));
    assert_eq!(with_comma("42 km in mi").as_deref(), Some("26,097590074 mi"));
    assert_eq!(with_comma("1,5 kg in lb").as_deref(), Some("3,3069339328 lbs"));
    assert_eq!(with_comma("1.000 + 1").as_deref(), Some("1001"));
    assert_eq!(with_comma("1.000.000,5 * 2").as_deref(), Some("2000001"));
}

#[test]
fn with_a_decimal_comma_a_dot_between_digits_must_group_thousands() {
    assert_eq!(with_comma("3.5 * 2"), None);
    assert_eq!(with_comma("0.1 + 0.2"), None);
    assert_eq!(with_comma("1,000.5 + 1"), None);
}
