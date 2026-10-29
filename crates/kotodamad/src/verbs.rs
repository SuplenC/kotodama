//! Verbs: the words of a query that name an Action by one of its
//! `keywords`, as `remove` in `remove chrome`, and the table they are looked
//! up in.
//!
//! # Owns
//!
//! - [`Verbs`]: every Action keyword on offer, folded and sorted, built again when the Actions
//!   change, never per keystroke.
//! - [`Verb`]: the keywords a query's verb names, and the rest of the query, its subject.
//!
//! # Not here
//!
//! - Searching the subject and keeping the Results that offer the verb: `daemon/search.rs`.
//! - Choosing each Result's Action: `daemon/rows.rs`.

use kotodama_protocol::Action;

use crate::rank::fold;
use crate::rank::fold_text;

/// The fewest letters of a keyword that name it when typed as the start of
/// it: `rem` names `remove`, `re` doesn't, as two letters start too many
/// words to mean the verb.
pub const MIN_PREFIX: usize = 3;

/// The longest keyword kept, in bytes: a word longer than any verb is
/// folded on the stack, never on the heap.
const LONGEST: usize = 32;

/// Every keyword of the Actions on offer, folded as the query is, sorted,
/// without repeats.
#[derive(Debug, Default)]
pub struct Verbs {
    keywords: Vec<Box<str>>,
}

/// The verb a query names: the keywords its verb word names, and the other
/// words, which find the Results.
#[derive(Clone, Debug, PartialEq)]
pub struct Verb {
    /// Folded, sorted.
    named: Vec<Box<str>>,
    /// The query without its verb word.
    pub subject: String,
}

impl Verbs {
    /// The table of `keywords`, each folded already. A keyword with a space,
    /// or longer than any word is looked up as, is left out: a query word
    /// never names it.
    #[must_use]
    pub fn new(mut keywords: Vec<Box<str>>) -> Self {
        keywords.retain(|keyword| {
            !keyword.is_empty() && keyword.len() <= LONGEST && !keyword.contains(char::is_whitespace)
        });
        keywords.sort_unstable();
        keywords.dedup();
        Verbs { keywords }
    }

    /// The verb `query` names: its first word that is a keyword, or starts
    /// one with [`MIN_PREFIX`] letters or more, in any place, when another
    /// word is left to find the Results by. A verb alone names nothing.
    pub fn find(&self, query: &str) -> Option<Verb> {
        if self.keywords.is_empty() {
            return None;
        }
        let mut buf = [0; LONGEST];
        let mut wide = String::new();
        for (n, word) in query.split_whitespace().enumerate() {
            let folded = if word.is_ascii() {
                let Some(buf) = buf.get_mut(..word.len()) else {
                    continue;
                };
                buf.copy_from_slice(word.as_bytes());
                buf.make_ascii_lowercase();
                std::str::from_utf8(buf).unwrap_or_default()
            } else {
                wide.clear();
                wide.extend(word.chars().map(fold));
                &wide
            };
            let named = self.named(folded);
            if named.is_empty() {
                continue;
            }
            let subject: Vec<&str> = query
                .split_whitespace()
                .enumerate()
                .filter(|&(m, _)| m != n)
                .map(|(_, word)| word)
                .collect();
            if subject.is_empty() {
                return None;
            }
            let mut subject = subject.join(" ");
            // A query ending in a space asks for its last word whole.
            if query.ends_with(char::is_whitespace) {
                subject.push(' ');
            }
            return Some(Verb {
                named: named.to_vec(),
                subject,
            });
        }
        None
    }

    /// The keywords the folded `word` names: itself, and every one it starts
    /// once it has [`MIN_PREFIX`] letters.
    fn named(&self, word: &str) -> &[Box<str>] {
        let start = self.keywords.partition_point(|keyword| **keyword < *word);
        let rest = &self.keywords[start..];
        let len = if word.chars().count() < MIN_PREFIX {
            usize::from(rest.first().is_some_and(|keyword| **keyword == *word))
        } else {
            rest.partition_point(|keyword| keyword.starts_with(word))
        };
        &rest[..len]
    }
}

impl Verb {
    /// Whether the verb names `keyword`, folded already.
    pub fn names_folded(&self, keyword: &str) -> bool {
        self.named.binary_search_by(|named| (**named).cmp(keyword)).is_ok()
    }

    /// Whether the verb names one of `action`'s keywords.
    pub fn names(&self, action: &Action) -> bool {
        action
            .keywords
            .iter()
            .any(|keyword| self.names_folded(&fold_text(keyword)))
    }
}

#[cfg(test)]
mod tests;
