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
use nucleo_matcher::pattern::{AtomKind, Pattern};
use std::ops::{BitOr, BitOrAssign};

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
    /// The letters of each word.
    letters: Vec<Letters>,
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
            query.letters.push(Letters::in_text(word.iter().copied()));
            query.words.push(word);
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether any word is long enough to have a typo.
    pub fn tolerates_typos(&self) -> bool {
        !self.typo_initials().is_empty()
    }

    /// The first letters of the words long enough to have a typo, as
    /// [`Haystack::initials`] gives them: a near miss has a word starting
    /// with one of them.
    pub fn typo_initials(&self) -> Letters {
        self.words
            .iter()
            .filter(|word| typos(word.len()) > 0)
            .fold(Letters::NONE, |set, word| set | Letters::of(word[0]))
    }

    /// Whether whatever nearly matches `longer`, a query that adds to this
    /// one, matches or nearly matches this one. So it does when each word
    /// that `longer` lengthens allows as many typos, for a word's start is no
    /// nearer a longer word. Not when this query asked for a whole word,
    /// which `longer` may not, nor when a word of one letter, which only
    /// matches at a word's start, grows to one that matches anywhere.
    pub fn narrows_near_misses_of(&self, longer: &Query) -> bool {
        !self.last_is_whole
            && longer.words.len() >= self.words.len()
            && self.words.iter().zip(&longer.words).all(|(word, longer)| {
                typos(word.len()) == typos(longer.len()) && (word.len() > 1 || longer.len() == 1)
            })
    }

    /// Whether the query is long enough for subtitles to be searched.
    pub fn searches_subtitles(&self) -> bool {
        self.words.iter().map(Vec::len).sum::<usize>() >= SUBTITLE_LETTERS
    }

    /// What lets [`Haystack::score`] pass over most Items without searching
    /// them.
    pub fn filter(&self) -> Filter {
        let letter = match self.words.as_slice() {
            [word] if word.len() == 1 && !self.last_is_whole => Some(Letters::of(word[0])),
            _ => None,
        };
        let subtitles = self.searches_subtitles().then(|| {
            self.words
                .iter()
                .map(|word| Letters::of(word[0]))
                .fold(Letters::NONE, BitOr::bitor)
        });
        let letters = self
            .letters
            .iter()
            .copied()
            .fold(Letters::NONE, BitOr::bitor);
        Filter {
            letter,
            subtitles,
            letters,
        }
    }
}

/// Letters an Item's texts must have to be worth searching.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    /// A one-letter query's letter, which only matches at a word start, so the
    /// title or a keyword must have a word starting with it.
    letter: Option<Letters>,
    /// The first letters of the query's words, which a subtitle's words must
    /// start with for it to match, if it's long enough for subtitles to be
    /// searched.
    subtitles: Option<Letters>,
    /// Every letter of the query: an Item's texts need them all to match.
    letters: Letters,
}

impl Filter {
    /// Whether the query's letter, if it is one, starts a word of the title
    /// or keywords alone, so it matches fewer Items than a longer query may.
    pub fn by_letter(&self) -> bool {
        self.letter.is_some()
    }

    /// Whether an Item with `letters` in its texts, as
    /// [`Haystack::every_letter`] gives them, may match.
    #[inline]
    pub fn may_match(&self, letters: Letters) -> bool {
        letters.contains(self.letters)
    }
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
pub const SEPARATOR: char = '\u{1f}';

/// One folded text as it is searched, wherever it is kept: its characters,
/// as nucleo holds them, and where its words start.
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    pub chars: Utf32Str<'a>,
    /// Where each word starts, as [`word_starts`] finds them, in order.
    pub starts: &'a [u32],
}

impl Text<'_> {
    pub const EMPTY: Text<'static> = Text {
        chars: Utf32Str::Ascii(&[]),
        starts: &[],
    };

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

