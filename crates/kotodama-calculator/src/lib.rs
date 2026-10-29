//! The calculator Bundled extension's evaluation: what a query comes to, and
//! the Live query result that shows it.
//!
//! # Owns
//!
//! - [`Calculator`]: trying a query as a date or time, then as math.
//! - [`Answer`] and its Live query result, with the Actions that copy it.
//!
//! # Not here
//!
//! - Arithmetic, units and currency through fend: `math.rs`.
//! - Dates and time zones: `dates.rs`.
//! - Fetching and caching exchange rates: `rates.rs`.
//! - The protocol loop, settings and Actions: `main.rs`.

#![forbid(unsafe_code)]

mod dates;
mod locale;
mod math;
pub mod rates;

pub use jiff::Zoned;
use kotodama_protocol::Action;
use kotodama_protocol::Fraction;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::LiveResult;
use kotodama_protocol::NamespacedKind;

pub use crate::locale::DecimalSeparator;
pub use crate::rates::Rates;

/// The primary Action: copies the result.
pub const COPY: &str = "copy";
/// Copies `expression = result`.
pub const COPY_EXPRESSION: &str = "copy-expression";

/// What a query evaluates to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// The query comes to something worth showing.
    Answer(Answer),
    /// The query converts currency, and no exchange rates are loaded yet.
    NeedsRates,
    /// The query isn't math, or doesn't parse.
    Nothing,
}

/// A query and what it comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The query, trimmed.
    pub expression: String,
    /// What the query comes to, as it is shown and copied: `84`,
    /// `3.1068559612 mi`. An approximate number has at most 12 significant
    /// digits and no trailing zeros.
    pub value: String,
}

/// Evaluates queries. One lives as long as the Extension.
#[derive(Debug)]
pub struct Calculator {
    math: math::Math,
}

impl Calculator {
    /// A calculator that converts currency with `rates`, and reads decimals
    /// with a dot.
    #[must_use]
    pub fn new(rates: Rates) -> Self {
        Calculator {
            math: math::Math::new(rates),
        }
    }

    /// Reads and writes decimals with `separator`: `3,5` is three and a half
    /// with [`DecimalSeparator::Comma`], and a thousands separator anywhere
    /// but between groups of a thousand (`3,5` with [`DecimalSeparator::Dot`])
    /// leaves a query unanswered. A new `Calculator` uses
    /// [`DecimalSeparator::Dot`].
    pub fn set_decimal_separator(&mut self, separator: DecimalSeparator) {
        self.math.set_decimal_separator(separator);
    }

    /// Evaluates `query` as of `now`. `routed` says the user asked for the
    /// calculator outright, with its `=` prefix: then anything that evaluates
    /// is answered, while a query that only looks like math must have an
    /// operation in it, so `42` or `fi` alone answers nothing.
    pub fn evaluate(&mut self, query: &str, routed: bool, now: &Zoned) -> Evaluation {
        let query = query.trim();
        if query.is_empty() {
            return Evaluation::Nothing;
        }
        let answer = |value: String| {
            Evaluation::Answer(Answer {
                expression: query.to_owned(),
                value,
            })
        };
        if let Some(value) = dates::evaluate(query, now) {
            return answer(value);
        }
        match self.math.evaluate(query, routed) {
            math::Outcome::Value(value) => answer(value),
            math::Outcome::NeedsRates => Evaluation::NeedsRates,
            math::Outcome::Nothing => Evaluation::Nothing,
        }
    }
}

impl Answer {
    /// The answer as the Live query result the Daemon shows in the top row.
    #[must_use]
    pub fn live_result(&self) -> LiveResult {
        LiveResult {
            item: Item {
                id: self.expression.clone(),
                kind: ItemKind::Namespaced(kind()),
                identity: None,
                title: self.value.clone(),
                subtitle: Some(self.expression.clone()),
                icon: Some("accessories-calculator".into()),
                keyword: None,
                keywords: vec![],
                hidden: false,
                app_only: false,
                weak_keywords: Vec::new(),
                yields: false,
                actions: vec![
                    Action::new(COPY, "Copy Answer"),
                    Action::new(COPY_EXPRESSION, "Copy Expression and Answer"),
                ],
            },
            score: certain(),
            answer: true,
        }
    }
}

fn kind() -> NamespacedKind {
    NamespacedKind::new("calculator.answer").expect("`calculator.answer` is a valid kind")
}

/// An answer's score: the highest there is.
fn certain() -> Fraction {
    Fraction::new(1.0).expect("1 is a fraction")
}
