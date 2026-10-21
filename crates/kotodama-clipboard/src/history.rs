//! The clipboard history on disk, in `$XDG_STATE_HOME/kotodama/clipboard/`:
//!
//! - `index.json`: every entry, oldest first, with a short preview of each text and the size of
//!   each image;
//! - `data/<id>`: what was copied, byte for byte, read only to paste or show it;
//! - `thumbs/<id>.png`: each image's thumbnail, for the detail View.
//!
//! Only the index is held in memory, so the history costs the Extension a
//! few hundred bytes an entry, whatever was copied. The directory and every
//! file in it are readable only by the user.
//!
//! Times are seconds since the Unix epoch, passed in by the caller, so expiry
//! runs the same on a made-up clock in tests.
//!
//! # Owns
//!
//! - The files of the history, their layout and their permissions.
//! - Noticing the same thing copied again, and expiring entries by [`Limits`].
//!
//! # Not here
//!
//! - Reading the limits from the config: `items.rs`.
//! - Deciding what is kept at all: `copied.rs`.

use std::fmt;
use std::fs;
use std::fs::DirBuilder;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::copied::Copied;

/// How many characters of a text show in its preview, which is its title and
/// all the Daemon searches. A keystroke's time grows with the titles' length:
/// in `benches/search.rs`, the next keystroke takes about 100 us at 60
/// characters, 135 us at 100 and 310 us at 200, against a budget of 2 ms.
pub const PREVIEW_CHARS: usize = 100;

const INDEX: &str = "index.json";
const DATA: &str = "data";
const THUMBS: &str = "thumbs";

/// How many entries the history keeps unless configured. Searching this
/// many stays well within the keystroke budget (`benches/search.rs`).
pub const DEFAULT_MAX_ENTRIES: usize = 1000;

/// How long an entry is kept unless configured, in seconds: a week.
pub const DEFAULT_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

/// How much history to keep, from the config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Past this many entries, the oldest go.
    pub max_entries: usize,
    /// Entries older than this go.
    pub max_age_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_age_secs: DEFAULT_MAX_AGE_SECS,
        }
    }
}

/// One thing copied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique in this history, and never reused.
    pub id: u64,
    /// When it was last copied, in seconds since the Unix epoch.
    pub copied: u64,
    /// The data's length in bytes.
    pub size: u64,
    /// A hash of the data, to notice the same thing copied again.
    hash: u64,
    pub content: Content,
}

/// What an entry holds, as much of it as the index keeps. Its `Debug` leaves
/// out a text's preview, which is what was copied.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// Text, whole in the entry's data file.
    Text {
        /// The first [`PREVIEW_CHARS`] characters, with each run of
        /// whitespace, newlines included, made one space.
        preview: String,
        /// How many characters the whole text has.
        chars: u64,
    },
    /// An image, with its thumbnail beside the data file.
    Image { mime: String, width: u32, height: u32 },
}

impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Content::Text { chars, .. } => f
                .debug_struct("Text")
                .field("preview", &format_args!("<redacted>"))
                .field("chars", chars)
                .finish(),
            Content::Image { mime, width, height } => f
                .debug_struct("Image")
                .field("mime", mime)
                .field("width", width)
                .field("height", height)
                .finish(),
        }
    }
}

/// What [`History::record`] did.
#[derive(Debug, PartialEq, Eq)]
pub struct Recorded {
    /// The entry at the top: a new one, or the same thing copied before.
    pub id: u64,
    /// The entries that expired to make room.
    pub removed: Vec<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    next_id: u64,
    /// Oldest first.
    entries: Vec<Entry>,
}

/// The history kept in one directory: its index in memory, what was copied
/// on disk. Its `Debug` shows the directory and how many entries it holds,
/// not what they are.
pub struct History {
    dir: PathBuf,
    index: Index,
}

impl fmt::Debug for History {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("History")
            .field("dir", &self.dir)
            .field("entries", &self.index.entries.len())
            .finish()
    }
}

impl History {
    /// Loads the history kept in `dir`. A missing index is an empty history;
    /// one that can't be read is logged and starts an empty history too.
    /// Either way, data that no entry names is deleted, so nothing copied
    /// outlives the index.
    pub fn open(dir: &Path) -> Self {
        let index = match fs::read(dir.join(INDEX)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                kotodama_extension::log!("{}: {err}; starting afresh", dir.join(INDEX).display());
                Index::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Index::default(),
            Err(err) => {
                kotodama_extension::log!("{}: {err}; starting afresh", dir.join(INDEX).display());
                Index::default()
            }
        };
        let history = History {
            dir: dir.to_owned(),
            index,
        };
        history.remove_strays();
        history
    }

