//! The calculator Bundled extension: answers Live queries that look like
//! math, or that follow its `=` prefix, with one Result marked as the
//! answer, which the Daemon puts in the top row. Its primary Action copies
//! the result; the second copies `expression = result`.
//!
//! ```toml
//! [extensions.calculator]
//! # Fetch exchange rates. Off, currency uses the cached rates only.
//! network = true
//! # Where the rates come from: frankfurter's API, or anything answering like it.
//! rates_url = "https://api.frankfurter.app/latest"
//! # How decimals are written, "dot" (3.5) or "comma" (3,5); the other groups
//! # thousands. Unset, the locale's (`LC_ALL`, `LC_NUMERIC` or `LANG`).
//! decimal_separator = "comma"
//!
//! # Dates to count to by name: `days until my birthday`. A month and day
//! # comes back every year; a full date is one day.
//! [extensions.calculator.dates]
//! birthday = "mar 14"
//! launch = "2027-05-01"
//! ```
//!
//! # Owns
//!
//! - The protocol loop: Live queries, their cancelling, and Actions.
//! - The settings above.
//! - When the cached rates are read and when they are fetched, on a thread of their own, and the
//!   Live query left waiting for them.
//!
//! # Not here
//!
//! - What a query comes to: [`Calculator`], in `lib.rs`.
//! - Fetching, reading and caching the rates: `rates.rs`.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use jiff::Zoned;
use kotodama_calculator::Answer;
use kotodama_calculator::COPY;
use kotodama_calculator::COPY_EXPRESSION;
use kotodama_calculator::Calculator;
use kotodama_calculator::DecimalSeparator;
use kotodama_calculator::Evaluation;
use kotodama_calculator::NamedDate;
use kotodama_calculator::Rates;
use kotodama_calculator::rates;
use kotodama_calculator::rates::FetchError;
use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::STACK_SIZE;
use kotodama_extension::Writer;
use kotodama_extension::config;
use kotodama_extension::dirs;
use kotodama_protocol::Clipboard;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Outcome;
use kotodama_protocol::Table;
use kotodama_protocol::ToExtension;
use kotodama_protocol::Value;

/// The curl the rates are fetched with. A package may build in its path.
const CURL: &str = match option_env!("KOTODAMA_CURL") {
    Some(curl) => curl,
    None => "curl",
};

/// How long after a failed fetch the next currency query tries again.
const RETRY: Duration = Duration::from_mins(10);

/// A fetch of the exchange rates finished.
struct Fetched(Result<rates::Table, FetchError>);

/// Why an Action on an answer failed. Its message is the failed Outcome's.
#[derive(Debug, thiserror::Error)]
enum RunError {
    /// The Item's expression evaluates to nothing.
    #[error("“{0}” no longer has an answer")]
    Gone(String),
    #[error("the calculator has no Action `{0}`")]
    NoAction(String),
}

impl RunError {
    fn gone(id: &str) -> Self {
        RunError::Gone(id.to_owned())
    }

    fn no_action(action: &str) -> Self {
        RunError::NoAction(action.to_owned())
    }
}

/// An error and each of its sources, joined by `: `, for the log.
struct Chain<'a>(&'a dyn Error);

impl fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(cause) = source {
            write!(f, ": {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-calculator", run)
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect()?;

    let rates = Rates::default();
    let locale = DecimalSeparator::from_env();
    let mut calculator = Calculator::new(rates.clone());
    calculator.set_decimal_separator(locale);
    let mut extension = Extension {
        writer,
        calculator,
        locale,
        rates,
        settings: Settings::default(),
        events,
        exchange: Exchange::default(),
        waiting: None,
        last: None,
    };
    for event in received {
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Own(Fetched(fetched)) => extension.fetched(fetched)?,
            Event::Hangup => break,
        }
    }
    Ok(())
}

