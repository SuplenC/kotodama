//! Completing what is typed into a form's file field: the entries of the
//! directory a partial path names, ranked as the search ranks, or with
//! nothing typed the newest files the field takes from the places downloads
//! and documents land.
//!
//! ```toml
//! [files]
//! recent_dirs = ["~/Downloads", "~/Documents", "~/Desktop"]
//! ```
//!
//! Directory reads stay off the thread that answers keystrokes: the Daemon
//! hands each request to the `completion` thread ([`Completions::start`]),
//! which answers with an [`Event::Completed`]. That thread keeps the last
//! [`CACHED_DIRS`] directories it listed, each checked again with one `stat`
//! of the directory per request, and reads at most [`MAX_LISTED`] entries of
//! one.
//!
//! # Owns
//!
//! - [`Completer`]: the listings, and ranking them for a request.
//! - [`Completions`]: where requests are answered, on the `completion` thread or at once.
//! - [`Settings`]: `[files]`, and the XDG user directories it defaults to.
//!
//! # Not here
//!
//! - Which file fields a Front end may ask about: `daemon/front_ends.rs`.
//! - Which files a field takes: `completion/accept.rs`.
//! - Matching and tiers: `rank.rs`.

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use kotodama_protocol::PathCompletion;
use kotodama_protocol::Table;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::Value;
use nucleo_matcher::Config;
use nucleo_matcher::Matcher;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;
use nucleo_matcher::pattern::Pattern;

use crate::arguments;
use crate::completion::accept::Accept;
use crate::daemon::Event;
use crate::daemon::FrontEndId;
use crate::problem::Report;
use crate::rank;
use crate::rank::Haystack;
use crate::rank::Tiers;

mod accept;

/// The most paths one answer offers: more than a menu shows without
/// scrolling, few enough to send on every keystroke.
pub const MAX_COMPLETIONS: usize = 50;
/// The most files offered with nothing typed: the newest are what is
/// looked for, and the rest are a keystroke away.
pub const MAX_RECENT: usize = 20;
/// The most entries read of one directory. A directory with more, such as a
/// mail spool, is completed from the first ones it lists, so a keystroke in
/// it stays bounded; a person's own folders are far smaller.
pub const MAX_LISTED: usize = 4096;
/// How many directories' listings are kept: the one being typed in, its
/// parents and children as the path grows and shrinks, and the recent
/// directories. Each holds its entries' folded names, about 150 bytes an
/// entry.
pub const CACHED_DIRS: usize = 8;

/// The coarsest modified times a file system keeps: FAT's two seconds.
const COARSEST_STAMP: std::time::Duration = std::time::Duration::from_secs(2);

/// What a file field asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    /// The files it takes, by extension or MIME type; empty takes any.
    pub accept: Vec<String>,
    /// It asks for a folder.
    pub folder: bool,
}

/// One request for completions, and where its answer goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub front_end: FrontEndId,
    pub extension: String,
    pub view: String,
    pub field: String,
    pub text: String,
    pub wanted: Field,
}

impl Ask {
    /// The answer to this request, holding `entries`.
    fn answer(self, entries: Vec<PathCompletion>) -> ToFrontEnd {
        ToFrontEnd::PathCompletions {
            extension: self.extension,
            view: self.view,
            field: self.field,
            text: self.text,
            entries,
        }
    }

    /// Whether `later` asks about the same field, so it makes this request
    /// stale.
    fn replaced_by(&self, later: &Ask) -> bool {
        self.front_end == later.front_end
            && self.extension == later.extension
            && self.view == later.view
            && self.field == later.field
    }
}

/// `[files]`: where the newest files come from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// `recent_dirs`; `None` for the XDG user directories for downloads,
    /// documents and the desktop.
    pub recent_dirs: Option<Vec<PathBuf>>,
}

