//! How a query names a keyword with the words it starts with, and where what
//! follows, the Argument, begins. People type what they see, not the
//! keyword's exact spelling: `new counter`, `New-Counter` and `newcounter`
//! all name `new-counter`. Only its letters count, folded as the search folds
//! them, without the spaces, hyphens and underscores that part its words.
//! An Item that takes Arguments is named with typos too, as many as the
//! search forgives.
//!
//! The whole keyword must be typed: a word cut short starts the Argument
//! instead, so the Argument can be anything, a word of the keyword included.
//!
//! The title of an Item that takes Arguments names it too, typed exactly,
//! and only by its letters and digits: the `…` of `Lock in…` or the `:` of
//! `Keyboard: set layouts` is never typed.
//!
//! # Owns
//!
//! - [`squash`] and [`is_squashed`]: a keyword as it is looked up.
//! - [`title_key`]: a title as it is looked up.
//! - [`leads`] and [`title_leads`]: the query's first words, squashed, each with what follows them.
//! - [`named`]: whether a query names one of an Item's names, and its Argument.
//! - [`takes_arguments`]: whether an Item is named with typos, and by its title.
//!
//! # Not here
//!
//! - Which keywords a query names, and how they rank: `daemon/search.rs`, `Daemon::add_named`.
//! - Filling the Arguments from what follows: `arguments.rs`.
//! - Typo distance: `rank.rs`.

use kotodama_protocol::Item;

use crate::rank;

/// How a query names a keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Naming {
    /// Its letters, whatever their case or the separators between them.
    Exact,
    /// Its letters but for as many typos as the query's words may have.
    Typo,
}

/// Whether a keyword may be named with typos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typos {
    Forgiven,
    Refused,
}

/// Whether `c` parts the words of a keyword, and is left out of it.
fn separates(c: char) -> bool {
    c.is_whitespace() || c == '-' || c == '_'
}

/// Whether `c` counts in a keyword: all but the separators.
fn in_keyword(c: char) -> bool {
    !separates(c)
}

/// Whether `c` counts in a title: its letters and digits alone.
fn in_title(c: char) -> bool {
    c.is_alphanumeric()
}

/// The letters of `keyword` as it is looked up: folded, without the
/// separators that part its words.
pub fn squash(keyword: &str) -> String {
    let mut squashed = String::with_capacity(keyword.len());
    push_kept(&mut squashed, keyword, in_keyword);
    squashed
}

/// The letters and digits of `title`, folded, as a title is looked up.
pub fn title_key(title: &str) -> String {
    let mut key = String::with_capacity(title.len());
    push_kept(&mut key, title, in_title);
    key
}

fn push_kept(squashed: &mut String, text: &str, keep: fn(char) -> bool) {
    squashed.extend(text.chars().filter(|&c| keep(c)).map(rank::fold));
}

/// Whether `item` takes Arguments: one of its own Actions has some. Only
/// then are the words after its name its Argument rather than more of the
/// search, so only then is it named with typos, or by its title.
pub fn takes_arguments(item: &Item) -> bool {
    item.actions.iter().any(|action| !action.arguments.is_empty())
}

/// [`Typos::Forgiven`] for an Item that [`takes_arguments`].
pub fn typos_for(item: &Item) -> Typos {
    if takes_arguments(item) {
        Typos::Forgiven
    } else {
        Typos::Refused
    }
}

/// Whether `keyword`, squashed, is `squashed`, without allocating.
pub fn is_squashed(keyword: &str, squashed: &str) -> bool {
    keyword
        .chars()
        .filter(|&c| !separates(c))
        .map(rank::fold)
        .eq(squashed.chars())
}

/// Calls `each` with the query's first word, then its first two, and so on:
/// squashed, as typed, and with what follows them, without the whitespace
/// that leads it. Stops once they are more than `longest` bytes squashed, or
/// `each` returns `true`.
pub fn leads<'q>(query: &'q str, longest: usize, each: impl FnMut(&str, &'q str, &'q str) -> bool) {
    leads_keeping(query, longest, in_keyword, each);
}

