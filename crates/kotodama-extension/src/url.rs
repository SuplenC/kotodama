//! URL templates and opening URLs, shared by the Daemon's Quicklinks and the
//! Extensions that open web pages, so a template means the same everywhere.
//!
//! # Owns
//!
//! The query placeholder, percent-encoding, and the command that opens a
//! URL.
//!
//! # Not here
//!
//! Starting that command (`crate::launch`); the Quicklinks themselves (the
//! Daemon).

use std::fmt::Write as _;

/// The placeholder a URL template takes the typed query in.
pub const PLACEHOLDER: &str = "{query}";

/// `template` with each [`PLACEHOLDER`] replaced by `query`, percent-encoded.
pub fn fill(template: &str, query: &str) -> String {
    template.replace(PLACEHOLDER, &percent_encode(query))
}

/// The command that opens `url` in the user's browser.
pub fn opener(url: &str) -> Vec<String> {
    vec!["xdg-open".into(), url.into()]
}

/// Percent-encodes everything but RFC 3986's unreserved characters, so any
/// query fits anywhere in a URL.
pub fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests;
