//! A query's words as the date forms read them. People ask the same thing many ways (`days til
//! xmas`, `how many days are left until christmas?`), so the words are cleaned and the filler
//! dropped before any form looks at them, and a keyword is recognised through its synonyms and
//! one typo.
//!
//! # Owns
//!
//! - Cleaning a query and splitting it into words.
//! - The filler words dropped from every query.
//! - The synonyms of `until` and `since`, and which typos a keyword is recognised through.
//!
//! # Not here
//!
//! - What the words mean: the forms in `dates.rs`, and dates in `calendar.rs`.

/// Words that change nothing a form reads, dropped wherever they are: `how many days are there
/// left until the new year` is `how many days until new year`. Every remaining word must still
/// be read, so dropping them never makes words that were no question into one.
const FILLER: [&str; 10] = [
    "the",
    "my",
    "our",
    "is",
    "it",
    "are",
    "there",
    "left",
    "remaining",
    "still",
];

/// `query` lowercased, without apostrophes or the question or exclamation mark it ends with:
/// `new year's` reads as `new years`, and `'til` as `til`.
pub fn clean(query: &str) -> String {
    let mut clean: String = query
        .chars()
        .filter(|c| !matches!(c, '\'' | '’'))
        .flat_map(char::to_lowercase)
        .collect();
    let end = clean.trim_end_matches(['?', '!', ' ']).len();
    clean.truncate(end);
    clean
}

/// The words of a [`clean`]ed query, without the filler, and with `+` and `-` as words of their
/// own except inside a date (`2026-12-25`) or a zone name (`america/port-au-prince`).
pub fn words(clean: &str) -> Vec<&str> {
    let mut words = Vec::new();
    for word in clean.split_whitespace() {
        let mut rest = word;
        while !rest.is_empty() {
            if let Some(sign) = rest.strip_prefix(['+', '-']) {
                words.push(&rest[..1]);
                rest = sign;
                continue;
            }
            if super::calendar::iso_date(rest).is_some() || rest.contains('/') {
                words.push(rest);
                break;
            }
            let Some(at) = rest.find(['+', '-']) else {
                words.push(rest);
                break;
            };
            words.push(&rest[..at]);
            rest = &rest[at..];
        }
    }
    words.retain(|word| !FILLER.contains(word));
    words
}

/// Whether `word` is `until` or says the same: `til`, `till`, `to`, `before`, or `until` with
/// one typo.
pub fn until(word: &str) -> bool {
    matches!(word, "til" | "till" | "to" | "before") || like(word, "until")
}

/// Whether `word` is `since` or says the same: `from`, or `since` with one typo.
pub fn since(word: &str) -> bool {
    word == "from" || like(word, "since")
}

/// Whether `word` is `keyword`, or a keyword of five letters or more with one typo in it: a
/// letter left out, added, changed, or two swapped (`untill`, `christams`). A shorter keyword
/// must be exact, as one typo away from it are too many other words.
pub fn like(word: &str, keyword: &str) -> bool {
    word == keyword || (keyword.len() >= 5 && word.len() >= 4 && one_edit(word.as_bytes(), keyword.as_bytes()))
}

/// Whether `a` and `b` differ by exactly one letter left out, added, changed, or two adjacent
/// letters swapped.
fn one_edit(a: &[u8], b: &[u8]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let Some(at) = short.iter().zip(long).position(|(x, y)| x != y) else {
        return long.len() == short.len() + 1;
    };
    match long.len() - short.len() {
        0 => {
            short[at + 1..] == long[at + 1..]
                || (at + 1 < short.len()
                    && short[at] == long[at + 1]
                    && short[at + 1] == long[at]
                    && short[at + 2..] == long[at + 2..])
        }
        1 => short[at..] == long[at + 1..],
        _ => false,
    }
}

#[cfg(test)]
mod tests;