/// The `[extensions.calculator]` table, as documented above.
struct Settings {
    network: bool,
    rates_url: String,
    /// `None` for the locale's.
    decimal_separator: Option<DecimalSeparator>,
    dates: Vec<NamedDate>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            network: true,
            rates_url: rates::DEFAULT_URL.to_owned(),
            decimal_separator: None,
            dates: Vec::new(),
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and left
    /// at its default.
    fn from_config(table: &Table) -> Self {
        let mut settings = Settings::default();
        if let Some(network) = config::boolean(table, "network") {
            settings.network = network;
        }
        if let Some(url) = config::string(table, "rates_url") {
            url.clone_into(&mut settings.rates_url);
        }
        if let Some(name) = config::string(table, "decimal_separator") {
            settings.decimal_separator = DecimalSeparator::named(name);
            if settings.decimal_separator.is_none() {
                config::warn(format_args!(
                    "`decimal_separator` should be \"dot\" or \"comma\", not \"{name}\""
                ));
            }
        }
        if let Some(dates) = table.get("dates") {
            settings.dates = named_dates(dates);
        }
        settings
    }
}

/// The `dates` table: each date the user named, leaving out with a warning
/// any that can't be read.
fn named_dates(dates: &Value) -> Vec<NamedDate> {
    let Value::Table(dates) = dates else {
        config::warn("`dates` should be a table of names and dates, such as `birthday = \"mar 14\"`");
        return Vec::new();
    };
    let mut named = Vec::new();
    for (name, date) in dates {
        let Value::String(date) = date else {
            config::warn(format_args!(
                "`dates.{name}` should be a date written as a string, such as \"mar 14\""
            ));
            continue;
        };
        match NamedDate::new(name, date) {
            Ok(date) => named.push(date),
            Err(err) => config::warn(format_args!("`dates.{name}`: {err}")),
        }
    }
    named
}

/// Where the exchange rates stand.
#[derive(Default)]
struct Exchange {
    /// Whether the cache on disk has been read; it is, on the first currency
    /// query.
    read_cache: bool,
    /// When the rates in use were fetched.
    fetched: Option<SystemTime>,
    /// Whether a fetch is running.
    fetching: bool,
    /// When the last fetch failed.
    failed: Option<Instant>,
}

/// A Live query waiting for exchange rates.
struct Waiting {
    id: u64,
    query: String,
    routed: bool,
}