impl Settings {
    /// `[files]` of the merged config. A setting that can't be read goes in
    /// `report`, and keeps its default.
    pub fn from_config(config: &Table, report: &mut Report) -> Self {
        let mut settings = Settings::default();
        let table = match config.get("files") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                report.add("files", "should be a table");
                return settings;
            }
            None => return settings,
        };
        for (key, value) in table {
            let set = match key.as_str() {
                "recent_dirs" => dirs(value).map(|dirs| settings.recent_dirs = Some(dirs)),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("files.{key}"), err);
            }
        }
        settings
    }
}

/// A list of directories from the config, each absolute or starting with
/// `~/`, which is expanded once the home directory is known.
fn dirs(value: &Value) -> Result<Vec<PathBuf>, &'static str> {
    const WANTED: &str = "should be a list of absolute paths, or ones starting with `~/`";
    let Value::Array(values) = value else {
        return Err(WANTED);
    };
    values
        .iter()
        .map(|value| match value {
            Value::String(dir) if dir.starts_with('/') || dir.starts_with("~/") => Ok(PathBuf::from(dir)),
            _ => Err(WANTED),
        })
        .collect()
}

/// The XDG user directories for downloads, documents and the desktop, as
/// `user-dirs.dirs` in `config_home` names them, each falling back to its
/// usual name in `home` (`~/Downloads`, `~/Documents`, `~/Desktop`). One set to
/// the home directory itself is switched off, as `xdg-user-dirs-update`
/// writes it, and left out.
pub fn user_dirs(config_home: &Path, home: &Path) -> Vec<PathBuf> {
    let named = fs::read_to_string(config_home.join("user-dirs.dirs")).unwrap_or_default();
    [
        ("XDG_DOWNLOAD_DIR", "Downloads"),
        ("XDG_DOCUMENTS_DIR", "Documents"),
        ("XDG_DESKTOP_DIR", "Desktop"),
    ]
    .into_iter()
    .map(|(name, fallback)| user_dir(&named, name, home).unwrap_or_else(|| home.join(fallback)))
    .filter(|dir| dir != home)
    .collect()
}

/// The directory `name` is set to in the shell-like lines of
/// `user-dirs.dirs`: `XDG_DOWNLOAD_DIR="$HOME/Downloads"`, the last line
/// that sets it winning.
fn user_dir(named: &str, name: &str, home: &Path) -> Option<PathBuf> {
    let value = named
        .lines()
        .filter_map(|line| line.trim().strip_prefix(name)?.strip_prefix('='))
        .next_back()?;
    let value = value.trim().trim_matches('"');
    let dir = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None if value.starts_with('/') => PathBuf::from(value),
        None => return None,
    };
    Some(dir)
}

/// The default `recent_dirs`, from the environment.
fn default_recent_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    user_dirs(&config_home, home)
}

/// One entry of a directory.
#[derive(Debug)]
struct Entry {
    name: String,
    folder: bool,
    modified: Option<u64>,
    /// Its name as the matcher reads it.
    haystack: Haystack,
}

/// What a directory held when it was last read.
#[derive(Debug)]
struct Listing {
    dir: PathBuf,
    /// The directory's modified time then: an entry added, removed or
    /// renamed changes it.
    stamp: SystemTime,
    /// Whether `stamp` is old enough to tell every change since: on a file
    /// system that keeps times in whole seconds (two on FAT), an entry added
    /// in the second the directory was read leaves its time as it was.
    settled: bool,
    entries: Vec<Entry>,
}

/// Completes paths, keeping the directories it lists.
#[derive(Debug)]
pub struct Completer {
    /// Where a relative path starts and what `~` stands for.
    home: Option<PathBuf>,
    settings: Settings,
    /// `settings`' directories, or the XDG ones, once worked out.
    recent_dirs: Option<Vec<PathBuf>>,
    /// The least recently used first.
    listings: Vec<Listing>,
    accept: accept::Globs,
    matcher: Matcher,
    tiers: Tiers,
    /// How many directories were read, rather than found unchanged.
    reads: u64,
}

