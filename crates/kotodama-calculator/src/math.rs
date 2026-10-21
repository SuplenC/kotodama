//! Arithmetic, units and currency, through fend, and what counts as math.

use std::time::Duration;
use std::time::Instant;

use crate::locale::DecimalSeparator;
use crate::rates::Rates;

/// How long one evaluation may take. `10^10^10` would otherwise run for
/// minutes; anything that takes this long is no answer to show as you type.
const BUDGET: Duration = Duration::from_millis(50);

/// The longest result shown. Longer ones are numbers nobody reads.
const MAX_RESULT: usize = 100;

/// The most significant digits an approximate number is shown with. fend
/// writes 10 decimals whatever the number's size (`26.0975900740 mi`,
/// `333333.3333333333`); 12 digits keep all 10 below 100, and cut larger
/// numbers down to what anyone reads.
const SIGNIFICANT_DIGITS: usize = 12;

pub enum Outcome {
    Value(String),
    NeedsRates,
    Nothing,
}

pub struct Math {
    /// Cloned for each query, so nothing one query defines (`a = 2`) leaks
    /// into the next.
    context: fend_core::Context,
    rates: Rates,
    separator: DecimalSeparator,
}

impl Math {
    pub fn new(rates: Rates) -> Self {
        let mut context = fend_core::Context::new();
        context.set_exchange_rate_handler_v2(rates.clone());
        Math {
            context,
            rates,
            separator: DecimalSeparator::Dot,
        }
    }

    pub fn set_decimal_separator(&mut self, separator: DecimalSeparator) {
        self.separator = separator;
        self.context.set_decimal_separator_style(separator.fend());
    }

    pub fn evaluate(&mut self, query: &str, routed: bool) -> Outcome {
        if !routed && !looks_like_math(query) {
            return Outcome::Nothing;
        }
        if !grouped_in_thousands(query, self.separator) {
            return Outcome::Nothing;
        }
        let asked = self.rates.asked();
        let mut context = self.context.clone();
        let deadline = Deadline(Instant::now() + BUDGET);
        let result = fend_core::evaluate_with_interrupt(query, &mut context, &deadline);
        let result = match result {
            Ok(result) => result,
            Err(_) if self.rates.asked() != asked && !self.rates.is_loaded() => {
                return Outcome::NeedsRates;
            }
            Err(_) => return Outcome::Nothing,
        };
        if result.output_is_empty() {
            return Outcome::Nothing;
        }
        let shown = result.get_main_result();
        let rounded;
        let shown = match shown.strip_prefix("approx. ") {
            Some(approximate) => {
                rounded = round(approximate, self.separator.decimal());
                rounded.trim()
            }
            None => shown.trim(),
        };
        if shown.is_empty() || shown.len() > MAX_RESULT || shown.contains(char::is_control) || same(shown, query) {
            return Outcome::Nothing;
        }
        Outcome::Value(shown.to_owned())
    }
}

/// Whether a query typed without the `=` prefix should be answered: it has a
/// number in it, and does something to it, whether an operator, a conversion
/// or a function. A number alone (`42`), a quantity alone (`5 kg`) and words
/// (`fi`, which fend reads as femto-inches) are left to the other Results.
fn looks_like_math(query: &str) -> bool {
    if !query.contains(|c: char| c.is_ascii_digit()) {
        return false;
    }
    // A leading sign belongs to the number.
    let body = query.trim_start_matches(['-', '+', ' ']);
    if body.contains(['+', '-', '*', '/', '^', '×', '÷', '%', '(', '!', '√']) {
        return true;
    }
    let lower = body.to_lowercase();
    lower.split_whitespace().any(|word| {
        matches!(
            word,
            "to" | "in" | "as" | "of" | "mod" | "per" | "times" | "plus" | "minus"
        )
    }) || FUNCTIONS.iter().any(|function| lower.starts_with(function))
}

/// Functions a query may start with, as in `sqrt 2`.
const FUNCTIONS: [&str; 13] = [
    "sqrt", "cbrt", "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "exp", "abs", "round",
];

