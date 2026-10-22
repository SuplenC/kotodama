//! The Extension's settings, from its `[extensions.clipboard]` config table,
//! and the Items and Views the history shows as.
//!
//! ```toml
//! [extensions.clipboard]
//! # Past this many entries, the oldest go.
//! max_entries = 1000
//! # Entries older than this many days go.
//! max_age_days = 7
//! # Copied text any of these regular expressions (`regex-lite` syntax)
//! # matches, anywhere in it, is never kept. They replace the defaults, one-time codes and secret
//! # tokens (`ignore::DEFAULT_IGNORE`); `[]` keeps everything.
//! ignore = ['^secret:']
//! ```
//!
//! # Owns
//!
//! - Reading the config table into [`Settings`].
//! - The ids of the Items, Views and Actions, and what each Item and View shows.
//!
//! # Not here
//!
//! - Running the Actions and sending the Items: `main.rs`.
//! - The entries themselves: `history.rs`.

use std::fmt::Write as _;
use std::path::Path;

use kotodama_extension::Chain;
use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::MetadataEntry;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::Table;
use kotodama_protocol::Value;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use crate::history::Content;
use crate::history::Entry;
use crate::history::History;
use crate::history::Limits;
use crate::ignore::Ignore;
use crate::pause::Pause;

/// The Item that opens the history, by id. Entries' ids are numbers, so it
/// never clashes with one.
pub const HISTORY: &str = "history";
/// The list View of every entry.
pub const LIST_VIEW: &str = "history";
/// The detail View of one entry.
pub const DETAIL_VIEW: &str = "clip";

/// A clip's Action that pastes it.
pub const PASTE: &str = "paste";
/// A clip's Action that puts it on the clipboard.
pub const COPY: &str = "copy";
/// A clip's Action that opens its detail View.
pub const SHOW: &str = "show";
/// A clip's Action that deletes it from the history.
pub const DELETE: &str = "delete";
/// The history Item's Action that opens the list View.
pub const OPEN: &str = "open";
/// The history Item's Action that clears the history.
pub const CLEAR: &str = "clear";

/// The Item that pauses the history and resumes it, by id.
pub const PAUSE: &str = "pause";
/// The pause Item's Action that keeps nothing copied for 5 minutes.
pub const PAUSE_5_MINUTES: &str = "pause-5-minutes";
/// The pause Item's Action that keeps nothing copied for an hour.
pub const PAUSE_HOUR: &str = "pause-hour";
/// The pause Item's Action that keeps nothing copied until resumed.
pub const PAUSE_UNTIL_RESUMED: &str = "pause-until-resumed";
/// The pause Item's Action, while paused, that keeps copies again.
pub const RESUME: &str = "resume";

/// How many characters of a text the detail View shows at most; pasting
/// gives all of it.
const DETAIL_CHARS: usize = 2000;
/// How many lines of a text the detail View shows at most.
const DETAIL_LINES: usize = 30;

const DAY_SECS: f64 = 24.0 * 60.0 * 60.0;

/// The settings of the `[extensions.clipboard]` table.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub limits: Limits,
    /// What copied text is never kept.
    pub ignore: Ignore,
}

/// Reads the config table. A setting of the wrong shape is logged and left at
/// its default, and a pattern of `ignore` that isn't a regular expression is
/// logged and left out; the others still apply.
pub fn settings(config: &Table) -> Settings {
    let mut settings = Settings::default();
    let limits = &mut settings.limits;
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
            "ignore" => match kotodama_extension::config::strings(value) {
                Some(patterns) => {
                    let (ignore, errors) = Ignore::new(patterns.iter().map(String::as_str));
                    for err in errors {
                        kotodama_extension::config::warn(format_args!("`ignore`: {}", Chain(&err)));
                    }
                    settings.ignore = ignore;
                    Ok(())
                }
                None => Err("should be a list of regular expressions"),
            },
            "ignore_apps" => Err(
                "isn't a setting: Wayland's data-control protocols never say which app copied, so the history can't \
                 leave out what one app copies",
            ),
            _ => Err("isn't a setting"),
        };
        if let Err(err) = set {
            kotodama_extension::config::warn(format_args!("`{key}` {err}"));
        }
    }
    settings
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
        app_only: false,
        actions: vec![
            Action::new(OPEN, "Show Clipboard History"),
            // Nothing brings the history back.
            Action::new(CLEAR, "Clear Clipboard History").confirm(),
        ],
    }
}