/// What an Item is searched by, wherever it is kept: an owned [`Haystack`]
/// or a row of the Index. Everything a keystroke needs, worked out once when
/// the Item arrives.
#[derive(Clone, Copy, Debug)]
pub struct Texts<'a> {
    pub title: Text<'a>,
    /// The keyword and keywords, joined by [`SEPARATOR`]; empty if there are
    /// none.
    pub words: Text<'a>,
    /// Empty if there is none. Searched so seldom that where its words start
    /// isn't kept.
    pub subtitle: Utf32Str<'a>,
    /// The letters words of the title and keywords start with, so a
    /// one-letter query passes over most Items without searching them.
    pub initials: Letters,
    /// The same for the subtitle.
    pub subtitle_initials: Letters,
    /// Every letter of the title and keywords, so looking for near misses
    /// passes over most Items without searching them.
    pub letters: Letters,
}

/// An Item's texts folded as [`fold`] does, with all that [`Texts`] holds:
/// what the Index copies into its arena, and [`Haystack`] keeps on its own.
pub struct Folded {
    pub title: Utf32String,
    pub words: Utf32String,
    pub subtitle: Utf32String,
    pub title_starts: Vec<u32>,
    pub words_starts: Vec<u32>,
    pub initials: Letters,
    pub subtitle_initials: Letters,
    pub letters: Letters,
    /// Every letter of the title, keywords and subtitle.
    pub every_letter: Letters,
}

impl Folded {
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
        let subtitle_initials = initials(&subtitle, None);
        let initials = initials(&title, Some(&item.title)) | initials(&words, None);
        let letters = Letters::in_text(title.chars().chain(words.chars()));
        let (title, words, subtitle) = (
            Utf32String::from(title),
            Utf32String::from(words),
            Utf32String::from(subtitle),
        );
        let every_letter = letters | Letters::in_text(subtitle.slice(..).chars());
        let mut title_starts = Vec::new();
        word_starts(title.slice(..), Some(&item.title), &mut title_starts);
        let mut words_starts = Vec::new();
        word_starts(words.slice(..), None, &mut words_starts);
        Folded {
            title,
            words,
            subtitle,
            title_starts,
            words_starts,
            initials,
            subtitle_initials,
            letters,
            every_letter,
        }
    }
}

/// What an Item is searched by, on its own rather than in the Index: the
/// Daemon's own Items and Live query results.
pub struct Haystack {
    pub title: Utf32String,
    words: Utf32String,
    subtitle: Utf32String,
    /// Where the title's words start, then the keywords'.
    starts: Vec<u32>,
    /// How many of `starts` are the title's.
    title_starts: usize,
    initials: Letters,
    subtitle_initials: Letters,
    letters: Letters,
    every_letter: Letters,
}

/// A set of folded characters, as bits of one word: one for each ASCII letter
/// or digit, and one shared by every other character. The sets of an Item's
/// texts always hold the shared one, so they never wrongly rule a text out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Letters(u64);

// As small as the word it wraps, so the Index's arrays of them stay dense.
const _: () = assert!(size_of::<Letters>() == size_of::<u64>());

impl Letters {
    pub const NONE: Letters = Letters(0);

    pub fn bits(self) -> u64 {
        self.0
    }
    /// The one every character but an ASCII letter or digit stands for.
    const OTHER: Letters = Letters(1 << 63);

    /// The set of one character.
    #[inline]
    pub fn of(c: char) -> Letters {
        Letters(match c {
            'a'..='z' => 1 << (c as u32 - 'a' as u32),
            '0'..='9' => 1 << (26 + c as u32 - '0' as u32),
            _ => Letters::OTHER.0,
        })
    }

    /// Every character of a text, and [`Letters::OTHER`].
    fn in_text(chars: impl Iterator<Item = char>) -> Letters {
        chars.map(Letters::of).fold(Letters::OTHER, BitOr::bitor)
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every one of `other` is in this set.
    #[inline]
    pub fn contains(self, other: Letters) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether this set and `other` share one.
    #[inline]
    pub fn meets(self, other: Letters) -> bool {
        self.0 & other.0 != 0
    }

    /// How many of `other` this set lacks.
    #[inline]
    pub fn lacks(self, other: Letters) -> usize {
        (other.0 & !self.0).count_ones() as usize
    }
}

impl BitOr for Letters {
    type Output = Letters;

