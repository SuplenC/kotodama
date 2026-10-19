//! How well an Item's text matches a query, in tiers that the Daemon ranks by
//! before anything else: an exact title beats a word that starts with the
//! query, which beats the query inside a word, which beats its letters
//! scattered. nucleo decides what matches at all and breaks ties within a
//! tier.
//!
//! Text is folded before it's compared: lowercase, with accents dropped
//! (`Żółw` is `zolw`), so neither case nor accents in the query matter.

use kotodama_protocol::Item;
use nucleo_matcher::pattern::Pattern;
use nucleo_matcher::{Matcher, Utf32Str, Utf32String, chars};

/// How well a text matches, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The query's letters in order, with gaps.
    Scattered,
    /// The query inside a word, or a word of a subtitle.
    Inside,
    /// A word starts with the query.
    WordStart,
    /// The query is the first letters of consecutive words, as `vsc` is of
    /// Visual Studio Code. Ranks as a word start, ahead of one on a tie.
    Initials,
    /// The whole title, or one of the Item's keywords, is the query.
    Exact,
}

impl Tier {
    /// The tier as ranking compares it, where initials count as a word start.
    pub fn coarse(self) -> u8 {
        match self {
            Tier::Scattered => 0,
            Tier::Inside => 1,
            Tier::WordStart | Tier::Initials => 2,
            Tier::Exact => 3,
        }
    }
}

/// The coarse tier one above `coarse`: where a Result the user picks often
/// ranks.
pub fn lift(coarse: u8) -> u8 {
    (coarse + 1).min(Tier::Exact.coarse())
}

/// A query, folded for comparing with [`Tiers`].
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Each word, without nucleo's syntax. Negated words are left out.
    words: Vec<Vec<char>>,
    /// The words joined by one space.
    whole: Vec<char>,
    /// The query ends in whitespace, so its last word must be a whole word.
    last_is_whole: bool,
}

/// One character, lowercase and without accents. Letters nucleo doesn't fold,
/// such as the Polish `ł`, are folded here.
pub fn fold(c: char) -> char {
    match chars::to_lower_case(chars::normalize(chars::to_lower_case(c))) {
        'ł' => 'l',
        'đ' | 'ð' => 'd',
        'ø' => 'o',
        'ħ' => 'h',
        'ŧ' => 't',
        'ı' => 'i',
        c => c,
    }
}

