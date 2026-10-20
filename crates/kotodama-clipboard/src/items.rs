//! The Extension's settings, from its `[extensions.clipboard]` config table,
//! and the Items and Views the history shows as.
//!
//! ```toml
//! [extensions.clipboard]
//! # Past this many entries, the oldest go.
//! max_entries = 1000
//! # Entries older than this many days go.
//! max_age_days = 7
//! ```

use std::fmt::Write as _;
use std::path::Path;

use kotodama_protocol::{
    Action, Item, ItemKind, MetadataEntry, NamespacedKind, Table, Value, View, ViewContent,
};

use crate::history::{Content, Entry, History, Limits};

/// The Item that opens the history, by id. Entries' ids are numbers, so it
/// never clashes with one.
pub const HISTORY: &str = "history";
/// The list View of every entry.
pub const LIST_VIEW: &str = "history";
/// The detail View of one entry.
pub const DETAIL_VIEW: &str = "clip";

/// Actions, by id.
pub const PASTE: &str = "paste";
pub const COPY: &str = "copy";
pub const SHOW: &str = "show";
pub const DELETE: &str = "delete";
pub const OPEN: &str = "open";
pub const CLEAR: &str = "clear";

/// How much of a text the detail View shows; pasting gives all of it.
const DETAIL_CHARS: usize = 2000;
const DETAIL_LINES: usize = 30;

const DAY_SECS: f64 = 24.0 * 60.0 * 60.0;

/// Reads the config table. A setting of the wrong shape is logged and left at
/// its default; the others still apply.
pub fn limits(config: &Table) -> Limits {
    let mut limits = Limits::default();
    for (key, value) in config {
        let set = match key.as_str() {
            "max_entries" => match value {
                Value::Integer(n) if *n >= 1 => {
                    limits.max_entries = usize::try_from(*n).unwrap_or(usize::MAX);
                    Ok(())
                }
                _ => Err("should be a whole number above 0"),
            },
            "max_age_days" => match value {
                Value::Integer(days) if *days > 0 => {
                    limits.max_age_secs = (*days as u64).saturating_mul(DAY_SECS as u64);
                    Ok(())
                }
                Value::Float(days) if *days > 0.0 => {
                    limits.max_age_secs = (days * DAY_SECS) as u64;
                    Ok(())
                }
                _ => Err("should be a number of days above 0"),
            },
            _ => Err("isn't a setting"),
        };
        if let Err(err) = set {
            kotodama_extension::config::warn(format_args!("`{key}` {err}"));
        }
    }
    limits
}

/// The Item that opens the whole history and clears it.
pub fn history_item() -> Item {
    Item {
        id: HISTORY.to_owned(),
        kind: kind("history"),
        identity: None,
        title: "Clipboard History".to_owned(),
        subtitle: Some("Paste something copied before".to_owned()),
        icon: Some("edit-paste".to_owned()),
        keyword: None,
        keywords: vec!["paste".to_owned(), "copied".to_owned()],
        hidden: false,
        actions: vec![
            action(OPEN, "Show Clipboard History"),
            action(CLEAR, "Clear Clipboard History"),
        ],
    }
}

/// A history entry as an Item of kind `clipboard.clip`. Its title is what a
/// text says, or an image's size, and an image's icon is its thumbnail.
pub fn clip_item(history: &History, entry: &Entry) -> Item {
    let (title, subtitle, icon) = match &entry.content {
        Content::Text { preview, chars } => (
            preview.clone(),
            format!("Text, {}", count(*chars, "character")),
            "edit-paste".to_owned(),
        ),
        Content::Image {
            mime,
            width,
            height,
        } => (
            format!("Image, {width}×{height}"),
            format!("{} image, {}", image_type(mime), bytes(entry.size)),
            history.thumbnail(entry.id).display().to_string(),
        ),
    };
    Item {
        id: entry.id.to_string(),
        kind: kind("clip"),
        identity: None,
        title,
        subtitle: Some(subtitle),
        icon: Some(icon),
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        actions: vec![
            action(PASTE, "Paste"),
            action(COPY, "Copy to Clipboard"),
            action(SHOW, "Show Details"),
            action(DELETE, "Delete from History"),
        ],
    }
}

/// Every entry, newest first, to pick from.
pub fn list_view(history: &History) -> View {
    View {
        id: LIST_VIEW.to_owned(),
        title: "Clipboard History".to_owned(),
        content: ViewContent::List {
            items: history
                .entries()
                .map(|entry| clip_item(history, entry))
                .collect(),
            // Typing filters it in the Daemon, newest first within a Tier.
            filters_itself: false,
        },
    }
}