    #[inline]
    fn bitor(self, other: Letters) -> Letters {
        Letters(self.0 | other.0)
    }
}

impl BitOrAssign for Letters {
    #[inline]
    fn bitor_assign(&mut self, other: Letters) {
        self.0 |= other.0;
    }
}

/// The letters words of `folded` start with, with words starting as
/// [`word_starts`] has them, and [`Letters::OTHER`].
fn initials(folded: &str, original: Option<&str>) -> Letters {
    let mut set = Letters::OTHER;
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
            set |= Letters::of(c);
        }
        prev = Some(c);
        prev_lower = lower.is_some_and(|(lower, _)| lower);
    }
    set
}

/// Appends to `starts` where each word of a folded text starts: at a letter
/// or digit after anything else, and, when `original` gives the case, at a
/// capital after a small letter, as in `WebStorm`. The case counts only where
/// `original` has a byte for each character, so it is ASCII.
pub fn word_starts(folded: Utf32Str, original: Option<&str>, starts: &mut Vec<u32>) {
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
            starts.push(at as u32);
        }
        prev = Some(c);
        prev_lower = lower.is_some_and(|(lower, _)| lower);
    }
}

/// `text` as a [`Haystack`] holds it.
pub fn folded(text: &str) -> Utf32String {
    Utf32String::from(fold_text(text))
}

impl Haystack {
    pub fn new(item: &Item) -> Self {
        let Folded {
            title,
            words,
            subtitle,
            mut title_starts,
            words_starts,
            initials,
            subtitle_initials,
            letters,
            every_letter,
        } = Folded::new(item);
        let len = title_starts.len();
        title_starts.extend(words_starts);
        Haystack {
            title,
            words,
            subtitle,
            starts: title_starts,
            title_starts: len,
            initials,
            subtitle_initials,
            letters,
            every_letter,
        }
    }

    pub fn texts(&self) -> Texts<'_> {
        let (title, words) = self.starts.split_at(self.title_starts);
        Texts {
            title: Text {
                chars: self.title.slice(..),
                starts: title,
            },
            words: Text {
                chars: self.words.slice(..),
                starts: words,
            },
            subtitle: self.subtitle.slice(..),
            initials: self.initials,
            subtitle_initials: self.subtitle_initials,
            letters: self.letters,
        }
    }

    /// As [`Texts::score`].
    pub fn score(
        &self,
        pattern: &Pattern,
        matcher: &mut Matcher,
        filter: Filter,
    ) -> Option<(u32, Matched)> {
        self.texts().score(pattern, matcher, filter)
    }

    /// The letters words of the title and keywords start with.
    pub fn initials(&self) -> Letters {
        self.initials
    }

    /// Every letter of the title, keywords and subtitle.
    pub fn every_letter(&self) -> Letters {
        self.every_letter
    }
}

/// Past this many characters, a text is scored by nucleo's greedy match, in
/// time that grows with the text, rather than its optimal match, in time that
/// grows with the text times the query: long clipboard entries would
/// otherwise make every keystroke slower (ADR 0016). Both match the same
/// texts; only the score, a late tie-break, may differ.
pub const OPTIMAL_CHARS: usize = 64;

/// `pattern`'s score in `haystack`, as [`Pattern::score`] gives it up to
/// [`OPTIMAL_CHARS`].
fn score(pattern: &Pattern, haystack: Utf32Str<'_>, matcher: &mut Matcher) -> Option<u32> {
    if haystack.len() <= OPTIMAL_CHARS {
        return pattern.score(haystack, matcher);
    }
    let mut score = 0;
    for atom in &pattern.atoms {
        score += u32::from(if atom.kind == AtomKind::Fuzzy && !atom.negative {
            // As nucleo sets them for a query parsed as the Daemon parses
            // it: folded like the haystacks.
            matcher.config.ignore_case = true;
            matcher.config.normalize = false;
            matcher.fuzzy_match_greedy(haystack, atom.needle_text())?
        } else {
            atom.score(haystack, matcher)?
        });
    }
    Some(score)
}

