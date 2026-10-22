//! What the history leaves out by its text: the `ignore` setting, a list of
//! regular expressions in the syntax of Rust's `regex-lite`. Copied text that
//! any of them matches, anywhere in it, is never kept. Unset,
//! [`DEFAULT_IGNORE`] leaves out what reads as a one-time code or a secret
//! token; `ignore = []` keeps everything.
//!
//! The history can't leave out what one app copies: Wayland's data-control
//! protocols offer a copy's MIME types and its bytes, and never say which
//! client made it, so there is no `ignore_apps`.
//!
//! Configured expressions are compiled once, when the config is read; the
//! defaults are matched by hand (`ignore/defaults.rs`). Either is matched
//! when a copy arrives, on the Extension's own thread; nothing of it runs
//! while the user types in the menu.
//!
//! `regex-lite` rather than `regex`: the Extension carries the engine whether
//! or not the config sets `ignore`, and `regex`'s Unicode tables and faster
//! engines cost it about 600 KB resident with a 1000-entry history, unused.
//! `regex-lite` has the same syntax, less Unicode: `\w`, `\d`, `\s` and `\b`
//! are about ASCII, and there are no `\p{…}` classes. It matches in time
//! linear in the text, as `regex` does: copied text has no size limit.
//!
//! # Owns
//!
//! - [`Ignore`]: the compiled expressions, and whether a text matches one.
//! - [`DEFAULT_IGNORE`].
//!
//! # Not here
//!
//! - Matching the defaults: `ignore/defaults.rs`.
//! - Reading the config table: `items.rs`.
//! - Dropping the copy: `main.rs`.

use regex_lite::Regex;

/// What the history leaves out unless the config's `ignore` says otherwise.
/// Each expression asks for the whole text to be a code, or for a token's
/// fixed prefix and length, so ordinary short text, numbers of other
/// lengths, hashes and words that merely start like a token are kept. They
/// match ASCII alone, as codes and tokens are. Unset, `ignore` matches these
/// by hand, with the same result; a config that sets `ignore` and wants them
/// too lists them.
pub const DEFAULT_IGNORE: [&str; 10] = [
    // A one-time code: six digits and nothing else, or two groups of three.
    // Eight digits are left alone, as a date such as 20261021 reads the same.
    r"^\s*[0-9]{3}[ -]?[0-9]{3}\s*$",
    // A private key, PEM-encoded.
    r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----",
    // GitHub tokens: classic, OAuth, user-to-server, server-to-server and
    // refresh, then fine-grained.
    r"\bgh[pousr]_[A-Za-z0-9]{36,}",
    r"\bgithub_pat_[A-Za-z0-9_]{60,}",
    // GitLab personal access tokens.
    r"\bglpat-[A-Za-z0-9_-]{20,}",
    // Slack tokens.
    r"\bxox[abposr]-[A-Za-z0-9-]{10,}",
    // AWS access key ids.
    r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
    // Stripe secret and restricted keys.
    r"\b[rs]k_(?:live|test)_[A-Za-z0-9]{16,}",
    // OpenAI and Anthropic API keys: `sk-`, and at least 32 more characters.
    r"\bsk-[A-Za-z0-9_-]{32,}",
    // A JSON Web Token: three base64url parts, the first two JSON objects.
    r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
];

/// The expressions copied text is matched against, compiled once.
#[derive(Clone, Debug, Default)]
pub struct Ignore {
    patterns: Patterns,
}

#[derive(Clone, Debug, Default)]
enum Patterns {
    /// [`DEFAULT_IGNORE`].
    #[default]
    Default,
    /// Never empty.
    Configured(Vec<Regex>),
    /// So a copy costs no match at all.
    None,
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
                Ok(regex) => kept.push(regex),
                Err(err) => errors.push(PatternError::new(pattern, err)),
            }
        }
        let patterns = if kept.is_empty() {
            Patterns::None
        } else {
            Patterns::Configured(kept)
        };
        (Ignore { patterns }, errors)
    }

    /// Whether the history leaves out `text`.
    #[must_use]
    pub fn ignores(&self, text: &str) -> bool {
        match &self.patterns {
            Patterns::Default => defaults::matches(text),
            Patterns::Configured(regexes) => regexes.iter().any(|regex| regex.is_match(text)),
            Patterns::None => false,
        }
    }
}

/// An `ignore` pattern that isn't a regular expression in `regex-lite`'s
/// syntax, or is past its size limit, and is left out.
#[derive(Debug, thiserror::Error)]
#[error("`{pattern}` isn't a regular expression")]
pub struct PatternError {
    pattern: String,
    #[source]
    source: regex_lite::Error,
}

impl PatternError {
    fn new(pattern: &str, source: regex_lite::Error) -> Self {
        PatternError {
            pattern: pattern.to_owned(),
            source,
        }
    }

    /// The pattern as the config gave it.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

mod defaults;

#[cfg(test)]
mod tests;