impl Completer {
    #[must_use]
    pub fn new(home: Option<PathBuf>, settings: Settings) -> Self {
        Completer {
            home,
            settings,
            recent_dirs: None,
            listings: Vec::new(),
            accept: accept::Globs::default(),
            matcher: Matcher::new(Config::DEFAULT),
            tiers: Tiers::default(),
            reads: 0,
        }
    }

    /// Takes new `[files]` settings and home directory.
    pub fn configure(&mut self, home: Option<PathBuf>, settings: Settings) {
        if self.home != home || self.settings != settings {
            self.recent_dirs = None;
        }
        self.home = home;
        self.settings = settings;
    }

    /// How many directories were read off the disk, rather than found
    /// unchanged since they last were.
    pub fn reads(&self) -> u64 {
        self.reads
    }

    /// The paths that complete `text` in a field that wants `wanted`.
    pub fn complete(&mut self, wanted: &Field, text: &str) -> Vec<PathCompletion> {
        let accept = Accept::new(&wanted.accept, &mut self.accept);
        if text.trim().is_empty() {
            return self.recent(wanted, &accept);
        }
        let Some((dir, typed)) = self.split(text) else {
            return Vec::new();
        };
        let Some(listing) = self.listing(&dir) else {
            return Vec::new();
        };
        let listing = &self.listings[listing];
        let shown = |entry: &&Entry| {
            (typed.starts_with('.') || !entry.name.starts_with('.'))
                && if wanted.folder {
                    entry.folder
                } else {
                    entry.folder || accept.takes(&entry.name)
                }
        };
        if typed.is_empty() {
            let mut found: Vec<&Entry> = listing.entries.iter().filter(shown).collect();
            found.sort_by_cached_key(|entry| (!entry.folder, entry.name.to_lowercase()));
            return found
                .into_iter()
                .take(MAX_COMPLETIONS)
                .map(|entry| completion(&listing.dir, entry, Vec::new()))
                .collect();
        }
        let pattern = Pattern::parse(&rank::fold_text(typed), CaseMatching::Ignore, Normalization::Never);
        let query = rank::Query::new(typed);
        let filter = query.filter();
        let mut found = Vec::new();
        for entry in listing.entries.iter().filter(shown) {
            if !filter.may_match(entry.haystack.every_letter(), entry.haystack.initials()) {
                continue;
            }
            let Some((score, matched)) = entry.haystack.score(&pattern, &mut self.matcher, filter) else {
                continue;
            };
            if let Some(tier) = self.tiers.texts(entry.haystack.texts(), &query, matched) {
                found.push((tier.coarse(), tier, score, entry));
            }
        }
        // Tiers first, as in the search; then, within a tier, folders before
        // files.
        found.sort_by(|a, b| {
            (b.0, b.1, b.3.folder, b.2)
                .cmp(&(a.0, a.1, a.3.folder, a.2))
                .then_with(|| by_name(&a.3.name, &b.3.name))
        });
        found
            .into_iter()
            .take(MAX_COMPLETIONS)
            .map(|(_, _, _, entry)| {
                let mut highlights = Vec::new();
                if pattern
                    .indices(entry.haystack.title.slice(..), &mut self.matcher, &mut highlights)
                    .is_some()
                {
                    highlights.sort_unstable();
                    highlights.dedup();
                }
                completion(&listing.dir, entry, highlights)
            })
            .collect()
    }

    /// `[files] recent_dirs`, with `~/` expanded, or the XDG user
    /// directories, each once, worked out the first time they are needed.
    fn recent_dirs(&mut self) -> &[PathBuf] {
        let home = self.home.as_deref();
        let settings = &self.settings;
        self.recent_dirs.get_or_insert_with(|| {
            let mut dirs: Vec<PathBuf> = match &settings.recent_dirs {
                Some(dirs) => dirs
                    .iter()
                    .filter_map(|dir| match dir.strip_prefix("~") {
                        Ok(rest) => home.map(|home| home.join(rest)),
                        Err(_) => Some(dir.clone()),
                    })
                    .collect(),
                None => default_recent_dirs(home),
            };
            let mut seen = Vec::new();
            dirs.retain(|dir| {
                let new = !seen.contains(dir);
                seen.push(dir.clone());
                new
            });
            dirs
        })
    }

