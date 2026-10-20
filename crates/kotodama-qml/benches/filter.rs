//! A list View of 1000 Items, as a full clipboard history is, and the cost on
//! the Qt thread of showing a filter reply: taking the Daemon's matches,
//! diffing the rows against those shown, and changing them. The model's
//! signals cost nothing here, since the bench has no Qt; each reply sends as
//! few as it can, and only the rows on screen cost a view anything.
//!
//! The cdylib can't be linked, so the bench compiles the model's Qt-free
//! modules itself; whatever it doesn't use is dead code to it.
#![allow(dead_code, unused_imports)]

#[path = "../src/rows.rs"]
mod rows;
#[path = "../src/views.rs"]
mod views;

use std::hint::black_box;
use std::ops::Range;

use criterion::{Criterion, criterion_group, criterion_main};
use kotodama_protocol::{Action, Item, ItemKind, ListMatch, View, ViewContent};

use rows::{Model, Rows};
use views::{Contributed, Open, Views};

const ITEMS: u32 = 1000;

fn item(n: u32) -> Item {
    Item {
        id: format!("entry-{n}"),
        kind: ItemKind::Url {
            url: format!("https://example.com/{n}"),
        },
        identity: None,
        title: format!("Entry number {n} with some words"),
        subtitle: Some(format!("https://example.com/{n}")),
        icon: Some("text-x-generic".into()),
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        actions: vec![
            Action::new("paste", "Paste"),
            Action::new("copy", "Copy").shortcut("ctrl+c"),
        ],
    }
}

fn found(items: impl Iterator<Item = u32>, highlights: &[u32]) -> Vec<ListMatch> {
    items
        .map(|item| ListMatch {
            item,
            highlights: highlights.to_vec(),
        })
        .collect()
}

/// The model's rows, with signals no view hears.
struct Shown(Rows);

impl Model for Shown {
    fn rows(&mut self) -> &mut Rows {
        &mut self.0
    }
    fn begin_remove(&mut self, rows: Range<usize>) {
        black_box(rows);
    }
    fn end_remove(&mut self) {}
    fn begin_insert(&mut self, rows: Range<usize>) {
        black_box(rows);
    }
    fn end_insert(&mut self) {}
    fn begin_move(&mut self, from: usize, before: usize) {
        black_box((from, before));
    }
    fn end_move(&mut self) {}
    fn changed(&mut self, rows: Range<usize>) {
        black_box(rows);
    }
}

/// What `KotodamaSearch` does with a filter reply, less Qt.
fn reply(views: &mut Views, rows: &mut Shown, text: &str, matches: &[ListMatch]) {
    views.set_filter(text);
    views.filtered("clipboard", "history", text, matches.to_vec());
    let listed = views.top().and_then(Open::listed).unwrap();
    rows::diff(rows, &listed);
}

fn bench(c: &mut Criterion) {
    let view = View {
        id: "history".into(),
        title: "History".into(),
        content: ViewContent::List {
            items: (0..ITEMS).map(item).collect(),
            filters_itself: false,
        },
    };
    let all = found(0..ITEMS, &[0]);
    // A second character typed: half the Items still match, each with one
    // more character highlighted.
    let half = found((0..ITEMS).step_by(2), &[0, 1]);
    // The same Items ranked a little differently.
    let mut reranked = found(0..ITEMS, &[0, 1]);
    reranked.swap(0, 500);
    reranked.swap(10, 20);
    reranked[100..110].reverse();

    let start = || {
        let mut views = Views::default();
        views.push("clipboard".into(), view.clone(), Contributed::new());
        let mut rows = Shown(Rows::default());
        reply(&mut views, &mut rows, "a", &all);
        (views, rows)
    };
    let mut group = c.benchmark_group("filter_reply_1000");
    // Two replies each: one narrows to 500 rows, the next widens back to all.
    group.bench_function("narrow_then_widen", |b| {
        let (mut views, mut rows) = start();
        b.iter(|| {
            reply(&mut views, &mut rows, "ab", &half);
            reply(&mut views, &mut rows, "a", &all);
        });
    });
    // Two replies each, of all 1000 rows: reranked, then back.
    group.bench_function("reorder", |b| {
        let (mut views, mut rows) = start();
        b.iter(|| {
            reply(&mut views, &mut rows, "ab", &reranked);
            reply(&mut views, &mut rows, "a", &all);
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
