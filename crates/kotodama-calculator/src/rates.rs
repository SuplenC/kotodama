//! Exchange rates: the European Central Bank's, through frankfurter.app,
//! with the euro as the base currency. They are fetched only once a currency
//! is asked for, at most once a day, and cached on disk.
//!
//! # Owns
//!
//! - [`Rates`], which fend converts with, and counting what fend asks of it.
//! - Reading frankfurter's answer, fetching it with `curl`, and its cache file.
//!
//! # Not here
//!
//! - When to read the cache and when to fetch: `main.rs`.
//! - Where the cache lives: `kotodama_extension::dirs::cache`.

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::SystemTime;

/// Where the rates come from, unless the config says otherwise.
pub const DEFAULT_URL: &str = "https://api.frankfurter.app/latest";

/// How old the cached rates may get before they are fetched again. The ECB
/// publishes once a working day.
pub const MAX_AGE: Duration = Duration::from_hours(24);

/// Exchange rates by currency code: how many units of each one euro buys.
pub type Table = HashMap<String, f64>;

/// The rates fend converts with, shared with its exchange rate handler.
/// Each rate is how many units of a currency one euro buys.
#[derive(Clone, Debug, Default)]
pub struct Rates(Arc<Mutex<State>>);

#[derive(Debug, Default)]
struct State {
    rates: Option<Table>,
    /// How many times fend has asked for a rate.
    asked: u64,
}

impl Rates {
    /// Converts with `rates` from here on.
    pub fn set(&self, rates: Table) {
        self.state().rates = Some(rates);
    }

    /// Whether any rates are set.
    pub fn is_loaded(&self) -> bool {
        self.state().rates.is_some()
    }

    /// How many times a rate has been asked for, so a caller can tell
    /// whether an evaluation converted currency.
    pub fn asked(&self) -> u64 {
        self.state().asked
    }

    /// The state, even after a panic while it was locked: every lock holder
    /// writes one field whole, so none leaves it half-written.
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fend_core::ExchangeRateFnV2 for Rates {
    fn relative_to_base_currency(
        &self,
        currency: &str,
        _options: &fend_core::ExchangeRateFnV2Options,
    ) -> Result<f64, Box<dyn Error + Send + Sync + 'static>> {
        let mut state = self.state();
        state.asked += 1;
        let rates = state.rates.as_ref().ok_or("no exchange rates yet")?;
        if currency == "EUR" {
            return Ok(1.0);
        }
        rates
            .get(currency)
            .copied()
            .ok_or_else(|| format!("no exchange rate for {currency}").into())
    }
}

/// Why frankfurter's answer couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The answer isn't JSON.
    #[error("the rates aren't JSON")]
    Json(#[source] serde_json::Error),
    /// The answer's `base` isn't `EUR`.
    #[error("the rates aren't based on the euro")]
    NotEuro,
    /// The answer has no `rates` object.
    #[error("the rates are missing")]
    Missing,
}

impl ParseError {
    fn json(err: serde_json::Error) -> Self {
        ParseError::Json(err)
    }
}

/// Why the rates couldn't be fetched.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// `curl` didn't start.
    #[error("can't run {curl}")]
    Run {
        curl: String,
        #[source]
        source: io::Error,
    },
    /// `curl` ran and failed; `stderr` is what it said.
    #[error("couldn't fetch the exchange rates: {stderr}")]
    Failed { stderr: String },
    /// The answer couldn't be read.
    #[error(transparent)]
    Parse(ParseError),
}

impl FetchError {
    fn run(curl: &str, source: io::Error) -> Self {
        FetchError::Run {
            curl: curl.to_owned(),
            source,
        }
    }

    fn failed(stderr: &[u8]) -> Self {
        FetchError::Failed {
            stderr: String::from_utf8_lossy(stderr).trim().to_owned(),
        }
    }

    fn parse(err: ParseError) -> Self {
        FetchError::Parse(err)
    }
}

/// Reads frankfurter's answer: `{"base": "EUR", "rates": {"USD": 1.08, ...}}`.
/// A rate that isn't a positive number is left out.
///
/// # Errors
///
/// If `json` isn't JSON, isn't based on the euro, or has no rates.
pub fn parse(json: &[u8]) -> Result<Table, ParseError> {
    let value: serde_json::Value = serde_json::from_slice(json).map_err(ParseError::json)?;
    if value.get("base").and_then(|base| base.as_str()) != Some("EUR") {
        return Err(ParseError::NotEuro);
    }
    let rates = value
        .get("rates")
        .and_then(|rates| rates.as_object())
        .ok_or(ParseError::Missing)?;
    Ok(rates
        .iter()
        .filter_map(|(currency, rate)| Some((currency.clone(), rate.as_f64()?)))
        .filter(|&(_, rate)| rate.is_finite() && rate > 0.0)
        .collect())
}

/// The cached rates, and when they were fetched. `None` if there is no cache
/// or it can't be read.
pub fn load(cache: &Path) -> Option<(Table, SystemTime)> {
    let json = fs::read(cache).ok()?;
    let fetched = fs::metadata(cache).and_then(|meta| meta.modified()).ok()?;
    Some((parse(&json).ok()?, fetched))
}

/// Fetches the rates, and caches them at `cache` if there is one. Runs
/// `curl`, so the Extension carries no TLS stack of its own for a request a
/// day. Rates that can't be cached are still returned.
///
/// # Errors
///
/// If `curl` can't be run, fails, or answers with something [`parse`] can't
/// read.
pub fn fetch(curl: &str, url: &str, cache: Option<&Path>) -> Result<Table, FetchError> {
    let output = Command::new(curl)
        .args(["--silent", "--show-error", "--fail", "--location"])
        .args(["--max-time", "10", "--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| FetchError::run(curl, err))?;
    if !output.status.success() {
        return Err(FetchError::failed(&output.stderr));
    }
    let rates = parse(&output.stdout).map_err(FetchError::parse)?;
    if let Some(cache) = cache
        && let Err(err) = save(cache, &output.stdout)
    {
        kotodama_extension::log!("couldn't cache the exchange rates in {}: {err}", cache.display());
    }
    Ok(rates)
}

/// Writes the rates whole, then renames them into place, so a reader never
/// sees half a file.
fn save(cache: &Path, json: &[u8]) -> io::Result<()> {
    if let Some(dir) = cache.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = cache.with_extension("json.partial");
    fs::write(&partial, json)?;
    fs::rename(&partial, cache)
}
