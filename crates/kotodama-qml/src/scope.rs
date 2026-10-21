//! The search box behind a Prefix: once the Daemon says a query is one
//! Extension's search, the box shows the rest of the query, after a chip for
//! the Extension, and the full query sent is the Prefix, a space and the box.

/// What `query` has after `prefix`, as the Daemon reads a query behind one:
/// the first word, in any case, then one whitespace character. `None` if
/// `query` doesn't start with it.
pub fn behind<'a>(query: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed = query.trim_start();
    let end = trimmed.find(char::is_whitespace)?;
    let word = &trimmed[..end];
    if !word.chars().flat_map(char::to_lowercase).eq(prefix.chars()) {
        return None;
    }
    let space = trimmed[end..].chars().next().map_or(0, char::len_utf8);
    Some(&trimmed[end + space..])
}

/// The query the Daemon gets for `text` in the box behind `prefix`.
pub fn full(prefix: Option<&str>, text: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix} {text}"),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