/// Whether fend only echoed the query back, as for `42`, `5 kg` or `-5`:
/// equal but for case and spaces.
fn same(result: &str, query: &str) -> bool {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect()
    };
    squash(result) == squash(query)
}

/// Whether every thousands separator in `query` stands between groups of a
/// thousand, as in `1,000,000` or `12,345.5`: one to three digits before the
/// first, three after each, and none among the decimals. fend reads one
/// anywhere between digits, so `3,5` would be 35 where a dot is the decimal
/// separator, which is a misreading rather than an answer.
fn grouped_in_thousands(query: &str, separator: DecimalSeparator) -> bool {
    let (decimal, thousands) = (separator.decimal(), separator.thousands());
    let bytes = query.as_bytes();
    for (at, &byte) in bytes.iter().enumerate() {
        let (before, after) = (&bytes[..at], &bytes[at + 1..]);
        if byte != thousands
            || !before.last().is_some_and(u8::is_ascii_digit)
            || !after.first().is_some_and(u8::is_ascii_digit)
        {
            continue;
        }
        let digits_before = before.iter().rev().take_while(|b| b.is_ascii_digit()).count();
        let digits_after = after.iter().take_while(|b| b.is_ascii_digit()).count();
        let in_decimals = before.len() > digits_before && before[before.len() - digits_before - 1] == decimal;
        if digits_before > 3 || digits_after != 3 || in_decimals {
            return false;
        }
    }
    true
}

/// `approximate`, as fend writes it, with each decimal number in it rounded
/// to [`SIGNIFICANT_DIGITS`] and its trailing zeros dropped:
/// `26.0975900740 mi` is `26.097590074 mi`. A whole part longer than that
/// keeps every digit, and loses its decimals.
fn round(approximate: &str, decimal: u8) -> String {
    let bytes = approximate.as_bytes();
    let mut rounded = String::with_capacity(approximate.len());
    let mut copied = 0;
    for (at, &byte) in bytes.iter().enumerate() {
        if byte != decimal
            || at < copied
            || !bytes[..at].last().is_some_and(u8::is_ascii_digit)
            || !bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
        {
            continue;
        }
        let start = at - bytes[..at].iter().rev().take_while(|b| b.is_ascii_digit()).count();
        let end = at + 1 + bytes[at + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        rounded.push_str(&approximate[copied..start]);
        round_number(&mut rounded, &bytes[start..at], &bytes[at + 1..end], decimal);
        copied = end;
    }
    rounded.push_str(&approximate[copied..]);
    rounded
}

/// Writes the number `whole`, `decimal`, `decimals` (ASCII digits) to `out`,
/// rounded half up to [`SIGNIFICANT_DIGITS`], without trailing zeros.
fn round_number(out: &mut String, whole: &[u8], decimals: &[u8], decimal: u8) {
    let mut digits: Vec<u8> = [whole, decimals].concat();
    let mut whole_len = whole.len();
    let Some(first) = digits.iter().position(|&digit| digit != b'0') else {
        out.push('0');
        return;
    };
    let kept = (first + SIGNIFICANT_DIGITS).max(whole_len).min(digits.len());
    let up = digits.get(kept).is_some_and(|&digit| digit >= b'5');
    digits.truncate(kept);
    if up {
        let carried = match digits.iter().rposition(|&digit| digit != b'9') {
            Some(at) => {
                digits[at] += 1;
                at + 1
            }
            None => {
                digits.insert(0, b'1');
                whole_len += 1;
                1
            }
        };
        digits[carried..].fill(b'0');
    }
    let end = whole_len
        + digits[whole_len..]
            .iter()
            .rposition(|&digit| digit != b'0')
            .map_or(0, |at| at + 1);
    let push = |out: &mut String, digits: &[u8]| out.extend(digits.iter().map(|&digit| char::from(digit)));
    push(out, &digits[..whole_len]);
    if end > whole_len {
        out.push(char::from(decimal));
        push(out, &digits[whole_len..end]);
    }
}

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}

#[cfg(test)]
mod tests;
