//! The calculator Bundled extension's evaluation: arithmetic, units and
//! currency through fend, and dates and time zones of its own (see
//! [`dates`]). `main.rs` speaks the protocol around it.

mod dates;
mod math;
pub mod rates;

pub use jiff::Zoned;
use kotodama_protocol::{Action, Fraction, Item, ItemKind, LiveResult, NamespacedKind};

pub use rates::Rates;

/// The primary Action: copies the result.
pub const COPY: &str = "copy";
/// Copies `expression = result`.
pub const COPY_EXPRESSION: &str = "copy-expression";

/// What a query evaluates to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
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
    /// What the query comes to, as it is shown and copied: `84`, `3.1068559612 mi`.
    pub value: String,
}

/// Evaluates queries. One lives as long as the Extension.
pub struct Calculator {
    math: math::Math,
}

impl Calculator {
    pub fn new(rates: Rates) -> Self {
        Calculator {
            math: math::Math::new(rates),
        }
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
    pub fn live_result(&self) -> LiveResult {
        LiveResult {
            item: Item {
                id: self.expression.clone(),
                kind: ItemKind::Namespaced(
                    NamespacedKind::new("calculator.answer").expect("a valid kind"),
                ),
                identity: None,
                title: self.value.clone(),
                subtitle: Some(self.expression.clone()),
                icon: Some("accessories-calculator".into()),
                keyword: None,
                keywords: vec![],
                hidden: false,
                actions: vec![
                    Action::new(COPY, "Copy Answer"),
                    Action::new(COPY_EXPRESSION, "Copy Expression and Answer"),
                ],
            },
            score: Fraction::new(1.0).expect("1 is a fraction"),
            answer: true,
        }
    }
}
