//! What the user picked, which Frecency and Learned choices rank by. It is
//! kept in `$XDG_STATE_HOME/kotodama/history.json` and written after every
//! pick, on a thread of its own so the Daemon never waits for the disk.
//!
//! Times are passed in by the caller, as [`When`], so the History is the same
//! in tests as on a real clock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::{fs, io};

use kotodama_protocol::{Table, Value};
use serde::{Deserialize, Serialize};

use crate::clock::{DAY_SECS, When};
use crate::host::spawn;
use crate::usage::{self, Entry};

const DAY: f64 = DAY_SECS as f64;

/// How many of the Results ranked above a Learned choice when it was picked
/// are remembered as passed over.
const PASSED_OVER: usize = 8;
/// How many Results per query are counted towards becoming its Learned choice.
const CONTENDERS: usize = 4;
/// The most Results and queries kept; past these the least recent go.
const MAX_RESULTS: usize = 2000;
const MAX_QUERIES: usize = 1000;

/// How the History learns and how much it counts, from the config's
/// `[history]` table:
///
/// ```toml
/// [history]
/// picks_to_learn = 3      # picks of one Result for one query that make it the query's Learned choice
/// picks_to_take_over = 2  # picks of another Result, since the choice was last picked, that replace it
/// half_life_days = 14     # how long a pick takes to count half as much
/// lift_threshold = 5.0    # the Frecency that lifts a Result one Tier: about a week of daily use
/// time_of_day = 0.5       # how much less a pick counts at a time of day it is seldom made: 0 to 1
/// usage_log = false       # log queries and picks, to tune ranking from real use
/// ```
///
/// A setting that doesn't parse is logged and keeps its default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// How many picks of one Result for one query make it that query's
    /// Learned choice.
    pub picks_to_learn: u8,
    /// How many picks of another Result, since the user last picked the
    /// Learned choice, replace it.
    pub picks_to_take_over: u8,
    /// How long a pick takes to count half as much. A pick never counts for
    /// nothing: it only gets weaker.
    pub half_life_secs: f64,
    /// The Frecency from which a Result ranks one Tier above how well it
    /// matches. The default, about a week of daily use, lets something used
    /// daily whose title starts with the query beat an exact title, but not a
    /// few picks.
    pub lift_threshold: f64,
    /// How much less a Result's picks count at a time of day it is seldom
    /// picked: 0 not at all, 1 for nothing at a time it is never picked. At
    /// the times it is picked most they count in full.
    pub time_of_day: f64,
    /// Whether queries and picks go in the Usage log (see [`crate::usage`]).
    pub usage_log: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            picks_to_learn: 3,
            picks_to_take_over: 2,
            half_life_secs: 14.0 * DAY,
            lift_threshold: 5.0,
            time_of_day: 0.5,
            usage_log: false,
        }
    }
}

impl Settings {
    /// The `[history]` table of the merged config.
    pub fn from_config(config: &Table) -> Self {
        let mut settings = Settings::default();
        let table = match config.get("history") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                log::error!("`history` should be a table");
                return settings;
            }
            None => return settings,
        };
        for (key, value) in table {
            let set = match key.as_str() {
                "picks_to_learn" => picks(value).map(|n| settings.picks_to_learn = n),
                "picks_to_take_over" => picks(value).map(|n| settings.picks_to_take_over = n),
                "half_life_days" => number(value)
                    .filter(|&days| days > 0.0)
                    .ok_or("should be a number of days above 0")
                    .map(|days| settings.half_life_secs = days * DAY),
                "lift_threshold" => number(value)
                    .filter(|&frecency| frecency > 0.0)
                    .ok_or("should be a number above 0")
                    .map(|frecency| settings.lift_threshold = frecency),
                "time_of_day" => number(value)
                    .filter(|weight| (0.0..=1.0).contains(weight))
                    .ok_or("should be a number from 0 to 1")
                    .map(|weight| settings.time_of_day = weight),
                "usage_log" => match value {
                    Value::Bool(on) => {
                        settings.usage_log = *on;
                        Ok(())
                    }
                    _ => Err("should be true or false"),
                },
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                log::error!("history.{key}: {err}");
            }
        }
        settings
    }
}