impl Texts<'_> {
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
            && !self.initials.meets(letter)
        {
            return None;
        }
        let title = score(pattern, self.title.chars, matcher);
        let words = if self.words.is_empty() {
            None
        } else {
            score(pattern, self.words.chars, matcher)
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
                .is_some_and(|initials| self.subtitle_initials.contains(initials)) =>
            {
                score(pattern, self.subtitle, matcher).map(|score| (score, Matched::SUBTITLE))
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
            .all(|(word, &letters)| self.letters.lacks(letters) <= typos(word.len()));
        letters && self.initials.meets(query.typo_initials())
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

/// Runs `$body` with `$chars` bound to the characters of a [`Utf32Str`], as
/// bytes or as `char`s, so the body is compiled for each without a branch
/// per character.
macro_rules! with_chars {
    ($text:expr, |$chars:ident| $body:expr) => {
        match $text {
            Utf32Str::Ascii($chars) => $body,
            Utf32Str::Unicode($chars) => $body,
        }
    };
}

/// A character of a folded text, as a byte of an ASCII one or a `char`.
trait Char: Copy {
    fn char(self) -> char;
}

impl Char for u8 {
    #[inline]
    fn char(self) -> char {
        char::from(self)
    }
}

impl Char for char {
    #[inline]
    fn char(self) -> char {
        self
    }
}

/// Whether `chars` from `at` on start with `word`.
#[inline]
fn starts_with<C: Char>(chars: &[C], at: usize, word: &[char]) -> bool {
    chars.len() >= at + word.len() && chars[at..].iter().zip(word).all(|(&c, &w)| c.char() == w)
}

/// Works out tiers, reusing its buffers from one text to the next.
#[derive(Default)]
pub struct Tiers {
    chars: Vec<char>,
    /// Where each word of the subtitle starts, while it is searched.
    starts: Vec<u32>,
    /// The table [`within`] works in.
    distances: Vec<usize>,
}

impl Tiers {
    /// How well an Item matches, as [`Tiers::texts`] works it out. `item`
    /// is the Item `haystack` was made from.
    pub fn item(
        &mut self,
        haystack: &Haystack,
        _item: &Item,
        query: &Query,
        matched: Matched,
    ) -> Option<Tier> {
        self.texts(haystack.texts(), query, matched)
    }

    /// How well an Item matches: the best of its title and its keywords, or
    /// failing those, its subtitle. A subtitle counts only for a query of at
    /// least [`SUBTITLE_LETTERS`] letters, only at word starts, and then only
    /// as [`Tier::Inside`]. Only the texts in `matched` are looked at. `None`
    /// if nothing matches well enough.
    pub fn texts(&mut self, texts: Texts, query: &Query, matched: Matched) -> Option<Tier> {
        let mut best = None;
        if matched.title {
            best = self.text(texts.title, query);
        }
        if matched.words && best < Some(Tier::Exact) && !texts.words.is_empty() {
            best = best.max(self.text(texts.words, query));
        }
        if best.is_none()
            && matched.subtitle
            && query.searches_subtitles()
            && !texts.subtitle.is_empty()
        {
            let mut starts = std::mem::take(&mut self.starts);
            starts.clear();
            word_starts(texts.subtitle, None, &mut starts);
            let subtitle = Text {
                chars: texts.subtitle,
                starts: &starts,
            };
            if self.text(subtitle, query) >= Some(Tier::WordStart) {
                best = Some(Tier::Inside);
            }
            self.starts = starts;
        }
        best
    }

    /// Whether an Item that doesn't match is a near miss, as
    /// [`Tiers::near_miss_texts`] works it out. `item` is the Item `haystack`
    /// was made from.
    pub fn near_miss(&mut self, haystack: &Haystack, _item: &Item, query: &Query) -> bool {
        self.near_miss_texts(haystack.texts(), query)
    }

    /// Whether an Item that doesn't match is a near miss: each word of the
    /// query matches its title or keywords, or a word of them starts with
    /// the query's word but for as many typos as that may have. Subtitles
    /// aren't looked at.
    pub fn near_miss_texts(&mut self, texts: Texts, query: &Query) -> bool {
        if !query.tolerates_typos() || !texts.may_nearly_match(query) {
            return false;
        }
        self.nearly(texts.title, query)
            || (!texts.words.is_empty() && self.nearly(texts.words, query))
    }

    /// Whether each word of the query matches one folded text, or a word of
    /// it starts with the query's word but for as many typos as that may
    /// have.
    fn nearly(&mut self, text: Text, query: &Query) -> bool {
        // Asked only of what doesn't match, so a one-word query's word
        // doesn't.
        if let [word] = query.words.as_slice() {
            return self.typo(text, word, query.last_is_whole);
        }
        let last = query.words.len() - 1;
        query.words.iter().enumerate().all(|(n, word)| {
            let whole = n == last && query.last_is_whole;
            word_tier(text, word, whole).is_some() || self.typo(text, word, whole)
        })
    }

    /// Whether a word of the text, or with `whole` false the start of one, is
    /// `word` but for as many typos as it may have. The first letter is never
    /// one: people rarely mistype it, and holding to it keeps short queries
    /// from nearly matching most words.
    fn typo(&mut self, text: Text, word: &[char], whole: bool) -> bool {
        let typos = typos(word.len());
        typos > 0 && self.nearest(text, word, whole, typos).is_some()
    }

    /// Where the first word of the text that starts with `word`, but for at
    /// most `typos` typos, starts, and how many of its letters `word` stands
    /// for. Only the words starting with `word`'s first letter are copied
    /// out, and of each only as much as could be near it.
    fn nearest(
        &mut self,
        text: Text,
        word: &[char],
        whole: bool,
        typos: usize,
    ) -> Option<(usize, usize)> {
        let Tiers {
            chars, distances, ..
        } = self;
        let most = word.len() + typos + 1;
        with_chars!(text.chars, |all| {
            text.starts.iter().find_map(|&at| {
                let at = at as usize;
                if all[at].char() != word[0] {
                    return None;
                }
                chars.clear();
                chars.extend(
                    all[at..]
                        .iter()
                        .map(|c| c.char())
                        .take_while(|c| c.is_alphanumeric())
                        .take(most),
                );
                within(word, chars, typos, whole, distances).map(|len| (at, len))
            })
        })
    }

    /// Adds to `highlights` where in a folded title each word of the query
    /// starts a word of it, but for as many typos as the word may have: why a
    /// near miss is one.
    pub fn near_miss_highlights(&mut self, title: Text, query: &Query, highlights: &mut Vec<u32>) {
        let last = query.words.len().saturating_sub(1);
        for (n, word) in query.words.iter().enumerate() {
            let whole = n == last && query.last_is_whole;
            if let Some((at, len)) = self.nearest(title, word, whole, typos(word.len())) {
                highlights.extend((at..at + len).map(|at| at as u32));
            }
        }
        highlights.sort_unstable();
        highlights.dedup();
    }

    /// How well one folded text matches: the worst of the query's words, or
    /// `Exact` if the whole text, or one of its parts between
    /// [`SEPARATOR`]s, is the query. A one-letter word only matches at the
    /// start of a word.
    fn text(&mut self, text: Text, query: &Query) -> Option<Tier> {
        if query.is_empty() {
            return None;
        }
        if let [word] = query.words.as_slice()
            && let [letter] = word.as_slice()
            && !query.last_is_whole
        {
            return one_letter(*letter, text);
        }
        if is_exact(text.chars, &query.whole) {
            return Some(Tier::Exact);
        }
        let last = query.words.len() - 1;
        let mut worst = Tier::Initials;
        for (n, word) in query.words.iter().enumerate() {
            worst = worst.min(word_tier(text, word, n == last && query.last_is_whole)?);
        }
        Some(worst)
    }
}

/// Whether a folded text, or one of its parts between [`SEPARATOR`]s, is
/// `whole`.
fn is_exact(chars: Utf32Str, whole: &[char]) -> bool {
    with_chars!(chars, |chars| {
        chars
            .split(|c| c.char() == SEPARATOR)
            .any(|part| part.len() == whole.len() && starts_with(part, 0, whole))
    })
}

/// How well one word of a query matches a folded text.
fn word_tier(text: Text, word: &[char], whole: bool) -> Option<Tier> {
    debug_assert!(!word.is_empty());
    with_chars!(text.chars, |chars| {
        word_in(chars, text.starts, word, whole)
    })
}

fn word_in<C: Char>(chars: &[C], starts: &[u32], word: &[char], whole: bool) -> Option<Tier> {
    if word.len() >= 2 && !whole {
        let initials = starts.windows(word.len()).any(|starts| {
            starts
                .iter()
                .zip(word)
                .all(|(&at, &c)| chars[at as usize].char() == c)
        });
        if initials {
            return Some(Tier::Initials);
        }
    }
    let mut prefix = false;
    for &at in starts {
        let at = at as usize;
        if starts_with(chars, at, word) {
            let end = at + word.len();
            if !whole || chars.get(end).is_none_or(|c| !c.char().is_alphanumeric()) {
                return Some(Tier::WordStart);
            }
            prefix = true;
        }
    }
    if word.len() == 1 {
        return None;
    }
    if prefix
        || (0..(chars.len() + 1).saturating_sub(word.len()))
            .any(|at| chars[at].char() == word[0] && starts_with(chars, at, word))
    {
        return Some(Tier::Inside);
    }
    let mut rest = chars.iter();
    word.iter()
        .all(|&c| rest.any(|t| t.char() == c))
        .then_some(Tier::Scattered)
}

/// How many letters of `text` are `word` but for at most `typos` letters
/// missing, extra or wrong, or swapped with the next one (the optimal string
/// alignment distance): all of them with `whole`, else those of the start
/// nearest `word`, the longest of equally near ones. `None` if none is near
/// enough. `distances` holds the table, a row per letter of `word`.
fn within(
    word: &[char],
    text: &[char],
    typos: usize,
    whole: bool,
    distances: &mut Vec<usize>,
) -> Option<usize> {
    // A longer start has more letters than typos to spare, and a shorter text
    // too few.
    let n = text.len().min(word.len() + typos);
    if (whole && text.len() > n) || text.len() + typos < word.len() {
        return None;
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
            return None;
        }
    }
    let last = &d[word.len() * width..];
    if whole {
        return (last[n] <= typos).then_some(n);
    }
    let (cost, len) = last
        .iter()
        .enumerate()
        .map(|(len, &cost)| (cost, std::cmp::Reverse(len)))
        .min()?;
    (cost <= typos).then_some(len.0)
}

/// [`Tiers::text`] for a one-letter query: the text is that letter, or a
/// word starts with it.
fn one_letter(letter: char, text: Text) -> Option<Tier> {
    if text.chars.len() == 1 {
        return (text.chars.get(0) == letter).then_some(Tier::Exact);
    }
    let starts = with_chars!(text.chars, |chars| {
        if letter.is_alphanumeric() {
            text.starts
                .iter()
                .any(|&at| chars[at as usize].char() == letter)
        } else {
            // Words start only at letters and digits; anything else counts
            // after anything but one.
            (0..chars.len()).any(|at| {
                chars[at].char() == letter && (at == 0 || !chars[at - 1].char().is_alphanumeric())
            })
        }
    });
    starts.then_some(Tier::WordStart)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(text: &str, query: &str) -> Option<Tier> {
        let chars = folded(text);
        let mut starts = Vec::new();
        word_starts(chars.slice(..), Some(text), &mut starts);
        let text = Text {
            chars: chars.slice(..),
            starts: &starts,
        };
        Tiers::default().text(text, &Query::new(query))
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
