//! Narrowing: a query that only adds to the last one can only match a subset of
//! what that one matched, so it searches those alone. The search and a list
//! View's filter both narrow by these rules.
//!
//! # Owns
//! - When a query narrows the last one, and what the last one matched.
//!
//! # Not here
//! - The searches: `Daemon::refresh` over the Index, `Views::filter` over a
//!   list View's Items.
//! - Near misses, which the search narrows by rules of its own:
//!   `Daemon::add_near_misses`.

use crate::rank;

/// The characters of the matcher's syntax. A query with one may not match a
/// subset of what the same query without it matched: `!` negates, `'` and `^`
/// and `$` anchor, `\` escapes.
const SYNTAX: &[char] = &['^', '$', '!', '\'', '\\'];

/// What the last query matched, by whatever names a match: a slot of the
/// Index, a position in a list View.
pub struct Narrowing<T> {
    /// What `query` matched in the matcher, in the order searched. Every match,
    /// not only those with a Tier: a longer query can have a Tier where this
    /// one had none, as `foo ba` does inside a word where `foo b` does not.
    matched: Vec<T>,
    query: String,
    /// Whether `query` searched subtitles.
    subtitles: bool,
    /// Whether `query` was one letter, which matches only at a word start, so
    /// `matched` lacks the matches a longer query has inside a word.
    by_letter: bool,
}

impl<T> Default for Narrowing<T> {
    fn default() -> Self {
        Narrowing {
            matched: Vec::new(),
            query: String::new(),
            subtitles: false,
            by_letter: false,
        }
    }
}

impl<T> Narrowing<T> {
    /// What the last query matched, which `query`, folded as `folded`, need
    /// only search; `None` if it must search everything.
    pub fn within(&self, query: &str, folded: &rank::Query) -> Option<&[T]> {
        // Subtitles are searched from a few letters on, so the matches of a
        // query too short for them may lack some of this one's.
        let narrows = !self.query.is_empty()
            && self.subtitles == folded.searches_subtitles()
            && !self.by_letter
            && query.starts_with(&self.query)
            && !query.contains(SYNTAX);
        narrows.then_some(self.matched.as_slice())
    }

    /// Remembers that `query`, folded as `folded`, matched `matched`, every
    /// match in the matcher in the order searched.
    pub fn found(
        &mut self,
        query: &str,
        folded: &rank::Query,
        matched: impl IntoIterator<Item = T>,
    ) {
        self.matched.clear();
        self.matched.extend(matched);
        self.query.clear();
        self.query.push_str(query);
        self.subtitles = folded.searches_subtitles();
        self.by_letter = folded.filter().by_letter();
    }

    /// What the last query matched.
    pub fn matched(&self) -> &[T] {
        &self.matched
    }

    /// Forgets the last query: the next one searches everything.
    pub fn forget(&mut self) {
        self.matched.clear();
        self.query.clear();
    }
}

#[cfg(test)]
mod tests;
