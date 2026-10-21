//! What the history leaves out by its text: the `ignore` setting, a list of
//! regular expressions in Rust's `regex` syntax. Copied text that any of
//! them matches, anywhere in it, is never kept. Unset, [`DEFAULT_IGNORE`]
//! leaves out what reads as a one-time code or a secret token; `ignore = []`
//! keeps everything.
//!
//! The history can't leave out what one app copies: Wayland's data-control
//! protocols offer a copy's MIME types and its bytes, and never say which
//! client made it, so there is no `ignore_apps`.
//!
//! The expressions are compiled once, when the config is read, and matched
//! when a copy arrives, on the Extension's own thread; nothing of it runs
//! while the user types in the menu.
//!
//! # Owns
//!
//! - [`Ignore`]: the compiled expressions, and whether a text matches one.
//! - [`DEFAULT_IGNORE`].
//!
//! # Not here
//!
//! - Reading the config table: `items.rs`.
//! - Dropping the copy: `main.rs`.

use regex::Regex;
use regex::RegexSet;

/// What the history leaves out unless the config's `ignore` says otherwise.
/// Each expression asks for the whole text to be a code, or for a token's
/// fixed prefix and length, so ordinary short text, numbers of other
/// lengths, hashes and words that merely start like a token are kept. They
/// match ASCII alone (`(?-u)`), as codes and tokens are, which keeps the
/// Unicode tables out of the compiled set.
pub const DEFAULT_IGNORE: [&str; 10] = [
    // A one-time code: six digits and nothing else, or two groups of three.
    // Eight digits are left alone, as a date such as 20261021 reads the same.
    r"(?-u)^\s*[0-9]{3}[ -]?[0-9]{3}\s*$",
    // A private key, PEM-encoded.
    r"(?-u)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----",
    // GitHub tokens: classic, OAuth, user-to-server, server-to-server and
    // refresh, then fine-grained.
    r"(?-u)\bgh[pousr]_[A-Za-z0-9]{36,}",
    r"(?-u)\bgithub_pat_[A-Za-z0-9_]{60,}",
    // GitLab personal access tokens.
    r"(?-u)\bglpat-[A-Za-z0-9_-]{20,}",
    // Slack tokens.
    r"(?-u)\bxox[abposr]-[A-Za-z0-9-]{10,}",
    // AWS access key ids.
    r"(?-u)\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
    // Stripe secret and restricted keys.
    r"(?-u)\b[rs]k_(?:live|test)_[A-Za-z0-9]{16,}",
    // OpenAI and Anthropic API keys: `sk-`, and at least 32 more characters.
    r"(?-u)\bsk-[A-Za-z0-9_-]{32,}",
    // A JSON Web Token: three base64url parts, the first two JSON objects.
    r"(?-u)\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
];

/// The expressions copied text is matched against, compiled once.
#[derive(Clone, Debug)]
pub struct Ignore {
    /// `None` when there are none, so a copy costs no match at all.
    set: Option<RegexSet>,
}

impl Default for Ignore {
    /// [`DEFAULT_IGNORE`].
    fn default() -> Self {
        let set = RegexSet::new(DEFAULT_IGNORE).expect("the default ignore patterns compile");
        Ignore { set: Some(set) }
    }
}

impl Ignore {
    /// Compiles `patterns`, leaving out each one that isn't a regular
    /// expression, which is returned with its error.
    #[must_use]
    pub fn new<'a>(patterns: impl IntoIterator<Item = &'a str>) -> (Self, Vec<PatternError>) {
        let mut kept = Vec::new();
        let mut errors = Vec::new();
        for pattern in patterns {
            match Regex::new(pattern) {
                Ok(_) => kept.push(pattern),
                Err(err) => errors.push(PatternError::invalid(pattern, err)),
            }
        }
        let set = if kept.is_empty() {
            None
        } else {
            RegexSet::new(&kept)
                .map_err(|err| errors.push(PatternError::TooLarge(err)))
                .ok()
        };
        (Ignore { set }, errors)
    }

    /// Whether the history leaves out `text`.
    #[must_use]
    pub fn ignores(&self, text: &str) -> bool {
        self.set.as_ref().is_some_and(|set| set.is_match(text))
    }
}

/// Why an `ignore` pattern is left out.
#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    /// It isn't a regular expression in Rust's `regex` syntax.
    #[error("`{pattern}` isn't a regular expression")]
    Invalid {
        pattern: String,
        #[source]
        source: regex::Error,
    },
    /// Each compiles, but together they are past the `regex` crate's size limit.
    #[error("the patterns are too large together")]
    TooLarge(#[source] regex::Error),
}

impl PatternError {
    fn invalid(pattern: &str, source: regex::Error) -> Self {
        PatternError::Invalid {
            pattern: pattern.to_owned(),
            source,
        }
    }
}

#[cfg(test)]
mod tests;
