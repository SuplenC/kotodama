
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use kotodama_protocol::ItemKind;
use nucleo_matcher::Config;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;

use super::*;
use crate::rank;
use crate::rank::Haystack;
use crate::rank::Query;
use crate::rank::Tier;
use crate::rank::Tiers;

/// xorshift64*: the same Items and queries on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.below(from.len())]
    }
}

const WORDS: &[&str] = &[
    "Firefox",
    "fire",
    "fox",
    "Terminal",
    "term",
    "Mail",
    "mailbox",
    "Files",
    "Café",
    "naïve",
    "Straße",
    "日本語",
    "😀",
    "getUserName",
    "VSCode",
    "code",
    "Settings",
    "set",
    "git",
    "push",
    "a",
    "x",
    "Ölfass",
    "ŁÓDŹ",
];
const SEPARATORS: &[&str] = &[" ", "-", "_", "", ".", " — "];

fn words(random: &mut Random, most: usize) -> String {
    let mut text = String::new();
    for n in 0..=random.below(most) {
        if n > 0 {
            text.push_str(random.pick(SEPARATORS));
        }
        text.push_str(random.pick(WORDS));
    }
    text
}

fn item(random: &mut Random) -> Item {
    let id = format!("item-{}", random.below(40));
    let kind = match random.below(3) {
        0 => ItemKind::Url {
            url: format!("https://example.org/{id}"),
        },
        1 => ItemKind::App {
            desktop_id: format!("{id}.desktop"),
        },
        _ => ItemKind::Project {
            path: format!("/src/{id}").into(),
        },
    };
    Item {
        id,
        kind,
        identity: (random.below(4) == 0).then(|| format!("thing:{}", random.below(5))),
        title: words(random, 3),
        subtitle: (random.below(2) == 0).then(|| words(random, 4)),
        icon: None,
        keyword: (random.below(5) == 0).then(|| random.pick(WORDS).to_lowercase()),
        keywords: (0..random.below(3)).map(|_| words(random, 2)).collect(),
        hidden: random.below(10) == 0,
        app_only: random.below(4) == 0,
        actions: (0..=random.below(2))
            .map(|n| Action {
                id: ["open", "copy"][n].into(),
                title: format!("Action {n}"),
                shortcut: None,
                arguments: vec![],
                confirm: false,
            })
            .collect(),
    }
}

fn query(random: &mut Random) -> String {
    let mut text = String::new();
    for n in 0..=random.below(2) {
        if n > 0 {
            text.push(' ');
        }
        let word: Vec<char> = random.pick(WORDS).chars().collect();
        let mut word: Vec<char> = word[..=random.below(word.len())].to_vec();
        if word.len() > 2 && random.below(3) == 0 {
            // A typo.
            let at = random.below(word.len() - 1);
            word.swap(at, at + 1);
        }
        if random.below(3) == 0 {
            word = word.iter().flat_map(|c| c.to_uppercase()).collect();
        }
        text.extend(word);
    }
    if random.below(5) == 0 {
        text.push(' ');
    }
    text
}

/// What the search finds of each Item: nucleo's score, its tier, and
/// whether it is a near miss, by Segment and id.
type Found = BTreeMap<(usize, String), (Option<(u32, Option<Tier>)>, bool)>;

/// Matched, as on a keystroke: the Index's dense layout.
fn search(index: &Index, reach: Reach, text: &str, matcher: &mut Matcher, tiers: &mut Tiers) -> Found {
    let pattern = Pattern::parse(&rank::fold_text(text), CaseMatching::Ignore, Normalization::Never);
    let query = Query::new(text);
    let mut out = Vec::new();
    index.search(
        &pattern,
        matcher,
        query.filter(),
        reach,
        None,
        |slot, _, score, matched| out.push((slot, score, matched)),
    );
    let mut found = Found::new();
    for (n, segment) in index.segments.iter().enumerate() {
        if !reach.reads(n, &index.searched) {
            continue;
        }
        for row in 0..segment.every_letter().len() as Row {
            let slot = Slot { segment: n as u32, row };
            let Some(texts) = index.texts(slot) else {
                continue;
            };
            let id = index.id(slot).unwrap().to_owned();
            let near = tiers.near_miss_texts(texts, &query);
            found.insert((n, id), (None, near));
        }
    }
    for (slot, score, matched) in out {
        let tier = tiers.texts(index.texts(slot).unwrap(), &query, matched);
        let id = index.id(slot).unwrap().to_owned();
        found.get_mut(&(slot.segment as usize, id)).unwrap().0 = Some((score, tier));
    }
    found
}

