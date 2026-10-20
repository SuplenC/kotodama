//! How well an Item's text matches a query, in tiers that the Daemon ranks by
//! before anything else: an exact title beats a word that starts with the
//! query, which beats the query inside a word, which beats its letters
//! scattered. nucleo decides what matches at all and breaks ties within a
//! tier. Below every real match comes a near miss: the query with typos in
//! it, at most one per [`LETTERS_PER_TYPO`] letters of each word.
//!
//! Text is folded before it's compared: lowercase, with accents dropped
//! (`Żółw` is `zolw`), so neither case nor accents in the query matter.

use kotodama_protocol::Item;
use nucleo_matcher::pattern::Pattern;
use nucleo_matcher::{Matcher, Utf32Str, Utf32String, chars};

/// How well a text matches, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// No match, but the query would match with as many typos as it may have:
    /// a letter missing, extra or wrong, or two letters swapped.
    NearMiss,
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
            Tier::NearMiss => 0,
            Tier::Scattered => 1,
            Tier::Inside => 2,
            Tier::WordStart | Tier::Initials => 3,
            Tier::Exact => 4,
        }
    }
}

/// The coarse tier one above `coarse`: where a Result the user picks often
/// ranks. A near miss stays one, so every real match ranks above it.
pub fn lift(coarse: u8) -> u8 {
    if coarse == Tier::NearMiss.coarse() {
        return coarse;
    }
    (coarse + 1).min(Tier::Exact.coarse())
}

/// How many letters of a query's word allow it one typo. A shorter word
/// allows none, so a short query doesn't nearly match everything.
pub const LETTERS_PER_TYPO: usize = 4;

