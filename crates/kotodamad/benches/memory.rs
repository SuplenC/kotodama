//! The heap the Daemon keeps for its Indexed items: the 10k made-up apps of
//! `corpus`, and every emoji and symbol of the emoji Extension, counted by a
//! global allocator that tracks the bytes in use. Unlike resident memory, it
//! is the same on every machine. Items' payloads are kept in a file, as
//! `kotodamad` keeps them, so they don't count.
//!
//! The lines use the bencher format with `bytes` as their unit, so CI compares
//! them with the baseline like the Daemon's resident memory (see
//! `scripts/bench-compare`).

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use kotodama_protocol::{FromExtension, Item, Manifest, Table};
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, Launcher, Output};
use kotodamad::discovery::Found;
use kotodamad::history::{History, Store};
use kotodamad::usage::Entry;

mod corpus;

/// The system's allocator, counting the bytes in use.
struct Counting;

static IN_USE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call goes to the system's allocator, unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        IN_USE.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller promised.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller promised.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        IN_USE.fetch_add(new_size, Ordering::Relaxed);
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: as the caller promised.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct Nothing;

impl Launcher for Nothing {
    fn start(&mut self, _: ExtensionId, _: u64, _: &Found) {}
    fn stop(&mut self, _: ExtensionId, _: u64) {}
}

impl Desktop for Nothing {
    fn output(&mut self, _: Output) {}
    fn launch(&mut self, _: Vec<String>) {}
    fn notify(&mut self, _: String, _: String) {}
}

impl Store for Nothing {
    fn save(&mut self, _: &History) {}
    fn log(&mut self, _: Entry) {}
    fn forget_usage(&mut self, _: Option<&str>) {}
}

/// The bytes the Daemon keeps for `items`, sent by an Extension named `name`.
fn kept(name: &str, items: impl FnOnce() -> Vec<Item>) -> (usize, usize) {
    let manifest = Manifest::from_toml(&format!("name = \"{name}\"\ncommand = [\"x\"]")).unwrap();
    let found = Found::new(manifest, PathBuf::from("/")).unwrap();
    let now = Instant::now();
    let mut daemon = Daemon::new(
        vec![found],
        Table::new(),
        History::default(),
        Nothing,
        Nothing,
        Nothing,
        now,
    );
    // As `kotodamad` runs: what a search doesn't read waits in a file.
    let payloads = tempfile::tempdir().unwrap();
    daemon.keep_items_in(payloads.path());
    let (sender, _extension) = mpsc::sync_channel(64);
    daemon.handle(
        Event::ExtensionUp {
            extension: ExtensionId(0),
            generation: 1,
            sender,
        },
        now,
    );
    let before = IN_USE.load(Ordering::Relaxed);
    let items = items();
    let count = items.len();
    daemon.handle(
        Event::FromExtension {
            extension: ExtensionId(0),
            generation: 1,
            message: FromExtension::AddItems { items },
        },
        now,
    );
    let after = IN_USE.load(Ordering::Relaxed);
    drop(daemon);
    (after.saturating_sub(before), count)
}

fn main() {
    // `cargo bench` passes `--bench`; a filter that names neither skips them.
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    type Corpus = fn() -> Vec<Item>;
    let corpora: [(&str, &str, Corpus); 2] = [
        ("index/10k_items/heap", "apps", corpus::items),
        ("index/emoji/heap", "emoji", kotodama_emoji::items),
    ];
    for (bench, name, items) in corpora {
        if filter
            .as_deref()
            .is_some_and(|filter| !bench.contains(filter))
        {
            continue;
        }
        let (bytes, count) = kept(name, items);
        println!(
            "{bench}: {count} Items, {:.1} MiB, {} bytes per Item",
            bytes as f64 / (1 << 20) as f64,
            bytes / count
        );
        println!("test {bench} ... bench: {bytes} bytes (+/- 0)");
    }
}