/// A number of picks: 1 to 255.
fn picks(value: &Value) -> Result<u8, &'static str> {
    match value {
        Value::Integer(n) => u8::try_from(*n)
            .ok()
            .filter(|&n| n > 0)
            .ok_or("should be a number of picks from 1 to 255"),
        _ => Err("should be a number of picks from 1 to 255"),
    }
}

/// A finite number, whole or not.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(n) => Some(*n as f64),
        Value::Float(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}

/// A Result, by the name of the Extension that owns its Item and the Item's id,
/// or, for an Item with an Identity, by [`IDENTITIES`] and the Identity.
pub type ResultKey = (String, String);

/// What the History keeps Items with an Identity under, in place of an
/// Extension's name, so the same thing from several Extensions shares one
/// History. No Extension can be named this.
pub const IDENTITIES: &str = "*";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// By Extension name, then Item id; or by [`IDENTITIES`], then Identity.
    /// An Item with an Identity is kept both ways: its Identity's picks rank
    /// it, and its own put it ahead of the same thing from another Extension.
    #[serde(default)]
    picks: HashMap<String, HashMap<String, Picks>>,
    /// By query, as [`query_key`] gives it.
    #[serde(default)]
    learned: HashMap<String, Learned>,
    #[serde(skip)]
    settings: Settings,
    /// Bumped on every change, so a cache built from the History knows when it
    /// is stale.
    #[serde(skip)]
    version: u64,
}

/// How many parts the day is cut into, for learning when a Result is picked.
const HOURS: usize = 6;
/// How long each part is.
const HOURS_SECS: f64 = DAY / HOURS as f64;
/// What [`Picks::hours`] adds up to.
const SHARES: f64 = u8::MAX as f64;

/// How often, how recently and at what times of day one Result was picked.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Picks {
    /// The decayed number of picks, as of `at`.
    score: f64,
    at: u64,
    /// How the picks fall across the day: in [`HOURS`] parts from midnight,
    /// each as its share of `score`, out of [`SHARES`]. All zero when it isn't
    /// known, for picks kept before it was.
    #[serde(default, skip_serializing_if = "unknown")]
    hours: [u8; HOURS],
}

fn unknown(hours: &[u8; HOURS]) -> bool {
    hours.iter().all(|&share| share == 0)
}

/// The two parts of the day a time of day falls between, and how near it is
/// to the middle of each, from 0 to 1. A pick counts towards both and a time
/// reads both, so a habit doesn't stop at a part's end.
fn parts(time_of_day: u32) -> [(usize, f64); 2] {
    let x = f64::from(time_of_day) / HOURS_SECS - 0.5;
    let before = x.floor();
    let after = x - before;
    let first = (before as i64).rem_euclid(HOURS as i64) as usize;
    [(first, 1.0 - after), ((first + 1) % HOURS, after)]
}

impl Picks {
    fn new(at: u64) -> Self {
        Picks {
            score: 0.0,
            at,
            hours: [0; HOURS],
        }
    }

    /// The decayed number of picks at time `at`: every pick counts 1 when
    /// made, and half as much every `half_life` seconds after.
    fn count(&self, at: u64, half_life: f64) -> f64 {
        decay(self.score, self.at, at, half_life)
    }

    /// The Frecency at `when`: the decayed number of picks, which count less
    /// at a time of day the Result is seldom picked, as `settings` say.
    pub fn frecency(&self, when: When, settings: &Settings) -> f64 {
        let count = self.count(when.at, settings.half_life_secs);
        count * (1.0 - settings.time_of_day * (1.0 - self.usual(when.time_of_day)))
    }

