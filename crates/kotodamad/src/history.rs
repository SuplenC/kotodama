//! What the user picked, which Frecency and Learned choices rank by. It is
//! kept in `$XDG_STATE_HOME/kotodama/history.json` and written after every
//! pick, on a thread of its own so the Daemon never waits for the disk.
//!
//! Times are seconds since the Unix epoch, passed in by the caller, so the
//! History is the same in tests as on a real clock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::host::spawn;

/// How long a pick takes to count half as much.
pub const HALF_LIFE_SECS: f64 = 14.0 * 24.0 * 60.0 * 60.0;
/// A Result whose Frecency has decayed below this is forgotten.
const FORGOTTEN: f64 = 0.1;
/// How many picks of one Result for one query make it that query's Learned
/// choice.
pub const LEARN_AFTER: u8 = 3;
/// How many Results per query are counted towards becoming its Learned choice.
const CONTENDERS: usize = 4;
/// The most Results and queries kept; past these the least recent go.
const MAX_RESULTS: usize = 2000;
const MAX_QUERIES: usize = 1000;

/// A Result, by the name of the Extension that owns its Item and the Item's id.
pub type ResultKey = (String, String);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// By Extension name, then Item id.
    #[serde(default)]
    picks: HashMap<String, HashMap<String, Picks>>,
    /// By query, as [`query_key`] gives it.
    #[serde(default)]
    learned: HashMap<String, Learned>,
    /// Bumped on every change, so a cache built from the History knows when it
    /// is stale.
    #[serde(skip)]
    version: u64,
}

/// How often and how recently one Result was picked.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Picks {
    /// The decayed number of picks, as of `at`.
    score: f64,
    at: u64,
}