impl Query {
    pub fn new(text: &str) -> Self {
        let mut query = Query {
            last_is_whole: text.ends_with(char::is_whitespace),
            ..Query::default()
        };
        for word in text.split_whitespace() {
            if word.starts_with('!') {
                continue;
            }
            let word = word.trim_start_matches(['^', '\'']).trim_end_matches('$');
            if word.is_empty() {
                continue;
            }
            let word: Vec<char> = word.chars().map(fold).collect();
            if !query.whole.is_empty() {
                query.whole.push(' ');
            }
            query.whole.extend(&word);
            query.words.push(word);
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether the query is long enough for subtitles to be searched.
    pub fn searches_subtitles(&self) -> bool {
        self.words.iter().map(Vec::len).sum::<usize>() >= SUBTITLE_LETTERS
    }

    /// What lets [`Haystack::score`] pass over most Items without searching
    /// them.
    pub fn filter(&self) -> Filter {
        let letter = match self.words.as_slice() {
            [word] if word.len() == 1 && !self.last_is_whole => Some(initial(word[0])),
            _ => None,
        };
        let subtitles = self.searches_subtitles().then(|| {
            self.words
                .iter()
                .map(|word| initial(word[0]))
                .fold(0, |a, b| a | b)
        });
        Filter { letter, subtitles }
    }
}

/// Word initials an Item's texts must have to be worth searching, as
/// [`initial`] gives them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    /// A one-letter query's letter, which only matches at a word start, so the
    /// title or a keyword must have a word starting with it.
    letter: Option<u64>,
    /// The first letters of the query's words, which a subtitle's words must
    /// start with for it to match, if it's long enough for subtitles to be
    /// searched.
    subtitles: Option<u64>,
}

/// The fewest letters a query needs for subtitles to be searched: fewer find
/// too much.
pub const SUBTITLE_LETTERS: usize = 3;

/// Text folded, character for character, for nucleo to match: lowercase and
/// without accents.
pub fn fold_text(text: &str) -> String {
    text.chars().map(fold).collect()
}

/// What separates an Item's keywords in [`Haystack`]: none has it, and it
/// starts a word like a space does.
const SEPARATOR: char = '\u{1f}';

/// What an Item is searched by, folded as [`fold`] does and converted once on
/// the way in. A folded text has as many characters as the original, so match
/// positions are the same in both.
pub struct Haystack {
    pub title: Utf32String,
    /// The keyword and keywords, joined by [`SEPARATOR`]; empty if there are
    /// none.
    words: Utf32String,
    /// Empty if there is none.
    subtitle: Utf32String,
    /// The letters words of the title and keywords start with, as
    /// [`initial`] gives them, so a one-letter query passes over most Items
    /// without searching them.
    initials: u64,
    /// The same for the subtitle.
    subtitle_initials: u64,
}

/// A character as a bit of a set of word initials: one of 36 for a folded
/// ASCII letter or digit, and one shared by every other character.
fn initial(c: char) -> u64 {
    match c {
        'a'..='z' => 1 << (c as u32 - 'a' as u32),
        '0'..='9' => 1 << (26 + c as u32 - '0' as u32),
        _ => 1 << 63,
    }
}

/// The letters words of `folded` start with, as [`initial`] gives them, with
/// words starting as [`one_letter`] has them. The bit every other character
/// shares is always set, so the set never wrongly rules a text out.
fn initials(folded: &str, original: Option<&str>) -> u64 {
    let mut set = 1 << 63;
    let mut cases = original
        .filter(|original| original.len() == folded.chars().count())
        .map(str::chars);
    let (mut prev, mut prev_lower) = (None, false);
    for c in folded.chars() {
        let lower = cases
            .as_mut()
            .and_then(Iterator::next)
            .map(|o| (o.is_lowercase(), o.is_uppercase()));
        let camel = prev_lower && lower.is_some_and(|(_, upper)| upper);
        if camel || prev.is_none_or(|p: char| !p.is_alphanumeric()) {
            set |= initial(c);
        }
        prev = Some(c);
        prev_lower = lower.is_some_and(|(lower, _)| lower);
    }
    set
}

/// `text` as a [`Haystack`] holds it.
pub fn folded(text: &str) -> Utf32String {
    Utf32String::from(fold_text(text))
}

impl Haystack {
    pub fn new(item: &Item) -> Self {
        let mut words = String::new();
        for word in item.keyword.iter().chain(&item.keywords) {
            if !words.is_empty() {
                words.push(SEPARATOR);
            }
            words.push_str(word);
        }
        let title = fold_text(&item.title);
        let words = fold_text(&words);
        let subtitle = fold_text(item.subtitle.as_deref().unwrap_or(""));
        Haystack {
            initials: initials(&title, Some(&item.title)) | initials(&words, None),
            subtitle_initials: initials(&subtitle, None),
            title: Utf32String::from(title),
            words: Utf32String::from(words),
            subtitle: Utf32String::from(subtitle),
        }
    }

    /// nucleo's score, the best of the title's and the keywords', and which of
    /// them matched. Failing both, the subtitle's, if `filter` lets subtitles
    /// be searched and the subtitle has words starting with every word of the
    /// query. `None` if none matches.
    pub fn score(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
    ) -> Option<(u32, Matched)> {
        if let Some(letter) = filter.letter
            && self.initials & letter == 0
        {
            return None;
        }
        let title = pattern.score(self.title.slice(..), matcher);
        let words = if self.words.is_empty() {
            None
        } else {
            pattern.score(self.words.slice(..), matcher)
        };
        let matched = Matched {
            title: title.is_some(),
            words: words.is_some(),
            subtitle: false,
        };
        match title.max(words) {
            Some(score) => Some((score, matched)),
            None if filter
                .subtitles
                .is_some_and(|initials| self.subtitle_initials & initials == initials) =>
            {
                pattern
                    .score(self.subtitle.slice(..), matcher)
                    .map(|score| (score, Matched::SUBTITLE))
            }
            None => None,
        }
    }
}

/// Which of an Item's texts matched a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matched {
    pub title: bool,
    pub words: bool,
    pub subtitle: bool,
}