/// The Item that pauses the history, or resumes it while `pause` holds at
/// `now`.
pub fn pause_item(pause: Pause, now: u64) -> Item {
    let (title, subtitle, actions) = if pause.keeps(now) {
        (
            "Pause Clipboard History",
            "Keep nothing copied for a while, as for a password",
            vec![
                Action::new(PAUSE_5_MINUTES, "Pause for 5 Minutes"),
                Action::new(PAUSE_HOUR, "Pause for an Hour"),
                Action::new(PAUSE_UNTIL_RESUMED, "Pause Until Resumed"),
            ],
        )
    } else {
        (
            "Resume Clipboard History",
            "Paused: nothing copied is kept",
            vec![Action::new(RESUME, "Resume Clipboard History")],
        )
    };
    Item {
        id: PAUSE.to_owned(),
        kind: kind("pause"),
        identity: None,
        title: title.to_owned(),
        subtitle: Some(subtitle.to_owned()),
        icon: Some(
            if pause.keeps(now) {
                "media-playback-pause"
            } else {
                "media-playback-start"
            }
            .to_owned(),
        ),
        keyword: None,
        keywords: ["pause", "resume", "private", "password"].map(str::to_owned).to_vec(),
        hidden: false,
        app_only: false,
        actions,
    }
}

/// The pause the pause Item's `action` asks for at `now`, or `None` for
/// another Action.
#[must_use]
pub fn paused_by(action: &str, now: u64) -> Option<Pause> {
    match action {
        PAUSE_5_MINUTES => Some(Pause::Until(now + 5 * 60)),
        PAUSE_HOUR => Some(Pause::Until(now + 60 * 60)),
        PAUSE_UNTIL_RESUMED => Some(Pause::UntilResumed),
        RESUME => Some(Pause::Off),
        _ => None,
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
        Content::Image { mime, width, height } => (
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
        app_only: false,
        actions: vec![
            Action::new(PASTE, "Paste"),
            Action::new(COPY, "Copy to Clipboard"),
            Action::new(SHOW, "Show Details"),
            Action::new(DELETE, "Delete from History"),
        ],
    }
}

/// Every entry, newest first, to pick from.
pub fn list_view(history: &History) -> View {
    View {
        id: LIST_VIEW.to_owned(),
        title: "Clipboard History".to_owned(),
        content: ViewContent::List {
            items: history.entries().map(|entry| clip_item(history, entry)).collect(),
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
            let text = history.read_start(entry.id, DETAIL_CHARS * 4).unwrap_or_default();
            code_block(&text, entry.size, *chars)
        }
        Content::Image { mime, width, height } => {
            metadata.push(MetadataEntry {
                label: "Size".to_owned(),
                value: format!("{width}×{height}, {}", bytes(entry.size)),
            });
            metadata.push(MetadataEntry {
                label: "Type".to_owned(),
                value: mime.clone(),
            });
            format!("![Image, {width}×{height}]({})", file_url(&history.thumbnail(entry.id)))
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
    ItemKind::Namespaced(NamespacedKind::new(format!("clipboard.{local}")).expect("a valid namespaced kind"))
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
    mime.strip_prefix("image/").unwrap_or(mime).to_ascii_uppercase()
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

/// `512 bytes`, `2 KB`, `1.5 MB`.
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
mod tests;
