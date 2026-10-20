//! The `[extensions.ai]` settings.
//!
//! # Owns
//!
//! - Reading the config table, and the defaults.
//! - Whether the Extension is on at all.
//!
//! # Not here
//!
//! - Running `api_key_command`: `key.rs`.

use std::time::Duration;

use kotodama_extension::config;
use kotodama_protocol::Table;
use kotodama_protocol::Value;

/// The Claude API's Messages endpoint.
pub const DEFAULT_URL: &str = "https://api.anthropic.com/v1/messages";
/// The fastest Claude model, as the user waits on it with the menu open.
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";
/// Picking from a list needs little thinking, and every token is latency.
pub const DEFAULT_EFFORT: &str = "low";
/// Long enough that a pause between words while typing a question doesn't
/// send half of it, short enough not to feel like waiting.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(600);

/// The `[extensions.ai]` settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Prints the API key. Unset, with `url` unset too, the Extension is off.
    pub api_key_command: Option<Vec<String>>,
    /// A Messages API endpoint: the Claude API, or a local server that speaks
    /// the same API, such as Ollama.
    pub url: String,
    /// Whether `url` was set in the config: a local endpoint may need no key.
    pub url_set: bool,
    pub model: String,
    /// `output_config.effort`; `None` leaves it out, for a server without it.
    pub effort: Option<String>,
    /// How long a `?` query must stay unchanged before it goes to the model.
    pub debounce: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            api_key_command: None,
            url: DEFAULT_URL.to_owned(),
            url_set: false,
            model: DEFAULT_MODEL.to_owned(),
            effort: Some(DEFAULT_EFFORT.to_owned()),
            debounce: DEFAULT_DEBOUNCE,
        }
    }
}

impl Settings {
    /// Reads the config table. A setting of the wrong shape is logged and
    /// left at its default.
    #[must_use]
    pub fn from_config(table: &Table) -> Self {
        let mut settings = Settings {
            api_key_command: config::command(table, "api_key_command"),
            ..Settings::default()
        };
        if let Some(url) = config::string(table, "url") {
            url.clone_into(&mut settings.url);
            settings.url_set = true;
        }
        if let Some(model) = config::string(table, "model") {
            model.clone_into(&mut settings.model);
        }
        if let Some(effort) = config::string(table, "effort") {
            settings.effort = (!effort.is_empty()).then(|| effort.to_owned());
        }
        match table.get("debounce_ms") {
            Some(Value::Integer(ms)) if let Ok(ms) = u64::try_from(*ms) => {
                settings.debounce = Duration::from_millis(ms);
            }
            Some(_) => config::warn("`debounce_ms` should be a whole number of milliseconds"),
            None => {}
        }
        settings
    }

    /// Off until configured, with a key or with an endpoint of the user's
    /// own. Off, the Extension adds no Results and makes no network calls.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.api_key_command.is_some() || self.url_set
    }
}

#[cfg(test)]
mod tests;