/// The Segment an Item of `extension` goes to.
fn segment_of(extension: usize, app_only: bool) -> usize {
    extension * SIDES + usize::from(app_only)
}

/// Matched the naive way: each Item in reach folded afresh, whole.
fn naive(
    items: &BTreeMap<(usize, String), Item>,
    reach: Reach,
    text: &str,
    matcher: &mut Matcher,
    tiers: &mut Tiers,
) -> Found {
    let pattern = Pattern::parse(&rank::fold_text(text), CaseMatching::Ignore, Normalization::Never);
    let query = Query::new(text);
    items
        .iter()
        .filter(|((n, _), _)| reach.reads(*n, &SEARCHED_BY_DEFAULT.repeat(2)))
        .map(|(key, item)| {
            let haystack = Haystack::new(item);
            let scored = haystack
                .score(&pattern, matcher, query.filter())
                .map(|(score, matched)| (score, tiers.item(&haystack, item, &query, matched)));
            let near = tiers.near_miss(&haystack, item, &query);
            (key.clone(), (scored, near))
        })
        .collect()
}

#[test]
fn a_search_of_the_index_finds_what_a_naive_search_of_every_item_finds() {
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut tiers = Tiers::default();
    for seed in 1..=60u64 {
        let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut index = Index::new(2);
        let mut items: BTreeMap<(usize, String), Item> = BTreeMap::new();
        for _ in 0..30 {
            let extension = random.below(2);
            if random.below(4) == 0 {
                let ids: Vec<String> = (0..=random.below(5))
                    .map(|_| format!("item-{}", random.below(40)))
                    .collect();
                for id in &ids {
                    for app_only in [false, true] {
                        items.remove(&(segment_of(extension, app_only), id.clone()));
                    }
                }
                index.edit(ExtensionId(extension), Edit::Remove(ids));
            } else {
                let batch: Vec<Item> = (0..=random.below(8)).map(|_| item(&mut random)).collect();
                for item in &batch {
                    let other = segment_of(extension, !item.app_only);
                    items.remove(&(other, item.id.clone()));
                    let key = (segment_of(extension, item.app_only), item.id.clone());
                    if item.hidden {
                        items.remove(&key);
                    } else {
                        items.insert(key, item.clone());
                    }
                }
                index.edit(ExtensionId(extension), Edit::Add(batch));
            }
            let text = query(&mut random);
            let reach = match random.below(3) {
                0 => Reach::Root,
                n => Reach::App(ExtensionId(n - 1)),
            };
            assert_eq!(
                search(&index, reach, &text, &mut matcher, &mut tiers),
                naive(&items, reach, &text, &mut matcher, &mut tiers),
                "seed {seed}, {reach:?}, query {text:?}"
            );
        }
        let indexed: BTreeSet<(usize, String)> = (0..2 * SIDES)
            .flat_map(|n| {
                let segment = &index.segments[n];
                (0..segment.every_letter().len() as Row).filter_map(move |row| Some((n, segment.id(row)?.to_owned())))
            })
            .collect();
        assert_eq!(indexed, items.keys().cloned().collect(), "seed {seed}");
        for ((n, id), item) in &items {
            let slot = index.find(ExtensionId(n / SIDES), id).unwrap();
            assert_eq!(slot.segment as usize, *n, "seed {seed}");
            assert_eq!(index.item(slot).as_deref(), Some(item), "seed {seed}");
            assert_eq!(index.identity(slot), item.identity.as_deref());
        }
    }
}