impl Picks {
    /// The Frecency at time `at`: every pick counts 1 when made, and half as
    /// much every [`HALF_LIFE_SECS`] after.
    pub fn frecency(&self, at: u64) -> f64 {
        let elapsed = at.saturating_sub(self.at) as f64;
        let score = self.score * 0.5f64.powf(elapsed / HALF_LIFE_SECS);
        if score < FORGOTTEN { 0.0 } else { score }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Learned {
    choice: Option<ResultKey>,
    /// Picks of other Results since the choice was made, most recent first.
    contenders: Vec<(ResultKey, u8)>,
    at: u64,
}

/// The form of a query that Learned choices are kept under: trimmed,
/// lowercase, with runs of whitespace as one space.
pub fn query_key(query: &str) -> String {
    let mut key = String::with_capacity(query.len());
    for word in query.split_whitespace() {
        if !key.is_empty() {
            key.push(' ');
        }
        key.extend(word.chars().flat_map(char::to_lowercase));
    }
    key
}

impl History {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_empty(&self) -> bool {
        self.picks.is_empty() && self.learned.is_empty()
    }

    /// Records that the user picked a Result while `query` was typed. A Result
    /// picked [`LEARN_AFTER`] times for one query becomes its Learned choice,
    /// and another picked as often since replaces it.
    pub fn pick(&mut self, extension: &str, id: &str, query: &str, at: u64) {
        self.version += 1;
        let picks = self
            .picks
            .entry(extension.to_owned())
            .or_default()
            .entry(id.to_owned())
            .or_insert(Picks { score: 0.0, at });
        *picks = Picks {
            score: picks.frecency(at) + 1.0,
            at,
        };

        let key = query_key(query);
        if !key.is_empty() {
            let learned = self.learned.entry(key).or_insert_with(|| Learned {
                choice: None,
                contenders: Vec::new(),
                at,
            });
            learned.at = at;
            let picked = |key: &ResultKey| key.0 == extension && key.1 == id;
            if !learned.choice.as_ref().is_some_and(picked) {
                let count = match learned.contenders.iter().position(|(key, _)| picked(key)) {
                    Some(n) => learned.contenders.remove(n).1 + 1,
                    None => 1,
                };
                if count >= LEARN_AFTER {
                    learned.choice = Some((extension.to_owned(), id.to_owned()));
                    learned.contenders.clear();
                } else {
                    learned.contenders.truncate(CONTENDERS - 1);
                    learned
                        .contenders
                        .insert(0, ((extension.to_owned(), id.to_owned()), count));
                }
            }
        }
        self.trim(at);
    }

    /// One Result's Frecency at time `at`; 0 if it was never picked.
    pub fn frecency(&self, extension: &str, id: &str, at: u64) -> f64 {
        self.picks
            .get(extension)
            .and_then(|ids| ids.get(id))
            .map_or(0.0, |picks| picks.frecency(at))
    }

    /// Every Result ever picked and not yet forgotten.
    pub fn picked(&self) -> impl Iterator<Item = (&str, &str, Picks)> {
        self.picks.iter().flat_map(|(extension, ids)| {
            ids.iter()
                .map(move |(id, picks)| (extension.as_str(), id.as_str(), *picks))
        })
    }

    /// The Learned choice for a query given as [`query_key`] gives it.
    pub fn learned(&self, key: &str) -> Option<(&str, &str)> {
        let (extension, id) = self.learned.get(key)?.choice.as_ref()?;
        Some((extension, id))
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.version += 1;
        self.picks.clear();
        self.learned.clear();
    }

    /// Forgets Results that have decayed away, then the least recent Results
    /// and queries past the caps.
    fn trim(&mut self, at: u64) {
        for ids in self.picks.values_mut() {
            ids.retain(|_, picks| picks.frecency(at) > 0.0);
        }
        self.picks.retain(|_, ids| !ids.is_empty());

        let results: usize = self.picks.values().map(HashMap::len).sum();
        if results > MAX_RESULTS {
            let mut scores: Vec<f64> = self.picked().map(|(.., p)| p.frecency(at)).collect();
            let cut = results - MAX_RESULTS;
            scores.select_nth_unstable_by(cut, f64::total_cmp);
            let floor = scores[cut];
            for ids in self.picks.values_mut() {
                ids.retain(|_, picks| picks.frecency(at) >= floor);
            }
        }
        if self.learned.len() > MAX_QUERIES {
            let mut times: Vec<u64> = self.learned.values().map(|l| l.at).collect();
            let cut = self.learned.len() - MAX_QUERIES;
            times.select_nth_unstable(cut);
            let floor = times[cut];
            self.learned.retain(|_, learned| learned.at >= floor);
        }
    }
}

/// Where the Daemon keeps its History.
pub trait Store {
    /// Called after every change. Must not block.
    fn save(&mut self, history: &History);
}

/// Reads the History at `path`. A missing file is an empty History; one that
/// can't be read is logged and replaced at the next save.
pub fn load(path: &Path) -> History {
    let text = match fs::read(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return History::default(),
        Err(err) => {
            log::error!("{}: starting with an empty history: {err}", path.display());
            return History::default();
        }
    };
    serde_json::from_slice(&text).unwrap_or_else(|err| {
        log::error!("{}: starting with an empty history: {err}", path.display());
        History::default()
    })
}

/// Writes the History to a file, replacing it whole, on a thread of its own.
/// Saves that pile up while the disk is slow are written once, latest only.
pub struct Disk {
    saves: Sender<Vec<u8>>,
}

impl Disk {
    pub fn new(path: PathBuf) -> Self {
        let (saves, pending) = mpsc::channel::<Vec<u8>>();
        spawn("history".into(), move || {
            while let Ok(mut bytes) = pending.recv() {
                while let Ok(later) = pending.try_recv() {
                    bytes = later;
                }
                if let Err(err) = write(&path, &bytes) {
                    log::error!("couldn't save the history: {}: {err}", path.display());
                }
            }
        });
        Disk { saves }
    }
}

impl Store for Disk {
    fn save(&mut self, history: &History) {
        match serde_json::to_vec(history) {
            Ok(bytes) => {
                let _ = self.saves.send(bytes);
            }
            Err(err) => log::error!("couldn't encode the history: {err}"),
        }
    }
}

/// Writes a sibling file and renames it over `path`, so a crash mid-write
/// leaves the old History whole.
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("json.partial");
    fs::write(&partial, bytes)?;
    fs::rename(&partial, path)
}