    /// The decayed number of picks made near the time of day of `when`,
    /// and of all picks, as the empty query ranks by.
    pub fn used(&self, when: When, half_life: f64) -> (f64, f64) {
        let count = self.count(when.at, half_life);
        (count * self.usual(when.time_of_day), count)
    }

    /// How usual a time of day is for picking it: 1 at the times it is picked
    /// most, or when that isn't known, down to 0 at times it never is.
    fn usual(&self, time_of_day: u32) -> f64 {
        let most = self.hours.iter().copied().max().unwrap_or(0);
        if most == 0 {
            return 1.0;
        }
        let parts = parts(time_of_day);
        let read: f64 = parts
            .iter()
            .map(|&(part, near)| near * f64::from(self.hours[part]))
            .sum();
        // So picks read at the time of day they were made read in full.
        let whole: f64 = parts.iter().map(|&(_, near)| near * near).sum();
        (read / (whole * f64::from(most))).min(1.0)
    }

    /// Adds a pick made at `when`.
    fn add(&mut self, when: When, half_life: f64) {
        let before = self.count(when.at, half_life);
        let total: f64 = self.hours.iter().copied().map(f64::from).sum();
        // Picks made at times of day that aren't known count as spread evenly.
        let mut hours = self.hours.map(|share| {
            if total > 0.0 {
                before * f64::from(share) / total
            } else {
                before / HOURS as f64
            }
        });
        for (part, near) in parts(when.time_of_day) {
            hours[part] += near;
        }
        self.score = before + 1.0;
        self.at = when.at;
        self.hours = hours.map(|count| (count / self.score * SHARES).round() as u8);
    }
}

