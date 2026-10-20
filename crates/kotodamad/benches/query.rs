//! From a query arriving to its Results encoded for the Front end, with 10k
//! Indexed items: everything the Daemon does per keystroke except the socket
//! write. Also: titles 200 characters long, a keystroke right behind a 1,000
//! Item batch, the first keystroke after the menu opens on a cold CPU cache,
//! and rows whose Items were read from the payload file. Then the same for a
//! list View of 10k Items the Daemon filters.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use kotodamad::history::History;

mod corpus;
mod setup;

use setup::{APPS, CLIPBOARD, Setup};

/// Typing on: each keystroke narrows the last one's matches.
fn next_keystroke(setup: &mut Setup, word: &str) -> impl FnMut() -> usize {
    let mut len = 0;
    move || {
        len = len % word.len() + 1;
        setup.query(&word[..len])
    }
}

fn bench(c: &mut Criterion) {
    let mut setup = Setup::new();
    let mut group = c.benchmark_group("query_10k_items");
    // The first keystroke matches the most Items and can't narrow.
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.query(if alternate { "f" } else { "t" }))
        })
    });
    // Two words pasted into a cleared box: the Empty query forgets the last
    // search, so this one searches every Item. (Named `two_words` while the
    // Daemon still narrowed it from the last iteration's.)
    group.bench_function("two_words_pasted", |b| {
        b.iter(|| {
            setup.query("");
            black_box(setup.query("fire tool"))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let mut keystroke = next_keystroke(&mut setup, "firefoxterminal");
        b.iter(|| black_box(keystroke()))
    });
    // A keystroke right behind a batch of 1,000 Items, as when the apps are
    // scanned again or the clipboard sends its history: from the batch
    // arriving to the keystroke's Results.
    group.bench_function("keystroke_behind_1k_batch", |b| {
        let batches = [1, 2].map(|seed| corpus::long_titles(1000, 60, seed));
        let mut n = 0;
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                setup.settle();
                n += 1;
                let batch = batches[n % 2].clone();
                let start = Instant::now();
                setup.send(CLIPBOARD, batch);
                black_box(setup.query(if n.is_multiple_of(2) { "f" } else { "t" }));
                total += start.elapsed();
            }
            total
        });
        setup.settle();
    });
    group.finish();

    // How long a batch takes to show: from an Extension sending it to its
    // new version being ready on the indexing thread. A batch of new clips,
    // and the apps scanned again.
    let mut group = c.benchmark_group("staleness");
    let clips = corpus::long_titles(1000, 60, 1);
    let apps = corpus::items();
    for (name, extension, batch) in [
        ("1k_clips", CLIPBOARD, &clips),
        ("10k_apps_rescanned", APPS, &apps),
    ] {
        group.bench_function(name, |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let batch = batch.clone();
                    let start = Instant::now();
                    setup.send(extension, batch);
                    let indexed = setup.indexed.recv().unwrap();
                    total += start.elapsed();
                    setup.daemon.handle(indexed, Instant::now());
                    setup.pending -= 1;
                }
                total
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("query_10k_items");
    // Opening the menu: the Results most used at this time of day.
    let mut setup = Setup::picked();
    group.bench_function("empty_query_2k_picked", |b| {
        b.iter(|| black_box(setup.query("")))
    });
    group.finish();

    // The first keystroke after the menu opens, with the CPU cache taken by
    // other programs since the last time: only the keystroke is timed. Each
    // iteration also writes 32 MiB, so it runs fewer.
    let mut group = c.benchmark_group("open_10k_items");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(2));
    // Opening warms the cache with what a keystroke reads; `_unwarmed` takes
    // it again between the toggle and the keystroke, as if the Daemon didn't.
    for (name, warmed) in [
        ("first_keystroke_cold_cache", true),
        ("first_keystroke_cold_cache_unwarmed", false),
    ] {
        group.bench_function(name, |b| {
            let mut evict = vec![0u8; 32 << 20];
            let mut n = 0u8;
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    setup.query("");
                    n = n.wrapping_add(1);
                    let mut evict = || {
                        for page in evict.chunks_mut(64) {
                            page[0] = n;
                        }
                        black_box(&evict);
                    };
                    evict();
                    setup.toggle();
                    if !warmed {
                        evict();
                    }
                    let start = Instant::now();
                    black_box(setup.query(if n.is_multiple_of(2) { "f" } else { "t" }));
                    total += start.elapsed();
                }
                total
            })
        });
    }
    group.finish();

    // A query whose rows' Items were read for the last Results, and one
    // whose rows' Items are each read from the payload file: an app's own
    // keyword, a different app each time, out of the most kept resident.
    let mut group = c.benchmark_group("query_10k_items");
    group.bench_function("rows_resident", |b| {
        b.iter(|| {
            setup.query("");
            black_box(setup.query("app4321"))
        })
    });
    group.bench_function("rows_from_payload_file", |b| {
        let mut n = 0;
        b.iter(|| {
            n = (n + 7919) % corpus::ITEMS;
            setup.query("");
            black_box(setup.query(&format!("app{n}")))
        })
    });
    group.finish();

    // Clipboard entries searched by their whole text: the time per keystroke
    // should not grow with the titles' length.
    let mut setup = Setup::with(History::default(), corpus::long_titles(1000, 200, 7));
    let mut group = c.benchmark_group("query_1k_long_titles");
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.query(if alternate { "f" } else { "t" }))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let mut keystroke = next_keystroke(&mut setup, "firefoxterminal");
        b.iter(|| black_box(keystroke()))
    });
    group.finish();
}

/// Typing into a list View of 10k Items, matched as the search matches, to
/// stay within `query_10k_items`'s budget.
fn filter(c: &mut Criterion) {
    let mut setup = Setup::new();
    setup.push_list();
    let mut group = c.benchmark_group("filter_10k_items");
    group.bench_function("first_keystroke", |b| {
        let mut alternate = false;
        b.iter(|| {
            alternate = !alternate;
            black_box(setup.filter(if alternate { "f" } else { "t" }))
        })
    });
    group.bench_function("next_keystroke", |b| {
        let word = "firefoxterminal";
        let mut len = 1;
        b.iter(|| {
            len = len % word.len() + 1;
            black_box(setup.filter(&word[..len]))
        })
    });
    group.finish();
}

criterion_group!(benches, bench, filter);
criterion_main!(benches);