/// How many typos a word of `len` letters may have.
fn typos(len: usize) -> usize {
    len / LETTERS_PER_TYPO
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
    /// The letters of each word, as [`letters`] gives them.
    letters: Vec<u64>,
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
            query.letters.push(letters(word.iter().copied()));
            query.words.push(word);
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether any word is long enough to have a typo.
    pub fn tolerates_typos(&self) -> bool {
        self.typo_initials() != 0
    }

    /// The first letters of the words long enough to have a typo, as
    /// [`Haystack::initials`] gives them: a near miss has a word starting
    /// with one of them.
    pub fn typo_initials(&self) -> u64 {
        self.words
            .iter()
            .filter(|word| typos(word.len()) > 0)
            .fold(0, |set, word| set | initial(word[0]))
    }

    /// Whether whatever nearly matches `longer`, a query that adds to this
    /// one, matches or nearly matches this one. So it does when each word
    /// that `longer` lengthens allows as many typos, for a word's start is no
    /// nearer a longer word. Not when this query asked for a whole word,
    /// which `longer` may not.
    pub fn narrows_near_misses_of(&self, longer: &Query) -> bool {
        !self.last_is_whole
            && longer.words.len() >= self.words.len()
            && self
                .words
                .iter()
                .zip(&longer.words)
                .all(|(word, longer)| typos(word.len()) == typos(longer.len()))
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
    /// Every letter of the title and keywords, as [`letters`] gives them, so
    /// looking for near misses passes over most Items without searching them.
    letters: u64,
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

/// Every character of `chars`, as [`initial`] gives them. The bit every other
/// character shares is always set, as in [`initials`].
fn letters(chars: impl Iterator<Item = char>) -> u64 {
    chars.map(initial).fold(1 << 63, |set, c| set | c)
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
            letters: letters(title.chars().chain(words.chars())),
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

    /// Whether the title and keywords have enough of each word's letters for
    /// the Item to be a near miss: each letter they lack takes a typo, and a
    /// word that could have one must start a word of theirs, as one with a
    /// typo does.
    fn may_nearly_match(&self, query: &Query) -> bool {
        let letters = query
            .words
            .iter()
            .zip(&query.letters)
            .all(|(word, &letters)| {
                (letters & !self.letters).count_ones() as usize <= typos(word.len())
            });
        letters && self.initials & query.typo_initials() != 0
    }

    /// The letters words of the title and keywords start with: one of 36 bits
    /// for each ASCII letter or digit, and one shared by every other
    /// character, which is always set.
    pub fn initials(&self) -> u64 {
        self.initials
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
    /// The table [`within`] works in.
    distances: Vec<usize>,
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

    /// Whether an Item that doesn't match is a near miss: each word of the
    /// query matches its title or keywords, or a word of them starts with
    /// the query's word but for as many typos as that may have. Subtitles
    /// aren't looked at.
    pub fn near_miss(&mut self, haystack: &Haystack, item: &Item, query: &Query) -> bool {
        if !query.tolerates_typos() || !haystack.may_nearly_match(query) {
            return false;
        }
        self.nearly(haystack.title.slice(..), Some(&item.title), query)
            || (!haystack.words.is_empty() && self.nearly(haystack.words.slice(..), None, query))
    }

    /// Whether each word of the query matches one folded text, or a word of
    /// it starts with the query's word but for as many typos as that may
    /// have. `original` gives the case, as for [`Tiers::text`].
    fn nearly(&mut self, folded: Utf32Str, original: Option<&str>, query: &Query) -> bool {
        // Asked only of what doesn't match, so a one-word query's word
        // doesn't, and only the words starting with its first letter count.
        if let [word] = query.words.as_slice() {
            return self.typo_in(folded, original, word, query.last_is_whole);
        }
        self.load(folded, original);
        let last = query.words.len() - 1;
        query.words.iter().enumerate().all(|(n, word)| {
            let whole = n == last && query.last_is_whole;
            self.word(word, whole).is_some() || self.typo(word, whole)
        })
    }

    /// [`Tiers::typo`] of a folded text. An ASCII text is read without
    /// [`Tiers::load`]: only the words starting with `word`'s first letter are
    /// copied out.
    fn typo_in(
        &mut self,
        folded: Utf32Str,
        original: Option<&str>,
        word: &[char],
        whole: bool,
    ) -> bool {
        let Utf32Str::Ascii(bytes) = folded else {
            self.load(folded, original);
            return self.typo(word, whole);
        };
        let typos = typos(word.len());
        // Most texts: look only where the first letter is.
        let Ok(first) = u8::try_from(word[0]) else {
            return false;
        };
        if typos == 0 {
            return false;
        }
        // As `load` reads them: the original's case counts only where it has
        // as many bytes as the folded text has characters, so it is ASCII.
        let cases = original
            .filter(|original| original.len() == folded.len())
            .map(str::as_bytes);
        for at in (0..bytes.len()).filter(|&at| bytes[at] == first) {
            let starts = at == 0
                || !bytes[at - 1].is_ascii_alphanumeric()
                || cases.is_some_and(|cases| {
                    cases[at].is_ascii_uppercase() && cases[at - 1].is_ascii_lowercase()
                });
            if !starts {
                continue;
            }
            self.chars.clear();
            self.chars.extend(
                bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric())
                    .take(word.len() + typos + 1)
                    .map(|&b| char::from(b)),
            );
            if within(word, &self.chars, typos, whole, &mut self.distances) {
                return true;
            }
        }
        false
    }

    /// Whether a word of the text, or with `whole` false the start of one, is
    /// `word` but for as many typos as it may have. The first letter is never
    /// one: people rarely mistype it, and holding to it keeps short queries
    /// from nearly matching most words.
    fn typo(&mut self, word: &[char], whole: bool) -> bool {
        let typos = typos(word.len());
        if typos == 0 {
            return false;
        }
        for &at in &self.starts {
            if self.chars[at] != word[0] {
                continue;
            }
            let len = self.chars[at..]
                .iter()
                .position(|c| !c.is_alphanumeric())
                .unwrap_or(self.chars.len() - at);
            if within(
                word,
                &self.chars[at..at + len],
                typos,
                whole,
                &mut self.distances,
            ) {
                return true;
            }
        }
        false
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
        self.load(folded, original);
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

    /// Fills `chars` with a folded text and `starts` with where its words
    /// start.
    fn load(&mut self, folded: Utf32Str, original: Option<&str>) {
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

/// Whether `text`, or with `whole` false a start of it, is `word` but for at
/// most `typos` letters missing, extra or wrong, or swapped with the next one
/// (the optimal string alignment distance). `distances` holds the table, a
/// row per letter of `word`.
fn within(
    word: &[char],
    text: &[char],
    typos: usize,
    whole: bool,
    distances: &mut Vec<usize>,
) -> bool {
    // A longer start has more letters than typos to spare, and a shorter text
    // too few.
    let n = text.len().min(word.len() + typos);
    if (whole && text.len() > n) || text.len() + typos < word.len() {
        return false;
    }
    // Only the cells at most `typos` off the diagonal can be within reach; the
    // rest hold `beyond`, as any distance past `typos` does.
    let beyond = typos + 1;
    let width = n + 1;
    let d = distances;
    d.clear();
    d.extend((0..width).map(|j| j.min(beyond)));
    for i in 1..=word.len() {
        let row = d.len();
        d.resize(row + width, beyond);
        d[row] = i.min(beyond);
        let mut best = d[row];
        for j in i.saturating_sub(typos).max(1)..=(i + typos).min(n) {
            let above = row - width + j;
            let mut cost = (d[above - 1] + usize::from(word[i - 1] != text[j - 1]))
                .min(d[above] + 1)
                .min(d[row + j - 1] + 1);
            if i > 1 && j > 1 && word[i - 1] == text[j - 2] && word[i - 2] == text[j - 1] {
                cost = cost.min(d[above - width - 2] + 1);
            }
            d[row + j] = cost.min(beyond);
            best = best.min(cost);
        }
        // No later row does better than this one's best.
        if best > typos {
            return false;
        }
    }
    let last = &d[word.len() * width..];
    if whole {
        last[n] <= typos
    } else {
        last.iter().any(|&cost| cost <= typos)
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