    /// The newest entries of the kind `wanted` asks for in the recent
    /// directories.
    fn recent(&mut self, wanted: &Field, accept: &Accept) -> Vec<PathCompletion> {
        let dirs = self.recent_dirs().to_vec();
        let mut found: Vec<PathCompletion> = Vec::new();
        // Each directory's entries are taken as it is read: reading the next
        // may drop it from `listings`.
        for dir in &dirs {
            let Some(at) = self.listing(dir) else {
                continue;
            };
            let listing = &self.listings[at];
            found.extend(
                listing
                    .entries
                    .iter()
                    .filter(|entry| {
                        !entry.name.starts_with('.')
                            && entry.modified.is_some()
                            && if wanted.folder {
                                entry.folder
                            } else {
                                !entry.folder && accept.takes(&entry.name)
                            }
                    })
                    .map(|entry| completion(&listing.dir, entry, Vec::new())),
            );
        }
        found.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| by_name(&a.title, &b.title)));
        found.truncate(MAX_RECENT);
        found
    }

    /// The directory a partial path names, and what is typed of the entry
    /// in it: `~/Down` is `Down` in the home directory, `~/Downloads/` all
    /// of `Downloads`. `None` when a relative path has no home to start
    /// from.
    fn split<'t>(&self, text: &'t str) -> Option<(PathBuf, &'t str)> {
        let text = text.trim();
        let (dir, typed) = match text.rfind('/') {
            Some(at) => (&text[..=at], &text[at + 1..]),
            None if text == "~" => ("~/", ""),
            None => ("", text),
        };
        if dir.starts_with('/') {
            return Some((PathBuf::from(dir), typed));
        }
        let home = self.home.as_deref()?;
        let dir = if dir.is_empty() {
            home.to_path_buf()
        } else {
            PathBuf::from(arguments::absolute(dir, home))
        };
        Some((dir, typed))
    }

    /// Where `dir`'s listing is in `listings`, read again if the directory
    /// changed since; `None` if it can't be read.
    fn listing(&mut self, dir: &Path) -> Option<usize> {
        let stamp = fs::metadata(dir).and_then(|meta| meta.modified()).ok();
        let kept = self.listings.iter().position(|listing| listing.dir == dir);
        if let Some(at) = kept {
            let listing = self.listings.remove(at);
            if listing.settled && Some(listing.stamp) == stamp {
                self.listings.push(listing);
                return Some(self.listings.len() - 1);
            }
        }
        let stamp = stamp?;
        let entries = read(dir)?;
        self.reads += 1;
        if self.listings.len() >= CACHED_DIRS {
            self.listings.remove(0);
        }
        let settled = SystemTime::now()
            .duration_since(stamp)
            .is_ok_and(|age| age >= COARSEST_STAMP);
        self.listings.push(Listing {
            dir: dir.to_path_buf(),
            stamp,
            settled,
            entries,
        });
        Some(self.listings.len() - 1)
    }
}

/// Up to [`MAX_LISTED`] entries of `dir`, with their kind and modified time,
/// through symlinks. Entries whose name isn't UTF-8 are left out: no path
/// could name them in a text field.
fn read(dir: &Path) -> Option<Vec<Entry>> {
    let entries = fs::read_dir(dir).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let meta = fs::metadata(entry.path()).or_else(|_| entry.metadata()).ok()?;
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                    .map(|since| since.as_secs());
                Some(Entry {
                    haystack: Haystack::of_title(&name),
                    name,
                    folder: meta.is_dir(),
                    modified,
                })
            })
            .take(MAX_LISTED)
            .collect(),
    )
}

/// Names in the order a person reads them: case aside, then as they are.
fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

fn completion(dir: &Path, entry: &Entry, highlights: Vec<u32>) -> PathCompletion {
    PathCompletion {
        path: dir.join(&entry.name),
        title: entry.name.clone(),
        folder: entry.folder,
        modified: entry.modified,
        highlights,
    }
}