/// The running Extension: the calculator, and the state around it.
struct Extension {
    writer: Writer,
    calculator: Calculator,
    /// The locale's decimal separator, which the config may override.
    locale: DecimalSeparator,
    rates: Rates,
    settings: Settings,
    events: Sender<Event<Fetched>>,
    exchange: Exchange,
    /// The Live query waiting for rates. Any newer Live query sets it aside.
    waiting: Option<Waiting>,
    /// The last answer sent, which an Action most likely runs on.
    last: Option<Answer>,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), kotodama_protocol::Error> {
        match message {
            ToExtension::Config(config) => {
                self.settings = Settings::from_config(&config);
                let separator = self.settings.decimal_separator.unwrap_or(self.locale);
                self.calculator.set_decimal_separator(separator);
                self.calculator.set_dates(std::mem::take(&mut self.settings.dates));
                // Its value is written with the separator it was worked out with.
                self.last = None;
                Ok(())
            }
            ToExtension::LiveQuery { id, query, prefix } => {
                self.waiting = None;
                self.answer(id, query, prefix.is_some())
            }
            ToExtension::CancelLiveQuery { id } => {
                if self.waiting.as_ref().is_some_and(|waiting| waiting.id == id) {
                    self.waiting = None;
                }
                Ok(())
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => {
                let outcome = match self.run(&item.id, &action) {
                    Ok(text) => Outcome::Copy(Clipboard::Text(text)),
                    Err(err) => Outcome::Failed {
                        message: err.to_string(),
                    },
                };
                self.writer.send(&FromExtension::ActionDone { invocation, outcome })
            }
            _ => Ok(()),
        }
    }

    /// Answers the Live query `id`, or leaves it waiting for exchange rates.
    fn answer(&mut self, id: u64, query: String, routed: bool) -> Result<(), kotodama_protocol::Error> {
        let asked = self.rates.asked();
        let mut evaluation = self.calculator.evaluate(&query, routed, &Zoned::now());
        if self.rates.asked() != asked {
            self.want_rates();
            if evaluation == Evaluation::NeedsRates && self.rates.is_loaded() {
                evaluation = self.calculator.evaluate(&query, routed, &Zoned::now());
            }
        }
        let results = match evaluation {
            Evaluation::Answer(answer) => {
                let result = answer.live_result();
                self.last = Some(answer);
                vec![result]
            }
            Evaluation::NeedsRates if self.exchange.fetching => {
                self.waiting = Some(Waiting { id, query, routed });
                return Ok(());
            }
            Evaluation::NeedsRates | Evaluation::Nothing => vec![],
        };
        self.writer.send(&FromExtension::LiveResults {
            live_query: id,
            results,
            done: true,
        })
    }

    /// A query converted currency: loads the cached rates the first time, and
    /// fetches them if they are missing or more than a day old. The cache is
    /// `$XDG_CACHE_HOME/kotodama/calculator/rates.json`.
    fn want_rates(&mut self) {
        let cache = dirs::cache("calculator").map(|dir| dir.join("rates.json"));
        if !self.exchange.read_cache {
            self.exchange.read_cache = true;
            if let Some((rates, fetched)) = cache.as_deref().and_then(rates::load) {
                self.rates.set(rates);
                self.exchange.fetched = Some(fetched);
            }
        }
        let fresh = self
            .exchange
            .fetched
            .is_some_and(|fetched| fetched.elapsed().is_ok_and(|age| age < rates::MAX_AGE));
        let retry = self.exchange.failed.is_none_or(|failed| failed.elapsed() >= RETRY);
        if fresh || !self.settings.network || self.exchange.fetching || !retry {
            return;
        }
        self.exchange.fetching = true;
        let url = self.settings.rates_url.clone();
        let events = self.events.clone();
        let spawned = thread::Builder::new()
            .name("rates".into())
            .stack_size(STACK_SIZE)
            .spawn(move || {
                let fetched = rates::fetch(CURL, &url, cache.as_deref());
                let _ = events.send(Event::Own(Fetched(fetched)));
            });
        if spawned.is_err() {
            self.exchange.fetching = false;
        }
    }

    /// A fetch finished: converts with its rates, or waits [`RETRY`] before
    /// the next, and answers the Live query left waiting.
    fn fetched(&mut self, fetched: Result<rates::Table, FetchError>) -> Result<(), kotodama_protocol::Error> {
        self.exchange.fetching = false;
        match fetched {
            Ok(rates) => {
                self.rates.set(rates);
                self.exchange.fetched = Some(SystemTime::now());
                self.exchange.failed = None;
            }
            Err(err) => {
                kotodama_extension::log!("{}", Chain(&err));
                self.exchange.failed = Some(Instant::now());
            }
        }
        match self.waiting.take() {
            Some(Waiting { id, query, routed }) => self.answer(id, query, routed),
            None => Ok(()),
        }
    }

    /// What an Action on the answer `id` copies. An answer's id is its
    /// expression, so one older than the last is worked out again.
    fn run(&mut self, id: &str, action: &str) -> Result<String, RunError> {
        let answer = match &self.last {
            Some(last) if last.expression == id => last.clone(),
            _ => match self.calculator.evaluate(id, true, &Zoned::now()) {
                Evaluation::Answer(answer) => answer,
                _ => return Err(RunError::gone(id)),
            },
        };
        match action {
            COPY => Ok(answer.value),
            COPY_EXPRESSION => Ok(format!("{} = {}", answer.expression, answer.value)),
            _ => Err(RunError::no_action(action)),
        }
    }
}
