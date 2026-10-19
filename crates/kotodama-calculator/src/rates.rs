//! Exchange rates: the European Central Bank's, through frankfurter.app,
//! with the euro as the base currency. They are fetched only once a currency
//! is asked for, at most once a day, and cached on disk.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// Where the rates come from, unless the config says otherwise.
pub const DEFAULT_URL: &str = "https://api.frankfurter.app/latest";

/// How old the cached rates may get before they are fetched again. The ECB
/// publishes once a working day.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The rates fend converts with, shared with its exchange rate handler.
/// Each rate is how many units of a currency one euro buys.
#[derive(Clone, Default)]
pub struct Rates(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    rates: Option<HashMap<String, f64>>,
    /// How many times fend has asked for a rate.
    asked: u64,
}

impl Rates {
    pub fn set(&self, rates: HashMap<String, f64>) {
        self.state().rates = Some(rates);
    }

    pub fn is_loaded(&self) -> bool {
        self.state().rates.is_some()
    }

    /// How many times a rate has been asked for, so a caller can tell
    /// whether an evaluation converted currency.
    pub fn asked(&self) -> u64 {
        self.state().asked
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
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

/// Reads frankfurter's answer: `{"base": "EUR", "rates": {"USD": 1.08, ...}}`.
pub fn parse(json: &[u8]) -> Result<HashMap<String, f64>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|err| format!("the rates aren't JSON: {err}"))?;
    if value.get("base").and_then(|base| base.as_str()) != Some("EUR") {
        return Err("the rates aren't based on the euro".into());
    }
    let rates = value
        .get("rates")
        .and_then(|rates| rates.as_object())
        .ok_or("the rates are missing")?;
    Ok(rates
        .iter()
        .filter_map(|(currency, rate)| Some((currency.clone(), rate.as_f64()?)))
        .filter(|&(_, rate)| rate.is_finite() && rate > 0.0)
        .collect())
}

/// The cached rates, and when they were fetched.
pub fn load(cache: &Path) -> Option<(HashMap<String, f64>, SystemTime)> {
    let json = std::fs::read(cache).ok()?;
    let fetched = std::fs::metadata(cache)
        .and_then(|meta| meta.modified())
        .ok()?;
    Some((parse(&json).ok()?, fetched))
}

/// Fetches the rates and caches them at `cache`. Runs `curl`, so the
/// Extension carries no TLS stack of its own for a request a day.
pub fn fetch(curl: &str, url: &str, cache: &Path) -> Result<HashMap<String, f64>, String> {
    let output = Command::new(curl)
        .args(["--silent", "--show-error", "--fail", "--location"])
        .args(["--max-time", "10", "--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("can't run {curl}: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "couldn't fetch the exchange rates: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let rates = parse(&output.stdout)?;
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Written whole, then renamed, so a reader never sees half a file.
    let partial = cache.with_extension("json.partial");
    std::fs::write(&partial, &output.stdout)
        .and_then(|()| std::fs::rename(&partial, cache))
        .map_err(|err| format!("couldn't cache the exchange rates: {err}"))?;
    Ok(rates)
}
