//! The history on disk, through the library: what a copy keeps, what expires,
//! and what deleting leaves behind. Times are passed in, so expiry runs on a
//! made-up clock.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use kotodama_clipboard::copied::Choice;
use kotodama_clipboard::copied::Copied;
use kotodama_clipboard::copied::choose;
use kotodama_clipboard::history::Content;
use kotodama_clipboard::history::History;
use kotodama_clipboard::history::Limits;

const DAY: u64 = 24 * 60 * 60;
/// 2026-10-19, as seconds since the Unix epoch.
const NOW: u64 = 1_792_368_000;

fn text(text: &str) -> Copied {
    Copied::read("text/plain;charset=utf-8", text.as_bytes().to_vec()).unwrap()
}

/// What the history holds, newest first, texts by their preview and images
/// by their size.
fn shown(history: &History) -> Vec<String> {
    history
        .entries()
        .map(|entry| match &entry.content {
            Content::Text { preview, .. } => preview.clone(),
            Content::Image { width, height, .. } => format!("{width}x{height}"),
        })
        .collect()
}

/// A PNG of `width` by `height` pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// Every file under `dir`.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path.display().to_string());
        }
    }
    found
}

#[test]
fn copied_text_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let first = history.record(text("first"), NOW, &Limits::default()).unwrap().id;
    history
        .record(text("second\nline"), NOW + 1, &Limits::default())
        .unwrap();
    drop(history);

    let history = History::open(dir.path());
    assert_eq!(shown(&history), ["second line", "first"]);
    assert_eq!(history.read(first).unwrap(), b"first");
}

#[test]
fn copying_the_same_again_moves_it_to_the_top() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let hello = history.record(text("hello"), NOW, &Limits::default()).unwrap().id;
    history.record(text("world"), NOW + 1, &Limits::default()).unwrap();

    let again = history.record(text("hello"), NOW + 2, &Limits::default()).unwrap();
    assert_eq!(again.id, hello);
    assert!(again.removed.is_empty());
    assert_eq!(shown(&history), ["hello", "world"]);
    assert_eq!(history.entries().next().unwrap().copied, NOW + 2);
}

#[test]
fn past_the_most_entries_the_oldest_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let limits = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    let oldest = history.record(text("one"), NOW, &limits).unwrap().id;
    history.record(text("two"), NOW + 1, &limits).unwrap();

    let recorded = history.record(text("three"), NOW + 2, &limits).unwrap();
    assert_eq!(recorded.removed, [oldest]);
    assert_eq!(shown(&history), ["three", "two"]);
    assert!(history.read(oldest).is_err());
}

#[test]
fn entries_expire_once_older_than_the_most_age() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let limits = Limits::default();
    let old = history.record(text("old"), NOW, &limits).unwrap().id;
    history.record(text("new"), NOW + DAY, &limits).unwrap();

    assert!(history.expire(NOW + 7 * DAY, &limits).is_empty());
    assert_eq!(history.expire(NOW + 7 * DAY + 1, &limits), [old]);
    assert_eq!(shown(&history), ["new"]);
    assert!(history.read(old).is_err());
    // The next expiry is when the remaining entry turns seven days old.
    assert_eq!(history.next_expiry(&limits), Some(NOW + 8 * DAY + 1));

    let history = History::open(dir.path());
    assert_eq!(shown(&history), ["new"]);
}

#[test]
fn a_shorter_age_from_the_config_expires_more() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    history.record(text("old"), NOW, &Limits::default()).unwrap();
    let limits = Limits {
        max_age_secs: DAY,
        ..Limits::default()
    };
    assert_eq!(history.expire(NOW + DAY + 1, &limits).len(), 1);
}

#[test]
fn deleting_an_entry_deletes_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let keep = history.record(text("keep"), NOW, &Limits::default()).unwrap().id;
    let gone = history.record(text("gone"), NOW + 1, &Limits::default()).unwrap().id;

    assert!(history.delete(gone));
    assert!(!history.delete(gone));
    assert_eq!(shown(&history), ["keep"]);
    assert!(history.read(gone).is_err());
    assert_eq!(history.read(keep).unwrap(), b"keep");
    assert_eq!(shown(&History::open(dir.path())), ["keep"]);
}

