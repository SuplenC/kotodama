//! Arithmetic, units and currency, through fend, and what counts as math.

use std::time::{Duration, Instant};

use crate::rates::Rates;

/// How long one evaluation may take. `10^10^10` would otherwise run for
/// minutes; anything that takes this long is no answer to show as you type.
const BUDGET: Duration = Duration::from_millis(50);

/// The longest result shown. Longer ones are numbers nobody reads.
const MAX_RESULT: usize = 100;

pub enum Outcome {
    Result(String),
    NeedsRates,
    Nothing,
}

pub struct Math {
    /// Cloned for each query, so nothing one query defines (`a = 2`) leaks
    /// into the next.
    context: fend_core::Context,
    rates: Rates,
}

impl Math {
    pub fn new(rates: Rates) -> Self {
        let mut context = fend_core::Context::new();
        context.set_exchange_rate_handler_v2(rates.clone());
        Math { context, rates }
    }

    pub fn evaluate(&mut self, query: &str, routed: bool) -> Outcome {
        if !routed && !looks_like_math(query) {
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
        let shown = shown.strip_prefix("approx. ").unwrap_or(shown).trim();
        if shown.is_empty()
            || shown.len() > MAX_RESULT
            || shown.contains(char::is_control)
            || same(shown, query)
        {
            return Outcome::Nothing;
        }
        Outcome::Result(shown.to_owned())
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

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}
