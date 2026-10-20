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
//! ```

use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use jiff::Zoned;
use kotodama_calculator::{Answer, COPY, COPY_EXPRESSION, Calculator, Evaluation, Rates, rates};
use kotodama_extension::{Connection, Event, STACK_SIZE, Writer, config, dirs};
use kotodama_protocol::{Clipboard, FromExtension, Outcome, Table, ToExtension};

/// The curl the rates are fetched with. A package may build in its path.
const CURL: &str = match option_env!("KOTODAMA_CURL") {
    Some(curl) => curl,
    None => "curl",
};

/// How long after a failed fetch the next currency query tries again.
const RETRY: Duration = Duration::from_secs(10 * 60);

/// A fetch of the exchange rates finished.
struct Fetched(Result<rates::Table, String>);

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
    let mut extension = Extension {
        writer,
        calculator: Calculator::new(rates.clone()),
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

struct Settings {
    network: bool,
    rates_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            network: true,
            rates_url: rates::DEFAULT_URL.to_owned(),
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
        settings
    }
}

/// Where the exchange rates stand.
#[derive(Default)]
struct Exchange {
    /// Whether the cache on disk has been read; it is, on the first currency
    /// query.
    read_cache: bool,
    /// When the rates in use were fetched.
    fetched: Option<SystemTime>,
    fetching: bool,
    /// When the last fetch failed.
    failed: Option<Instant>,
}

/// `$XDG_CACHE_HOME/kotodama/calculator/rates.json`.
/// A Live query waiting for exchange rates.
struct Waiting {
    id: u64,
    query: String,
    routed: bool,
}

struct Extension {
    writer: Writer,
    calculator: Calculator,
    rates: Rates,
    settings: Settings,
    events: Sender<Event<Fetched>>,
    exchange: Exchange,
    waiting: Option<Waiting>,
    /// The last answer sent, which an Action most likely runs on.
    last: Option<Answer>,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), kotodama_protocol::Error> {
        match message {
            ToExtension::Config(config) => {
                self.settings = Settings::from_config(&config);
                Ok(())
            }
            ToExtension::LiveQuery { id, query, prefix } => {
                self.waiting = None;
                self.answer(id, query, prefix.is_some())
            }
            ToExtension::CancelLiveQuery { id } => {
                if self
                    .waiting
                    .as_ref()
                    .is_some_and(|waiting| waiting.id == id)
                {
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
                    Err(message) => Outcome::Failed { message },
                };
                self.writer.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })
            }
            _ => Ok(()),
        }
    }

    /// Answers the Live query `id`, or leaves it waiting for exchange rates.
    fn answer(
        &mut self,
        id: u64,
        query: String,
        routed: bool,
    ) -> Result<(), kotodama_protocol::Error> {
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
    /// fetches them if they are missing or more than a day old.
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
        let retry = self
            .exchange
            .failed
            .is_none_or(|failed| failed.elapsed() >= RETRY);
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

    fn fetched(
        &mut self,
        fetched: Result<rates::Table, String>,
    ) -> Result<(), kotodama_protocol::Error> {
        self.exchange.fetching = false;
        match fetched {
            Ok(rates) => {
                self.rates.set(rates);
                self.exchange.fetched = Some(SystemTime::now());
                self.exchange.failed = None;
            }
            Err(err) => {
                kotodama_extension::log!("{err}");
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
    fn run(&mut self, id: &str, action: &str) -> Result<String, String> {
        let answer = match &self.last {
            Some(last) if last.expression == id => last.clone(),
            _ => match self.calculator.evaluate(id, true, &Zoned::now()) {
                Evaluation::Answer(answer) => answer,
                _ => return Err(format!("“{id}” no longer has an answer")),
            },
        };
        match action {
            COPY => Ok(answer.value),
            COPY_EXPRESSION => Ok(format!("{} = {}", answer.expression, answer.value)),
            _ => Err(format!("the calculator has no Action `{action}`")),
        }
    }
}