    /// Every entry, newest first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.index.entries.iter().rev()
    }

    /// The entry `id`, if the history holds it.
    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.index.entries.iter().find(|entry| entry.id == id)
    }

    pub fn len(&self) -> usize {
        self.index.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.entries.is_empty()
    }

    /// Keeps what was copied at `now`, then expires what `limits` don't
    /// allow. The same thing copied again keeps its entry, and its id, and
    /// moves it to the top.
    ///
    /// # Errors
    ///
    /// The error writing a new entry's data or thumbnail, which leaves the
    /// index as it was. A failure to save the index is only logged.
    pub fn record(&mut self, copied: Copied, now: u64, limits: &Limits) -> io::Result<Recorded> {
        let data = copied.data();
        let hash = fnv1a(data);
        let size = data.len() as u64;
        let same = self.index.entries.iter().position(|entry| {
            entry.hash == hash
                && entry.size == size
                && same_kind(&entry.content, &copied)
                && self.read(entry.id).is_ok_and(|kept| kept == data)
        });
        let id = if let Some(at) = same {
            self.move_to_top(at, now)
        } else {
            self.add(copied, hash, size, now)?
        };
        let removed = self.drop_expired(now, limits);
        // What was copied is kept on disk and in memory either way, so the
        // Extension still shows it; a stale index only loses it on restart.
        if let Err(err) = self.save() {
            kotodama_extension::log!("couldn't save the history index: {err}");
        }
        Ok(Recorded { id, removed })
    }

    /// Removes the entries `limits` don't allow at `now`: those older
    /// than the most age, then the oldest past the most entries. Returns
    /// their ids.
    pub fn expire(&mut self, now: u64, limits: &Limits) -> Vec<u64> {
        let removed = self.drop_expired(now, limits);
        if !removed.is_empty() {
            self.save_or_warn();
        }
        removed
    }

    /// When the next entry turns too old, if any will.
    pub fn next_expiry(&self, limits: &Limits) -> Option<u64> {
        self.index
            .entries
            .iter()
            .map(|entry| entry.copied.saturating_add(limits.max_age_secs).saturating_add(1))
            .min()
    }

    /// Deletes one entry and its data. False if there was none.
    pub fn delete(&mut self, id: u64) -> bool {
        let Some(at) = self.index.entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        self.index.entries.remove(at);
        self.remove_files(id);
        self.save_or_warn();
        true
    }

    /// Deletes every entry and its data. Returns their ids.
    pub fn clear(&mut self) -> Vec<u64> {
        let removed: Vec<u64> = self.index.entries.drain(..).map(|entry| entry.id).collect();
        self.save_or_warn();
        // Whatever the index named, nothing copied stays on disk.
        let _ = fs::remove_dir_all(self.dir.join(DATA));
        let _ = fs::remove_dir_all(self.dir.join(THUMBS));
        removed
    }

    /// What was copied, byte for byte.
    ///
    /// # Errors
    ///
    /// The error reading the entry's data file, such as `NotFound` for an
    /// entry that is gone.
    pub fn read(&self, id: u64) -> io::Result<Vec<u8>> {
        fs::read(self.data_path(id))
    }

    /// At most the first `max` bytes of a text entry, cut at a character.
    ///
    /// # Errors
    ///
    /// The error opening or reading the entry's data file.
    pub fn read_start(&self, id: u64, max: usize) -> io::Result<String> {
        let mut data = Vec::with_capacity(max);
        File::open(self.data_path(id))?
            .take(max as u64)
            .read_to_end(&mut data)?;
        Ok(match String::from_utf8(data) {
            Ok(text) => text,
            // A character the cut split in two is left out.
            Err(err) if err.utf8_error().error_len().is_none() => {
                let valid = err.utf8_error().valid_up_to();
                let mut data = err.into_bytes();
                data.truncate(valid);
                String::from_utf8(data).unwrap_or_default()
            }
            Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
        })
    }

    /// Where an image entry's thumbnail is, a PNG.
    pub fn thumbnail(&self, id: u64) -> PathBuf {
        self.dir.join(THUMBS).join(format!("{id}.png"))
    }

    /// Moves the entry at `at` to the top, copied again at `now`, and
    /// returns its id.
    fn move_to_top(&mut self, at: usize, now: u64) -> u64 {
        let mut entry = self.index.entries.remove(at);
        entry.copied = now;
        let id = entry.id;
        self.index.entries.push(entry);
        id
    }

    /// Writes a new entry's files, then adds it at the top. A failed write
    /// leaves the index as it was.
    fn add(&mut self, copied: Copied, hash: u64, size: u64, now: u64) -> io::Result<u64> {
        let id = self.index.next_id;
        self.make_dirs()?;
        write_private(&self.data_path(id), copied.data())?;
        let content = match copied {
            Copied::Text(text) => Content::Text {
                preview: preview(&text),
                chars: text.chars().count() as u64,
            },
            Copied::Image {
                mime,
                width,
                height,
                thumbnail,
                ..
            } => {
                if let Err(err) = write_private(&self.thumbnail(id), &thumbnail) {
                    let _ = fs::remove_file(self.data_path(id));
                    return Err(err);
                }
                Content::Image { mime, width, height }
            }
        };
        self.index.next_id += 1;
        self.index.entries.push(Entry {
            id,
            copied: now,
            size,
            hash,
            content,
        });
        Ok(id)
    }

    fn data_path(&self, id: u64) -> PathBuf {
        self.dir.join(DATA).join(id.to_string())
    }

    fn drop_expired(&mut self, now: u64, limits: &Limits) -> Vec<u64> {
        let mut removed = Vec::new();
        self.index.entries.retain(|entry| {
            let keep = now.saturating_sub(entry.copied) <= limits.max_age_secs;
            if !keep {
                removed.push(entry.id);
            }
            keep
        });
        let over = self.index.entries.len().saturating_sub(limits.max_entries);
        removed.extend(self.index.entries.drain(..over).map(|entry| entry.id));
        for &id in &removed {
            self.remove_files(id);
        }
        removed
    }

    fn remove_files(&self, id: u64) {
        let _ = fs::remove_file(self.data_path(id));
        let _ = fs::remove_file(self.thumbnail(id));
    }

    /// Deletes data and thumbnails no entry names.
    fn remove_strays(&self) {
        for sub in [DATA, THUMBS] {
            let Ok(files) = fs::read_dir(self.dir.join(sub)) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name();
                let id = name
                    .to_str()
                    .map(|name| name.trim_end_matches(".png"))
                    .and_then(|id| id.parse::<u64>().ok());
                if id.is_none_or(|id| self.get(id).is_none()) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
    }

    fn make_dirs(&self) -> io::Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(self.dir.join(DATA))?;
        builder.create(self.dir.join(THUMBS))
    }

    /// Writes the index whole, then renames it into place, so a crash leaves
    /// the old one or the new one.
    fn save(&self) -> io::Result<()> {
        DirBuilder::new().recursive(true).mode(0o700).create(&self.dir)?;
        let json = serde_json::to_vec(&self.index).map_err(io::Error::other)?;
        let partial = self.dir.join(format!("{INDEX}.partial"));
        write_private(&partial, &json)?;
        fs::rename(&partial, self.dir.join(INDEX))
    }

    fn save_or_warn(&self) {
        if let Err(err) = self.save() {
            kotodama_extension::log!("couldn't save {}: {err}", self.dir.join(INDEX).display());
        }
    }
}

/// Writes a file only the user can read.
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut file: File = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(data)
}

fn same_kind(content: &Content, copied: &Copied) -> bool {
    matches!(
        (content, copied),
        (Content::Text { .. }, Copied::Text(_)) | (Content::Image { .. }, Copied::Image { .. })
    )
}

/// The first [`PREVIEW_CHARS`] characters of `text`, trimmed, with each run of
/// whitespace made one space.
fn preview(text: &str) -> String {
    let mut preview = String::new();
    let mut count = 0;
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = !preview.is_empty();
            continue;
        }
        if count == PREVIEW_CHARS || (space && count + 1 == PREVIEW_CHARS) {
            break;
        }
        if space {
            preview.push(' ');
            count += 1;
            space = false;
        }
        preview.push(c);
        count += 1;
    }
    preview
}

/// 64-bit FNV-1a: stable across builds, unlike std's hasher, since it is
/// kept on disk.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
