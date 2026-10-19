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

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use jiff::Zoned;
use kotodama_calculator::{Answer, COPY, COPY_EXPRESSION, Calculator, Evaluation, Rates, rates};
use kotodama_protocol::{
    Channel, ChannelWriter, Clipboard, FromExtension, Hello, Outcome, Table, ToExtension, Value,
    WireFormat,
};

/// The curl the rates are fetched with. A package may build in its path.
const CURL: &str = match option_env!("KOTODAMA_CURL") {
    Some(curl) => curl,
    None => "curl",
};

/// How long after a failed fetch the next currency query tries again.
const RETRY: Duration = Duration::from_secs(10 * 60);

enum Event {
    Message(ToExtension),
    /// A fetch of the exchange rates finished.
    Fetched(Result<rates::Table, String>),
    /// The Daemon hung up, or its messages can't be read any more.
    Hangup,
}

/// Keeps resident memory down, as the Daemon does: one malloc arena, and no
/// transparent huge pages.
fn lean_memory() {
    #[cfg(target_os = "linux")]
    // SAFETY: both calls only set process-wide flags, before any thread starts.
    unsafe {
        libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0);
        #[cfg(target_env = "gnu")]
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

fn main() -> ExitCode {
    lean_memory();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kotodama-calculator: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let channel = Channel::handshake(io::stdin(), io::stdout(), &Hello::new(&WireFormat::ALL))?;
    let (mut reader, writer) = channel.split();
    let (events, received) = mpsc::channel();

    let messages = events.clone();
    thread::Builder::new()
        .name("read".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            loop {
                match reader.recv::<ToExtension>() {
                    Ok(Some(message)) => {
                        if messages.send(Event::Message(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(err @ kotodama_protocol::Error::Decode { .. }) => {
                        eprintln!("kotodama-calculator: skipped a message: {err}");
                    }
                    Err(err) => {
                        eprintln!("kotodama-calculator: {err}");
                        break;
                    }
                }
            }
            let _ = messages.send(Event::Hangup);
        })?;

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
            Event::Fetched(fetched) => extension.fetched(fetched)?,
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
    fn from_config(config: &Table) -> Self {
        let mut settings = Settings::default();
        match config.get("network") {
            Some(Value::Bool(network)) => settings.network = *network,
            Some(_) => warn("`network` should be true or false"),
            None => {}
        }
        match config.get("rates_url") {
            Some(Value::String(url)) => settings.rates_url.clone_from(url),
            Some(_) => warn("`rates_url` should be a string"),
            None => {}
        }
        settings
    }
}

fn warn(message: &str) {
    eprintln!("kotodama-calculator: config: {message}");
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
fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(home.join("kotodama/calculator/rates.json"))
}

/// A Live query waiting for exchange rates.
struct Waiting {
    id: u64,
    query: String,
    routed: bool,
}

struct Extension {
    writer: ChannelWriter<Stdout>,
    calculator: Calculator,
    rates: Rates,
    settings: Settings,
    events: Sender<Event>,
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
        let cache = cache_path();
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
            .stack_size(256 * 1024)
            .spawn(move || {
                let _ = events.send(Event::Fetched(rates::fetch(CURL, &url, cache.as_deref())));
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
                eprintln!("kotodama-calculator: {err}");
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