/// `score` as of `then`, decayed to `at`.
fn decay(score: f64, then: u64, at: u64, half_life: f64) -> f64 {
    let elapsed = at.saturating_sub(then) as f64;
    score * 0.5f64.powf(elapsed / half_life)
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

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes how the History learns and counts from now on. What it kept
    /// stays: picks already made decay at the new half-life, and counts
    /// towards a Learned choice are compared with the new ones at the next
    /// pick.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn is_empty(&self) -> bool {
        self.picks.is_empty() && self.learned.is_empty()
    }

    /// Records that the user picked a Result while `query` was typed, with
    /// `above` the Results ranked above it, nearest first. A Result picked
    /// [`Settings::picks_to_learn`] times for one query becomes its Learned
    /// choice, and one picked [`Settings::picks_to_take_over`] times since the
    /// choice was last picked replaces it.
    pub fn pick(
        &mut self,
        extension: &str,
        id: &str,
        query: &str,
        above: &[ResultKey],
        when: When,
    ) {
        self.bump(extension, id, when);
        let at = when.at;

        let key = query_key(query);
        let Settings {
            half_life_secs: half_life,
            picks_to_learn,
            picks_to_take_over,
            ..
        } = self.settings;
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
                learned.strength = decay(learned.strength, learned.at, at, half_life) + 1.0;
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
                    picks_to_take_over
                } else {
                    picks_to_learn
                };
                if count >= needed {
                    learned.choice = Some((extension.to_owned(), id.to_owned()));
                    learned.strength = f64::from(count);
                    learned.passed_over = above.iter().take(PASSED_OVER).cloned().collect();
                    learned.contenders.clear();
                } else {
                    learned.strength = decay(learned.strength, learned.at, at, half_life);
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
    pub fn touch(&mut self, extension: &str, id: &str, when: When) {
        self.bump(extension, id, when);
        self.trim();
    }

    fn bump(&mut self, extension: &str, id: &str, when: When) {
        self.version += 1;
        self.picks
            .entry(extension.to_owned())
            .or_default()
            .entry(id.to_owned())
            .or_insert(Picks::new(when.at))
            .add(when, self.settings.half_life_secs);
    }

    /// How one Result was picked, if it was.
    pub fn picks(&self, extension: &str, id: &str) -> Option<&Picks> {
        self.picks.get(extension).and_then(|ids| ids.get(id))
    }

    /// One Result's Frecency at `when`; 0 if it was never picked.
    pub fn frecency(&self, extension: &str, id: &str, when: When) -> f64 {
        self.picks(extension, id)
            .map_or(0.0, |picks| picks.frecency(when, &self.settings))
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
                        strength: decay(
                            learned.strength,
                            learned.at,
                            at,
                            self.settings.half_life_secs,
                        ),
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
            let half_life = self.settings.half_life_secs;
            let mut scores: Vec<f64> = self
                .picked()
                .map(|(.., p)| p.count(at, half_life))
                .collect();
            let cut = results - MAX_RESULTS;
            scores.select_nth_unstable_by(cut, f64::total_cmp);
            let floor = scores[cut];
            for ids in self.picks.values_mut() {
                ids.retain(|_, picks| picks.count(at, half_life) >= floor);
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

/// Where the Daemon keeps its History, and its Usage log.
pub trait Store {
    /// Called after every change. Must not block.
    fn save(&mut self, history: &History);
    /// Adds an entry to the Usage log. Called only while the config turns the
    /// log on. Must not block.
    fn log(&mut self, entry: Entry);
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
/// The Usage log goes to a file of its own, on another thread, started only
/// once there is something to log.
pub struct Disk {
    saves: Sender<Vec<u8>>,
    usage: PathBuf,
    usage_log: Option<usage::Writer>,
}

impl Disk {
    pub fn new(path: PathBuf, usage: PathBuf) -> Self {
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
        Disk {
            saves,
            usage,
            usage_log: None,
        }
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

    fn log(&mut self, entry: Entry) {
        self.usage_log
            .get_or_insert_with(|| usage::Writer::new(self.usage.clone()))
            .write(entry);
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

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u32 = 3600;

    /// `hour` seconds into day `day`, in UTC.
    fn when(day: u64, hour: u32) -> When {
        When {
            at: 1_792_368_000 + day * DAY_SECS + u64::from(hour),
            time_of_day: hour,
        }
    }

    #[test]
    fn a_result_takes_a_few_dozen_bytes_however_often_it_is_picked() {
        let mut history = History::default();
        for day in 0..100 {
            for hour in 0..24 {
                history.touch("apps", "firefox", when(day, hour * HOUR + 17 * 60));
            }
        }
        let json = serde_json::to_string(&history).unwrap();
        let entry = json.find("\"firefox\"").map(|at| &json[at..]).unwrap();
        let entry = &entry[..=entry.find('}').unwrap()];
        assert!(entry.len() < 90, "{entry}");
    }

    #[test]
    fn picks_kept_before_times_of_day_were_count_in_full_at_any_time() {
        let history: History = serde_json::from_str(
            r#"{"picks":{"apps":{"firefox":{"score":2.0,"at":1792368000}}},"learned":{}}"#,
        )
        .unwrap();
        assert_eq!(counts(&history, "firefox", when(0, 3 * HOUR)), 1.0);
        assert_eq!(counts(&history, "firefox", when(0, 15 * HOUR)), 1.0);
    }

    #[test]
    fn picks_count_in_full_at_the_time_of_day_they_are_made_and_less_away_from_it() {
        let mut history = History::default();
        for day in 0..5 {
            history.touch("apps", "mail", when(day, 7 * HOUR));
        }
        assert_eq!(counts(&history, "mail", when(5, 7 * HOUR)), 1.0);
        let near = counts(&history, "mail", when(5, 9 * HOUR));
        assert!(near > 0.8 && near < 1.0, "two hours later: {near}");
        assert_eq!(counts(&history, "mail", when(5, 19 * HOUR)), 0.5, "away");
    }

    /// How much an app's picks count at `when`, of their decayed number.
    fn counts(history: &History, id: &str, when: When) -> f64 {
        let picks = history.picks("apps", id).unwrap();
        let count = picks.count(when.at, history.settings().half_life_secs);
        history.frecency("apps", id, when) / count
    }
}