/// As [`leads`], with the words squashed as [`title_key`] squashes a title.
pub fn title_leads<'q>(query: &'q str, longest: usize, each: impl FnMut(&str, &'q str, &'q str) -> bool) {
    leads_keeping(query, longest, in_title, each);
}

fn leads_keeping<'q>(
    query: &'q str,
    longest: usize,
    keep: fn(char) -> bool,
    mut each: impl FnMut(&str, &'q str, &'q str) -> bool,
) {
    let query = query.trim_start();
    let mut squashed = String::new();
    let mut end = 0;
    while !query[end..].trim_start().is_empty() {
        let rest = query[end..].trim_start();
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        push_kept(&mut squashed, &rest[..word_end], keep);
        end = query.len() - rest.len() + word_end;
        if squashed.len() > longest {
            return;
        }
        if !squashed.is_empty() && each(&squashed, &query[..end], query[end..].trim_start()) {
            return;
        }
    }
}

/// How many typos the words of `typed` may have, each word counted on its
/// own, as the search counts them.
fn typos_in(typed: &str) -> usize {
    typed
        .split(separates)
        .map(|word| rank::typos(word.chars().count()))
        .sum()
}

/// What follows one of `names` in `query`, if its first words name it, and
/// how. A name typed exactly wins over one with typos, whichever comes
/// first in `names`. `distances` is the scratch the typo distance works in.
pub fn named<'q, N: AsRef<str>>(
    query: &'q str,
    names: &[N],
    typos: Typos,
    distances: &mut Vec<usize>,
) -> Option<(&'q str, Naming)> {
    let squashed: Vec<String> = names.iter().map(|name| squash(name.as_ref())).collect();
    let longest = squashed.iter().map(String::len).max()?;
    let mut exact = None;
    leads(query, longest, |lead, _, rest| {
        if squashed.iter().any(|name| name == lead) {
            exact = Some(rest);
        }
        exact.is_some()
    });
    if let Some(rest) = exact {
        return Some((rest.trim_end(), Naming::Exact));
    }
    match typos {
        Typos::Forgiven => squashed
            .iter()
            .find_map(|name| nearly(query, name, distances))
            .map(|rest| (rest, Naming::Typo)),
        Typos::Refused => None,
    }
}

/// The words of `query` that are `title`, as typed, and what follows them,
/// if its first words are the title's letters and digits exactly.
pub fn after_title<'q>(query: &'q str, title: &str) -> Option<(&'q str, &'q str)> {
    let key = title_key(title);
    let mut after = None;
    title_leads(query, key.len(), |lead, typed, rest| {
        if lead == key {
            after = Some((typed, rest.trim_end()));
        }
        after.is_some()
    });
    after
}

/// What follows the fewest of the query's first words that name the keyword
/// `squashed` with typos, as many as those words may have. The first letter
/// is never one, as in the search.
pub fn nearly<'q>(query: &'q str, squashed: &str, distances: &mut Vec<usize>) -> Option<&'q str> {
    let keyword: Vec<char> = squashed.chars().collect();
    let mut lead_chars = Vec::new();
    let mut found = None;
    leads(query, usize::MAX, |lead, typed, rest| {
        let typos = typos_in(typed);
        lead_chars.clear();
        lead_chars.extend(lead.chars());
        // A word more adds a typo for every four letters at most, so a lead
        // this far past the keyword only drifts further.
        if lead_chars.len() > keyword.len() + typos {
            return true;
        }
        if typos > 0
            && lead_chars.first() == keyword.first()
            && rank::within(&lead_chars, &keyword, typos, true, distances).is_some()
        {
            found = Some(rest);
        }
        found.is_some()
    });
    found.map(str::trim_end)
}

#[cfg(test)]
mod tests;