impl Matched {
    /// Every text, when it isn't known which matched.
    pub const ALL: Matched = Matched {
        title: true,
        words: true,
        subtitle: true,
    };
    const SUBTITLE: Matched = Matched {
        title: false,
        words: false,
        subtitle: true,
    };
}

/// Works out tiers, reusing its buffers from one text to the next.
#[derive(Default)]
pub struct Tiers {
    chars: Vec<char>,
    /// Where each word of `chars` starts.
    starts: Vec<usize>,
}

impl Tiers {
    /// How well an Item matches: the best of its title and its keywords, or
    /// failing those, its subtitle. A subtitle counts only for a query of at
    /// least [`SUBTITLE_LETTERS`] letters, only at word starts, and then only
    /// as [`Tier::Inside`]. Only the texts in `matched` are looked at. `None`
    /// if nothing matches well enough.
    pub fn item(
        &mut self,
        haystack: &Haystack,
        item: &Item,
        query: &Query,
        matched: Matched,
    ) -> Option<Tier> {
        let mut best = None;
        if matched.title {
            best = self.text(haystack.title.slice(..), Some(&item.title), query);
        }
        if matched.words && best < Some(Tier::Exact) && !haystack.words.is_empty() {
            best = best.max(self.text(haystack.words.slice(..), None, query));
        }
        if best.is_none()
            && matched.subtitle
            && query.searches_subtitles()
            && !haystack.subtitle.is_empty()
            && self.text(haystack.subtitle.slice(..), None, query) >= Some(Tier::WordStart)
        {
            best = Some(Tier::Inside);
        }
        best
    }

    /// How well one folded text matches: the worst of the query's words, or
    /// `Exact` if the whole text, or one of its parts between
    /// [`SEPARATOR`]s, is the query. A one-letter word only matches at the
    /// start of a word. Words start after a character that isn't a letter or
    /// digit, and, when `original` gives the case, at a capital after a small
    /// letter, as in `WebStorm`.
    fn text(&mut self, folded: Utf32Str, original: Option<&str>, query: &Query) -> Option<Tier> {
        if query.is_empty() {
            return None;
        }
        if let [word] = query.words.as_slice()
            && let [letter] = word.as_slice()
            && !query.last_is_whole
        {
            return one_letter(*letter, folded, original);
        }
        self.chars.clear();
        self.starts.clear();
        // The original's case, where it has the same characters.
        let mut cases = original
            .filter(|original| original.len() == folded.len())
            .map(str::chars);
        let (mut prev, mut prev_lower) = (None, false);
        for (at, c) in folded.chars().enumerate() {
            let lower = cases
                .as_mut()
                .and_then(Iterator::next)
                .map(|o| (o.is_lowercase(), o.is_uppercase()));
            let camel = prev_lower && lower.is_some_and(|(_, upper)| upper);
            if c.is_alphanumeric() && (camel || prev.is_none_or(|p: char| !p.is_alphanumeric())) {
                self.starts.push(at);
            }
            prev = Some(c);
            prev_lower = lower.is_some_and(|(lower, _)| lower);
            self.chars.push(c);
        }
        if self
            .chars
            .split(|&c| c == SEPARATOR)
            .any(|part| part == query.whole)
        {
            return Some(Tier::Exact);
        }
        let last = query.words.len() - 1;
        let mut worst = Tier::Initials;
        for (n, word) in query.words.iter().enumerate() {
            worst = worst.min(self.word(word, n == last && query.last_is_whole)?);
        }
        Some(worst)
    }

