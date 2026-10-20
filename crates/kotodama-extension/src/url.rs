//! URL templates and opening URLs, shared by the Daemon's Quicklinks and the
//! Extensions that open web pages, so a template means the same everywhere.

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
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_ampersands_and_non_ascii_are_encoded_as_utf_8_bytes() {
        assert_eq!(
            fill("https://example.com/?q={query}", "rock & roll żółw"),
            "https://example.com/?q=rock%20%26%20roll%20%C5%BC%C3%B3%C5%82w"
        );
    }

    #[test]
    fn unreserved_characters_stay_and_the_rest_are_encoded() {
        assert_eq!(percent_encode("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(percent_encode("a+b=c?d#e/f%"), "a%2Bb%3Dc%3Fd%23e%2Ff%25");
    }

    #[test]
    fn every_placeholder_is_filled_and_a_template_without_one_is_unchanged() {
        assert_eq!(fill("{query}/{query}", "a b"), "a%20b/a%20b");
        assert_eq!(fill("https://github.com", "x"), "https://github.com");
    }
}
