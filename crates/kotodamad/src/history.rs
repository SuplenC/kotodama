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

/// How long a pick takes to count half as much. A pick never counts for
/// nothing: it only gets weaker.
pub const HALF_LIFE_SECS: f64 = 14.0 * 24.0 * 60.0 * 60.0;
/// How many picks of one Result for one query make it that query's Learned
/// choice.
pub const LEARN_AFTER: u8 = 3;
/// How many picks of another Result, since the user last picked the Learned
/// choice, replace it.
pub const TAKE_OVER_AFTER: u8 = 2;
/// How many of the Results ranked above a Learned choice when it was picked
/// are remembered as passed over.
const PASSED_OVER: usize = 8;
/// How many Results per query are counted towards becoming its Learned choice.
const CONTENDERS: usize = 4;
/// The most Results and queries kept; past these the least recent go.
const MAX_RESULTS: usize = 2000;
const MAX_QUERIES: usize = 1000;

/// A Result, by the name of the Extension that owns its Item and the Item's id,
/// or, for an Item with an identity, by [`IDENTITY`] and the identity.
pub type ResultKey = (String, String);

/// What the History keeps Items with an identity under, in place of an
/// Extension's name, so the same thing from several Extensions shares one
/// History. No Extension can be named this.
pub const IDENTITY: &str = "*";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// By Extension name, then Item id; or by [`IDENTITY`], then identity.
    /// An Item with an identity is kept both ways: its identity's picks rank
    /// it, and its own put it ahead of the same thing from another Extension.
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
        decay(self.score, self.at, at)
    }
}

/// `score` as of `then`, decayed to `at`.
fn decay(score: f64, then: u64, at: u64) -> f64 {
    let elapsed = at.saturating_sub(then) as f64;
    score * 0.5f64.powf(elapsed / HALF_LIFE_SECS)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Learned {
    choice: Option<ResultKey>,
    /// The decayed number of picks of the choice, as of `at`.
    #[serde(default)]
    strength: f64,
    /// The Results ranked above the choice when it was picked, most recent
    /// first. The choice ranks above these even when they match exactly.
    #[serde(default)]
    passed_over: Vec<ResultKey>,
    /// Picks of other Results since the choice was last picked, most recent
    /// first.
    contenders: Vec<(ResultKey, u8)>,
    at: u64,
}

/// A Learned choice that applies to a query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Choice<'a> {
    pub extension: &'a str,
    pub id: &'a str,
    /// How many picks it was learned from, decayed.
    pub strength: f64,
    /// What it was picked over.
    pub passed_over: &'a [ResultKey],
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

    /// Records that the user picked a Result while `query` was typed, with
    /// `above` the Results ranked above it, nearest first. A Result picked
    /// [`LEARN_AFTER`] times for one query becomes its Learned choice, and
    /// one picked [`TAKE_OVER_AFTER`] times since the choice was last picked
    /// replaces it.
    pub fn pick(&mut self, extension: &str, id: &str, query: &str, above: &[ResultKey], at: u64) {
        self.bump(extension, id, at);

        let key = query_key(query);
        if !key.is_empty() {
            let learned = self.learned.entry(key).or_insert_with(|| Learned {
                choice: None,
                strength: 0.0,
                passed_over: Vec::new(),
                contenders: Vec::new(),
                at,
            });
            let picked = |key: &ResultKey| key.0 == extension && key.1 == id;
            if learned.choice.as_ref().is_some_and(picked) {
                learned.strength = decay(learned.strength, learned.at, at) + 1.0;
                learned.contenders.clear();
                for key in above.iter().rev() {
                    learned.passed_over.retain(|old| old != key);
                    learned.passed_over.insert(0, key.clone());
                }
                learned.passed_over.truncate(PASSED_OVER);
            } else {
                let count = match learned.contenders.iter().position(|(key, _)| picked(key)) {
                    Some(n) => learned.contenders.remove(n).1 + 1,
                    None => 1,
                };
                let needed = if learned.choice.is_some() {
                    TAKE_OVER_AFTER
                } else {
                    LEARN_AFTER
                };
                if count >= needed {
                    learned.choice = Some((extension.to_owned(), id.to_owned()));
                    learned.strength = f64::from(count);
                    learned.passed_over = above.iter().take(PASSED_OVER).cloned().collect();
                    learned.contenders.clear();
                } else {
                    learned.strength = decay(learned.strength, learned.at, at);
                    learned.contenders.truncate(CONTENDERS - 1);
                    learned
                        .contenders
                        .insert(0, ((extension.to_owned(), id.to_owned()), count));
                }
            }
            learned.at = at;
        }
        self.trim();
    }

    /// Records a pick for Frecency alone, with nothing learned for the query.
    pub fn touch(&mut self, extension: &str, id: &str, at: u64) {
        self.bump(extension, id, at);
        self.trim();
    }

    fn bump(&mut self, extension: &str, id: &str, at: u64) {
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
    }

    /// One Result's Frecency at time `at`; 0 if it was never picked.
    pub fn frecency(&self, extension: &str, id: &str, at: u64) -> f64 {
        self.picks
            .get(extension)
            .and_then(|ids| ids.get(id))
            .map_or(0.0, |picks| picks.frecency(at))
    }

    /// Every Result picked and kept.
    pub fn picked(&self) -> impl Iterator<Item = (&str, &str, Picks)> {
        self.picks.iter().flat_map(|(extension, ids)| {
            ids.iter()
                .map(move |(id, picks)| (extension.as_str(), id.as_str(), *picks))
        })
    }

    /// The Learned choices that apply to a query, given as [`query_key`]
    /// gives it: those learned for it, for a query it starts with, or for one
    /// that starts with it. The strongest come first, and of two as strong,
    /// the one learned for the longer query.
    pub fn learned(&self, key: &str, at: u64) -> Vec<Choice<'_>> {
        let mut choices: Vec<(usize, Choice)> = self
            .learned
            .iter()
            .filter(|(learned_for, _)| {
                !key.is_empty()
                    && (learned_for.starts_with(key) || key.starts_with(learned_for.as_str()))
            })
            .filter_map(|(learned_for, learned)| {
                let (extension, id) = learned.choice.as_ref()?;
                Some((
                    learned_for.len(),
                    Choice {
                        extension,
                        id,
                        strength: decay(learned.strength, learned.at, at),
                        passed_over: &learned.passed_over,
                    },
                ))
            })
            .collect();
        choices.sort_by(|(a_len, a), (b_len, b)| {
            b.strength.total_cmp(&a.strength).then(b_len.cmp(a_len))
        });
        choices.into_iter().map(|(_, choice)| choice).collect()
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.version += 1;
        self.picks.clear();
        self.learned.clear();
    }

    /// Forgets the weakest Results and the least recent queries past the caps.
    fn trim(&mut self) {
        let results: usize = self.picks.values().map(HashMap::len).sum();
        if results > MAX_RESULTS {
            let at = self.picked().map(|(.., p)| p.at).max().unwrap_or(0);
            let mut scores: Vec<f64> = self.picked().map(|(.., p)| p.frecency(at)).collect();
            let cut = results - MAX_RESULTS;
            scores.select_nth_unstable_by(cut, f64::total_cmp);
            let floor = scores[cut];
            for ids in self.picks.values_mut() {
                ids.retain(|_, picks| picks.frecency(at) >= floor);
            }
            self.picks.retain(|_, ids| !ids.is_empty());
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