    fn word(&self, word: &[char], whole: bool) -> Option<Tier> {
        debug_assert!(!word.is_empty());
        let chars = &self.chars[..];
        if word.len() >= 2 && !whole {
            let initials = self
                .starts
                .windows(word.len())
                .any(|starts| starts.iter().zip(word).all(|(&at, &c)| chars[at] == c));
            if initials {
                return Some(Tier::Initials);
            }
        }
        let mut prefix = false;
        for &at in &self.starts {
            if chars[at..].starts_with(word) {
                let end = at + word.len();
                if !whole || chars.get(end).is_none_or(|c| !c.is_alphanumeric()) {
                    return Some(Tier::WordStart);
                }
                prefix = true;
            }
        }
        if word.len() == 1 {
            return None;
        }
        if prefix || chars.windows(word.len()).any(|window| window == word) {
            return Some(Tier::Inside);
        }
        let mut rest = chars.iter();
        word.iter()
            .all(|c| rest.any(|t| t == c))
            .then_some(Tier::Scattered)
    }
}

/// [`Tiers::text`] for a one-letter query, without its buffers: the text is
/// that letter, or a word starts with it.
fn one_letter(letter: char, folded: Utf32Str, original: Option<&str>) -> Option<Tier> {
    if folded.len() == 1 {
        return (folded.get(0) == letter).then_some(Tier::Exact);
    }
    let mut cases = original
        .filter(|original| original.len() == folded.len())
        .map(str::chars);
    let (mut prev, mut prev_lower) = (None, false);
    for c in folded.chars() {
        let lower = cases
            .as_mut()
            .and_then(Iterator::next)
            .map(|o| (o.is_lowercase(), o.is_uppercase()));
        let camel = prev_lower && lower.is_some_and(|(_, upper)| upper);
        if c == letter && (camel || prev.is_none_or(|p: char| !p.is_alphanumeric())) {
            return Some(Tier::WordStart);
        }
        prev = Some(c);
        prev_lower = lower.is_some_and(|(lower, _)| lower);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(text: &str, query: &str) -> Option<Tier> {
        Tiers::default().text(folded(text).slice(..), Some(text), &Query::new(query))
    }

    #[test]
    fn tiers_from_exact_to_scattered() {
        assert_eq!(tier("Files", "files"), Some(Tier::Exact));
        assert_eq!(tier("Fire Alarm", "fire  alarm"), Some(Tier::Exact));
        assert_eq!(tier("Visual Studio Code", "vsc"), Some(Tier::Initials));
        assert_eq!(tier("Terminal", "term"), Some(Tier::WordStart));
        assert_eq!(tier("WebStorm", "storm"), Some(Tier::WordStart));
        assert_eq!(tier("Determinate", "term"), Some(Tier::Inside));
        assert_eq!(tier("Color Decoder", "code"), Some(Tier::Inside));
        assert_eq!(tier("Color Dev", "code"), Some(Tier::Scattered));
        assert_eq!(tier("Firefox", "alarm"), None);
    }

    #[test]
    fn every_word_must_match_and_the_worst_one_counts() {
        assert_eq!(tier("Fire Alarm", "alarm fire"), Some(Tier::WordStart));
        assert_eq!(tier("Fire Alarm", "fire larm"), Some(Tier::Inside));
        assert_eq!(tier("Firefox", "fire alarm"), None);
    }

    #[test]
    fn one_letter_matches_only_at_a_word_start() {
        assert_eq!(tier("Thunar", "t"), Some(Tier::WordStart));
        assert_eq!(tier("Steam", "t"), None);
    }

    #[test]
    fn a_trailing_space_asks_for_a_whole_word() {
        assert_eq!(tier("Fi Calculator", "fi "), Some(Tier::WordStart));
        assert_eq!(tier("Firefox", "fi "), Some(Tier::Inside));
    }

    #[test]
    fn neither_case_nor_accents_matter() {
        assert_eq!(tier("Firefox", "FI"), Some(Tier::WordStart));
        assert_eq!(tier("Żółw Notes", "zolw"), Some(Tier::WordStart));
        assert_eq!(tier("Zolw Notes", "żółw"), Some(Tier::WordStart));
        assert_eq!(fold_text("ŻÓŁW"), "zolw");
    }
}