#[test]
fn clearing_leaves_nothing_on_disk_but_an_empty_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    history.record(text("secret-ish"), NOW, &Limits::default()).unwrap();
    history
        .record(
            Copied::read("image/png", png(40, 30)).unwrap(),
            NOW + 1,
            &Limits::default(),
        )
        .unwrap();

    let removed = history.clear();
    assert_eq!(removed.len(), 2);
    assert_eq!(shown(&history), Vec::<String>::new());
    let left = files(dir.path());
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].ends_with("index.json"), "{left:?}");
    assert!(!fs::read_to_string(&left[0]).unwrap().contains("secret-ish"));
}

#[test]
fn an_image_is_kept_with_a_small_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let data = png(1200, 600);
    let id = history
        .record(
            Copied::read("image/png", data.clone()).unwrap(),
            NOW,
            &Limits::default(),
        )
        .unwrap()
        .id;

    let entry = history.entries().next().unwrap();
    match &entry.content {
        Content::Image { mime, width, height } => {
            assert_eq!((mime.as_str(), *width, *height), ("image/png", 1200, 600));
        }
        other @ Content::Text { .. } => panic!("expected an image, got {other:?}"),
    }
    assert_eq!(history.read(id).unwrap(), data);
    let thumbnail = image::open(history.thumbnail(id)).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (256, 128));
}

#[test]
fn a_text_preview_is_one_short_line() {
    let long = format!("  fn main() {{\n\t{}\n}}", "x".repeat(500));
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let id = history.record(text(&long), NOW, &Limits::default()).unwrap().id;

    let entry = history.entries().next().unwrap();
    let Content::Text { preview, chars } = &entry.content else {
        panic!("expected text");
    };
    assert!(preview.starts_with("fn main() { xxx"), "{preview}");
    assert_eq!(preview.chars().count(), 100);
    assert_eq!(*chars, long.chars().count() as u64);
    // The whole text is still there to paste.
    assert_eq!(history.read(id).unwrap(), long.as_bytes());
}

#[test]
fn blank_text_and_broken_images_are_not_kept() {
    assert!(Copied::read("text/plain", b" \n\t".to_vec()).is_err());
    assert!(Copied::read("image/png", b"not a png".to_vec()).is_err());
}

#[test]
fn a_password_managers_offer_is_sensitive() {
    let offer = |mimes: &[&str]| choose(mimes.iter().copied());
    assert_eq!(
        offer(&["text/plain;charset=utf-8", "x-kde-passwordManagerHint", "TEXT"]),
        Choice::Sensitive
    );
    assert_eq!(
        offer(&["TEXT", "text/plain", "text/plain;charset=utf-8"]),
        Choice::Read("text/plain;charset=utf-8".into())
    );
    assert_eq!(offer(&["text/html", "image/png"]), Choice::Read("image/png".into()));
    // Text wins over an image, as most clipboard tools read it.
    assert_eq!(offer(&["image/png", "UTF8_STRING"]), Choice::Read("UTF8_STRING".into()));
    assert_eq!(offer(&["text/html", "application/x-other"]), Choice::Nothing);
}

#[test]
fn a_broken_index_starts_an_empty_history_and_drops_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    history.record(text("hello"), NOW, &Limits::default()).unwrap();
    drop(history);
    fs::write(dir.path().join("index.json"), "{not json").unwrap();

    let history = History::open(dir.path());
    assert_eq!(shown(&history), Vec::<String>::new());
    // Data no index names is never left behind.
    assert!(files(dir.path()).iter().all(|file| !file.contains("/data/")));
}

#[test]
fn reading_the_start_of_a_text_never_splits_a_character() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::open(dir.path());
    let id = history.record(text("aéé"), NOW, &Limits::default()).unwrap().id;
    // "a" is one byte and each "é" two, so four bytes end inside the second.
    assert_eq!(history.read_start(id, 4).unwrap(), "aé");
    assert_eq!(history.read_start(id, 100).unwrap(), "aéé");
}