/// What the `completion` thread is sent.
#[derive(Debug)]
pub enum Job {
    Ask(Ask),
    Configure(Option<PathBuf>, Settings),
}

/// Where requests for completions are answered.
#[derive(Debug)]
pub enum Completions {
    /// At once, on the thread that asks: until [`Completions::start`], and
    /// in tests.
    Here(Box<Completer>),
    /// On the `completion` thread, which answers with
    /// [`Event::Completed`]; with the settings it was last sent, to go on
    /// here should the thread be gone.
    Thread {
        jobs: Sender<Job>,
        home: Option<PathBuf>,
        settings: Settings,
    },
}

impl Default for Completions {
    fn default() -> Self {
        Completions::Here(Box::new(Completer::new(None, Settings::default())))
    }
}

impl Completions {
    /// Answers from here on on a thread of its own, which sends each answer
    /// to `events`.
    pub fn start(&mut self, events: Sender<Event>) {
        let Completions::Here(completer) = std::mem::take(self) else {
            return;
        };
        let (home, settings) = (completer.home.clone(), completer.settings.clone());
        let (jobs, received) = mpsc::channel();
        let started = thread::Builder::new()
            .name("completion".into())
            .spawn(move || serve(*completer, &received, &events));
        match started {
            Ok(_) => *self = Completions::Thread { jobs, home, settings },
            Err(err) => {
                log::warn!("couldn't start the completion thread, so completions are worked out inline: {err}");
                *self = Completions::Here(Box::new(Completer::new(home, settings)));
            }
        }
    }

    /// Takes new `[files]` settings and home directory.
    pub fn configure(&mut self, home: Option<PathBuf>, settings: Settings) {
        match self {
            Completions::Here(completer) => completer.configure(home, settings),
            Completions::Thread {
                jobs,
                home: kept_home,
                settings: kept,
            } => {
                kept_home.clone_from(&home);
                kept.clone_from(&settings);
                if jobs.send(Job::Configure(home, settings)).is_err() {
                    self.fall_back();
                }
            }
        }
    }

    /// Asks for completions: the answer, when it is worked out here, or
    /// `None` when the `completion` thread sends it.
    pub fn ask(&mut self, ask: Ask) -> Option<(FrontEndId, ToFrontEnd)> {
        match self {
            Completions::Here(completer) => {
                let entries = completer.complete(&ask.wanted, &ask.text);
                Some((ask.front_end, ask.answer(entries)))
            }
            Completions::Thread { jobs, .. } => {
                let Err(mpsc::SendError(Job::Ask(ask))) = jobs.send(Job::Ask(ask)) else {
                    return None;
                };
                self.fall_back();
                self.ask(ask)
            }
        }
    }

    /// Answers on the thread that asks, as the `completion` thread is gone,
    /// as after a panic.
    fn fall_back(&mut self) {
        let Completions::Thread { home, settings, .. } = std::mem::take(self) else {
            return;
        };
        log::warn!("the completion thread is gone, so completions are worked out inline");
        *self = Completions::Here(Box::new(Completer::new(home, settings)));
    }
}

/// The `completion` thread: answers each request that no later one for the
/// same field has made stale by the time it is read.
fn serve(mut completer: Completer, jobs: &Receiver<Job>, events: &Sender<Event>) {
    while let Ok(first) = jobs.recv() {
        let mut queued = vec![first];
        queued.extend(jobs.try_iter());
        let mut asks: Vec<Ask> = Vec::new();
        for job in queued {
            match job {
                Job::Configure(home, settings) => completer.configure(home, settings),
                Job::Ask(ask) => {
                    asks.retain(|earlier| !earlier.replaced_by(&ask));
                    asks.push(ask);
                }
            }
        }
        for ask in asks {
            let entries = completer.complete(&ask.wanted, &ask.text);
            let front_end = ask.front_end;
            let event = Event::Completed {
                front_end,
                message: ask.answer(entries),
            };
            if events.send(event).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
