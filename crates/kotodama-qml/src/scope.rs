//! The search box in an app: once the Daemon says a query is one
//! Extension's search, the box shows the rest of the query, after a chip for
//! the app, and the full query sent is the app's word, a space and the box.

/// What `query` has after `app`, an app's word, as the Daemon reads a query in
/// an app: the first word, in any case, then one whitespace character. `None`
/// if `query` doesn't start with it.
pub fn behind<'a>(query: &'a str, app: &str) -> Option<&'a str> {
    let trimmed = query.trim_start();
    let end = trimmed.find(char::is_whitespace)?;
    let word = &trimmed[..end];
    if !word.chars().flat_map(char::to_lowercase).eq(app.chars()) {
        return None;
    }
    let space = trimmed[end..].chars().next().map_or(0, char::len_utf8);
    Some(&trimmed[end + space..])
}

/// The query the Daemon gets for `text` in the box in the app of `word`.
pub fn full(word: Option<&str>, text: &str) -> String {
    match word {
        Some(word) => format!("{word} {text}"),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests;