/// One entry in full: a text, shortened if long, or an image's thumbnail,
/// with when it was copied and how big it is. `now` is in seconds since the
/// Unix epoch.
pub fn detail_view(history: &History, entry: &Entry, now: u64) -> View {
    let mut metadata = vec![MetadataEntry {
        label: "Copied".to_owned(),
        value: ago(now.saturating_sub(entry.copied)),
    }];
    let markdown = match &entry.content {
        Content::Text { chars, .. } => {
            metadata.push(MetadataEntry {
                label: "Size".to_owned(),
                value: count(*chars, "character"),
            });
            // Only what shows is read, however much was copied.
            let text = history
                .read_start(entry.id, DETAIL_CHARS * 4)
                .unwrap_or_default();
            code_block(&text, entry.size, *chars)
        }
        Content::Image {
            mime,
            width,
            height,
        } => {
            metadata.push(MetadataEntry {
                label: "Size".to_owned(),
                value: format!("{width}×{height}, {}", bytes(entry.size)),
            });
            metadata.push(MetadataEntry {
                label: "Type".to_owned(),
                value: mime.clone(),
            });
            format!(
                "![Image, {width}×{height}]({})",
                file_url(&history.thumbnail(entry.id))
            )
        }
    };
    let title = match entry.content {
        Content::Text { .. } => "Copied Text",
        Content::Image { .. } => "Copied Image",
    };
    View {
        id: DETAIL_VIEW.to_owned(),
        title: title.to_owned(),
        content: ViewContent::Detail { markdown, metadata },
    }
}

fn kind(local: &str) -> ItemKind {
    ItemKind::Namespaced(
        NamespacedKind::new(format!("clipboard.{local}")).expect("a valid namespaced kind"),
    )
}

fn action(id: &str, title: &str) -> Action {
    Action {
        id: id.to_owned(),
        title: title.to_owned(),
        shortcut: None,
        arguments: Vec::new(),
    }
}

/// The start of a text, `size` bytes and `chars` characters in all, in a
/// fenced code block, so Markdown in it shows as typed: the fence is longer
/// than any run of backticks inside. Long texts are cut.
fn code_block(text: &str, size: u64, chars: u64) -> String {
    let mut shown: String = text
        .split_inclusive('\n')
        .take(DETAIL_LINES)
        .collect::<String>()
        .chars()
        .take(DETAIL_CHARS)
        .collect();
    let cut = (shown.len() as u64) < size;
    if shown.ends_with('\n') {
        shown.pop();
    }
    let mut longest = 0;
    let mut run = 0;
    for c in shown.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    let mut markdown = format!("{fence}\n{shown}\n{fence}\n");
    if cut {
        let _ = write!(
            markdown,
            "\nShortened here; pasting gives all {}.",
            count(chars, "character")
        );
    }
    markdown
}

/// A `file://` URL for `path`, with anything but plain path characters
/// percent-encoded so Markdown reads it whole.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            url.push(byte as char);
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

fn image_type(mime: &str) -> String {
    mime.strip_prefix("image/")
        .unwrap_or(mime)
        .to_ascii_uppercase()
}

/// `1 character`, `1,234 characters`.
fn count(n: u64, what: &str) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if n == 1 {
        format!("{grouped} {what}")
    } else {
        format!("{grouped} {what}s")
    }
}

fn bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    match n as f64 {
        size if size < KB => count(n, "byte"),
        size if size < KB * KB => format!("{:.0} KB", size / KB),
        size => format!("{:.1} MB", size / (KB * KB)),
    }
}

/// How long ago, in words.
fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} ago", count(secs / 60, "minute")),
        3600..86400 => format!("{} ago", count(secs / 3600, "hour")),
        _ => format!("{} ago", count(secs / 86400, "day")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_block_fence_outlasts_the_backticks_inside() {
        assert_eq!(code_block("a ``` b", 7, 7), "````\na ``` b\n````\n");
        assert_eq!(code_block("plain\n", 6, 6), "```\nplain\n```\n");
    }

    #[test]
    fn a_long_text_is_cut_with_a_note() {
        let text = "line\n".repeat(100);
        let block = code_block(&text, 500, 500);
        assert_eq!(block.matches("line").count(), DETAIL_LINES);
        assert!(
            block.ends_with("pasting gives all 500 characters."),
            "{block}"
        );
    }

    #[test]
    fn a_file_url_escapes_what_markdown_would_split_on() {
        assert_eq!(
            file_url(Path::new("/home/a b/thumbs/1.png")),
            "file:///home/a%20b/thumbs/1.png"
        );
    }

    #[test]
    fn numbers_read_as_words() {
        assert_eq!(count(1, "character"), "1 character");
        assert_eq!(count(1234567, "character"), "1,234,567 characters");
        assert_eq!(bytes(2048), "2 KB");
        assert_eq!(ago(30), "just now");
        assert_eq!(ago(3 * 3600 + 5), "3 hours ago");
    }
}
