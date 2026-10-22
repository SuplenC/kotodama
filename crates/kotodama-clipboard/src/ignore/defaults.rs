//! [`DEFAULT_IGNORE`](super::DEFAULT_IGNORE), matched by hand. Compiling
//! even one expression with the `regex` crate pages in its parser and
//! compilers and costs the Extension about 600 KB resident, measured with a
//! 1000-entry history; these few loops over the bytes cost nothing until a
//! copy arrives. Each function here matches what its expression in
//! `DEFAULT_IGNORE` matches, byte for byte: the expressions are ASCII-only
//! (`(?-u)`), so `\s`, `\b` and the classes are about bytes, and a byte of a
//! multi-byte character is never in one.
//!
//! # Owns
//!
//! - [`matches`]: whether any default expression matches a text, in time linear in its length, as
//!   `regex` would: copied text has no size limit.
//!
//! # Not here
//!
//! - The expressions themselves, and configured ones: `ignore.rs`.

/// A secret token: one of `prefixes`, then at least `min` bytes that `body`
/// allows. It starts a word (`\b`), and every prefix starts with a word
/// byte.
struct Token {
    prefixes: &'static [&'static str],
    body: fn(u8) -> bool,
    min: usize,
}

const TOKENS: [Token; 6] = [
    Token {
        prefixes: &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
        body: alphanumeric,
        min: 36,
    },
    Token {
        prefixes: &["github_pat_"],
        body: word,
        min: 60,
    },
    Token {
        prefixes: &["glpat-"],
        body: base64url,
        min: 20,
    },
    Token {
        prefixes: &["xoxa-", "xoxb-", "xoxp-", "xoxo-", "xoxs-", "xoxr-"],
        body: alphanumeric_or_dash,
        min: 10,
    },
    Token {
        prefixes: &["rk_live_", "rk_test_", "sk_live_", "sk_test_"],
        body: alphanumeric,
        min: 16,
    },
    Token {
        prefixes: &["sk-"],
        body: base64url,
        min: 32,
    },
];

/// Whether any expression of `DEFAULT_IGNORE` matches `text`.
pub(super) fn matches(text: &str) -> bool {
    let bytes = text.as_bytes();
    one_time_code(bytes)
        || private_key(text)
        || word_starts(bytes).any(|start| token(&bytes[start..]))
        || json_web_token(bytes)
}

/// `^\s*[0-9]{3}[ -]?[0-9]{3}\s*$`
fn one_time_code(text: &[u8]) -> bool {
    let start = text.iter().position(|&b| !space(b)).unwrap_or(text.len());
    let end = text.iter().rposition(|&b| !space(b)).map_or(start, |last| last + 1);
    let code = &text[start..end];
    let (first, second) = match code.len() {
        6 => code.split_at(3),
        7 if matches!(code[3], b' ' | b'-') => (&code[..3], &code[4..]),
        _ => return false,
    };
    first.iter().chain(second).all(u8::is_ascii_digit)
}

/// `-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----`
fn private_key(text: &str) -> bool {
    const BEGIN: &str = "-----BEGIN ";
    text.match_indices(BEGIN).any(|(at, _)| {
        let mut rest = &text.as_bytes()[at + BEGIN.len()..];
        loop {
            if rest.starts_with(b"PRIVATE KEY-----") {
                return true;
            }
            // A word is followed by a space, so it runs to the next byte
            // outside `[A-Z0-9]`, and that byte must be the space.
            let len = rest.iter().take_while(|&&b| upper_or_digit(b)).count();
            if len == 0 || rest.get(len) != Some(&b' ') {
                return false;
            }
            rest = &rest[len + 1..];
        }
    })
}

/// Where a word starts: a word byte with no word byte before it, so `\b`
/// holds before it.
fn word_starts(text: &[u8]) -> impl Iterator<Item = usize> {
    (0..text.len()).filter(|&at| word(text[at]) && (at == 0 || !word(text[at - 1])))
}

/// Whether a token with a fixed prefix starts `text`, at the start of a
/// word.
fn token(text: &[u8]) -> bool {
    TOKENS.iter().any(|token| {
        token.prefixes.iter().any(|prefix| {
            text.strip_prefix(prefix.as_bytes())
                .is_some_and(|rest| run(rest, token.body, token.min) == token.min)
        })
    }) || aws_access_key(text)
}

/// `(?:AKIA|ASIA)[A-Z0-9]{16}\b`, at the start of a word.
fn aws_access_key(text: &[u8]) -> bool {
    let Some(rest) = text.strip_prefix(b"AKIA").or_else(|| text.strip_prefix(b"ASIA")) else {
        return false;
    };
    run(rest, upper_or_digit, 16) == 16 && rest.get(16).is_none_or(|&b| !word(b))
}

/// `\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}`. No
/// part holds a dot, so the text is cut at its dots and each three pieces in
/// a row are tried: the end of the first, all of the second and the start of
/// the third.
fn json_web_token(text: &[u8]) -> bool {
    let mut pieces = text.split(|&b| b == b'.');
    let (Some(mut first), Some(mut second)) = (pieces.next(), pieces.next()) else {
        return false;
    };
    for third in pieces {
        let payload = second
            .strip_prefix(b"eyJ")
            .is_some_and(|rest| rest.len() >= 8 && rest.iter().all(|&b| base64url(b)));
        if payload && header_ends(first) && run(third, base64url, 8) == 8 {
            return true;
        }
        (first, second) = (second, third);
    }
    false
}

/// Whether `text` ends with `\beyJ[A-Za-z0-9_-]{8,}`. Within the base64url
/// bytes it ends with, a word starts after a dash, and the first `eyJ` there
/// leaves the most bytes after it.
fn header_ends(text: &[u8]) -> bool {
    let tail = &text[text.iter().rposition(|&b| !base64url(b)).map_or(0, |last| last + 1)..];
    (0..tail.len())
        .find(|&at| (at == 0 || tail[at - 1] == b'-') && tail[at..].starts_with(b"eyJ"))
        .is_some_and(|at| tail.len() - at - 3 >= 8)
}

/// How many bytes `text` starts with that `allowed` allows, counting up to
/// `max`: a run past what an expression asks for is not read, so a long one
/// is never read again from each word in it.
fn run(text: &[u8], allowed: fn(u8) -> bool, max: usize) -> usize {
    text.iter().take(max).take_while(|&&b| allowed(b)).count()
}

/// `\s` without Unicode: Rust's `u8::is_ascii_whitespace` leaves out the
/// vertical tab, which `regex` counts.
fn space(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

/// `\w` without Unicode, which `\b` is about.
fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn alphanumeric(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn alphanumeric_or_dash(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

fn base64url(b: u8) -> bool {
    word(b) || b == b'-'
}

fn upper_or_digit(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}
