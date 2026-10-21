//! The instructions the Daemon runs per keystroke with 10k Indexed items, from
//! a query arriving to its Results encoded for the Front end, on the setup
//! `query.rs` times them on (`setup/`). gungraun counts them under Valgrind's
//! Callgrind, so the count is the same from run to run on one machine, and a
//! few percent more shows, where the wall-clock benchmarks can't tell it from
//! noise. Another CPU can count a little differently: glibc picks its string
//! functions by CPU feature.
//!
//! Needs Valgrind and the gungraun-runner of the `gungraun` version pinned in
//! Cargo.toml (both in the dev shell): `cargo bench -p kotodamad --bench instructions`.
//! Building the Daemon and its Index is setup, and dropping them teardown:
//! neither is counted.

use std::hint::black_box;

use gungraun::{library_benchmark, library_benchmark_group, main};
use kotodama_protocol::{Table, Value};

mod corpus;
mod setup;

use setup::Setup;

/// The search with `before` typed, and the query the next keystroke makes.
fn typed_then(before: &str, query: &'static str) -> (Setup, &'static str) {
    let mut setup = Setup::new();
    setup.query(before);
    (setup, query)
}

/// Drops the Daemon outside of what is counted.
fn drop_setup(setup: Setup) {
    drop(setup);
}

#[library_benchmark(setup = typed_then, teardown = drop_setup)]
// The first keystroke matches the most Items and can't narrow.
#[bench::first_keystroke("", "f")]
// Typing on: the keystroke narrows the last one's matches.
#[bench::next_keystroke("firefo", "firefox")]
// Two words pasted into a cleared box: no last search to narrow.
#[bench::two_words_pasted("", "fire tool")]
fn keystroke((mut setup, query): (Setup, &'static str)) -> Setup {
    black_box(setup.query(black_box(query)));
    setup
}

/// The menu opening on 2,000 Items picked, with the Results most used then
/// ranked ahead as the hot Items were planned, and shown once already: what
/// the empty query costs from then on.
fn picked_then_empty() -> Setup {
    let mut setup = Setup::picked();
    setup.query("");
    setup
}

#[library_benchmark(setup = picked_then_empty, teardown = drop_setup)]
#[bench::empty_query_2k_picked()]
fn empty_query(mut setup: Setup) -> Setup {
    black_box(setup.query(black_box("")));
    setup
}

/// The emoji Extension's Items beside the corpus, behind the Prefix
/// `prefix` (`""` for none), and the query the next keystroke makes.
fn emoji_then(prefix: &str, query: &'static str) -> (Setup, &'static str) {
    let emoji = Table::from([("prefix".to_owned(), Value::String(prefix.into()))]);
    let extensions = Table::from([("emoji".to_owned(), Value::Table(emoji))]);
    let config = Table::from([("extensions".to_owned(), Value::Table(extensions))]);
    (Setup::with_emoji(config), query)
}

#[library_benchmark(setup = emoji_then, teardown = drop_setup)]
// The emoji in the root search, as `prefix = ""` puts them back.
#[bench::emoji_in_root("", "f")]
// The emoji behind their Prefix: the root search skips them.
#[bench::emoji_scoped_out("emoji", "f")]
// Typed behind the Prefix: only the emoji are searched.
#[bench::emoji_scope_searched("emoji", "emoji f")]
fn emoji_keystroke((mut setup, query): (Setup, &'static str)) -> Setup {
    black_box(setup.query(black_box(query)));
    setup
}

library_benchmark_group!(
    name = query_10k_items,
    benchmarks = [keystroke, empty_query, emoji_keystroke]
);

main!(library_benchmark_groups = query_10k_items);
