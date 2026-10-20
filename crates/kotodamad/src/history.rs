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
const PARTS: usize = 8;
/// How long each part of the day is.
const PART_SECS: f64 = DAY / PARTS as f64;
/// What a Result's shares of its picks across the day add up to. Fine enough
/// that one more pick still moves them at hundreds of picks a day.
const SHARES: f64 = 10_000.0;

/// How often, how recently and at what times of day one Result was picked.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Picks {
    /// The decayed number of picks, as of `at`. An `f32`, so the History
    /// file keeps its shortest form: ranking needs no more digits.
    score: f32,
    at: u64,
    /// How the picks fall across the day: in [`PARTS`] parts from midnight,
    /// each as its share of `score`, out of [`SHARES`]. All zero when it isn't
    /// known, for picks kept before it was.
    #[serde(default, skip_serializing_if = "unknown")]
    parts: [u16; PARTS],
}

fn unknown(parts: &[u16; PARTS]) -> bool {
    parts.iter().all(|&share| share == 0)
}

/// The two parts of the day a time of day falls between, and how near it is
/// to the middle of each, from 0 to 1. A pick counts towards both and a time
/// reads both, so a habit doesn't stop at a part's end.
fn parts(time_of_day: u32) -> [(usize, f64); 2] {
    let x = f64::from(time_of_day) / PART_SECS - 0.5;
    let before = x.floor();
    let after = x - before;
    let first = (before as i64).rem_euclid(PARTS as i64) as usize;
    [(first, 1.0 - after), ((first + 1) % PARTS, after)]
}

/// How much a Result was picked, as the empty query ranks it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Habit {
    /// The decayed number of picks made near this time of day.
    pub near_now: f64,
    /// The decayed number of all picks.
    pub overall: f64,
}

impl Habit {
    /// What two ways to one Result add up to.
    pub fn and(self, other: Habit) -> Habit {
        Habit {
            near_now: self.near_now + other.near_now,
            overall: self.overall + other.overall,
        }
    }

    /// The more used of the two, for each count.
    pub fn max(self, other: Habit) -> Habit {
        Habit {
            near_now: self.near_now.max(other.near_now),
            overall: self.overall.max(other.overall),
        }
    }

    /// Most used near now first, then most used overall.
    pub fn rank(&self, other: &Habit) -> std::cmp::Ordering {
        other
            .near_now
            .total_cmp(&self.near_now)
            .then(other.overall.total_cmp(&self.overall))
    }
}

impl Picks {
    fn new(at: u64) -> Self {
        Picks {
            score: 0.0,
            at,
            parts: [0; PARTS],
        }
    }

    /// The decayed number of picks at time `at`: every pick counts 1 when
    /// made, and half as much every `half_life` seconds after.
    fn count(&self, at: u64, half_life: f64) -> f64 {
        decay(f64::from(self.score), self.at, at, half_life)
    }

    /// The Frecency at `when`: the decayed number of picks, which count less
    /// at a time of day the Result is seldom picked, as `settings` say.
    pub fn frecency(&self, when: When, settings: &Settings) -> f64 {
        let count = self.count(when.at, settings.half_life_secs);
        count * (1.0 - settings.time_of_day * (1.0 - self.usual(when.time_of_day)))
    }

    /// How much it was picked as of `when`, near that time of day and overall.
    pub fn habit(&self, when: When, half_life: f64) -> Habit {
        let overall = self.count(when.at, half_life);
        Habit {
            near_now: overall * self.usual(when.time_of_day),
            overall,
        }
    }

    /// How usual a time of day is for picking it: 1 at the times it is picked
    /// most, or when that isn't known, down to 0 at times it never is.
    fn usual(&self, time_of_day: u32) -> f64 {
        let most = self.parts.iter().copied().max().unwrap_or(0);
        if most == 0 {
            return 1.0;
        }
        let parts = parts(time_of_day);
        let read: f64 = parts
            .iter()
            .map(|&(part, near)| near * f64::from(self.parts[part]))
            .sum();
        // So picks read at the time of day they were made read in full.
        let whole: f64 = parts.iter().map(|&(_, near)| near * near).sum();
        (read / (whole * f64::from(most))).min(1.0)
    }

    /// Adds a pick made at `when`.
    fn add(&mut self, when: When, half_life: f64) {
        let before = self.count(when.at, half_life);
        let total: f64 = self.parts.iter().copied().map(f64::from).sum();
        // Picks made at times of day that aren't known count as spread evenly.
        let mut counts = self.parts.map(|share| {
            if total > 0.0 {
                before * f64::from(share) / total
            } else {
                before / PARTS as f64
            }
        });
        for (part, near) in parts(when.time_of_day) {
            counts[part] += near;
        }
        let score = before + 1.0;
        self.score = score as f32;
        self.at = when.at;
        self.parts = counts.map(|count| (count / score * SHARES).round() as u16);
    }
}

/// The Picks of many Results, as a struct of arrays, so their Habits or
/// Frecency at one time come out of one vectorised loop. Each one's picks are
/// decayed to the latest pick's time when it is made, so a time only
/// multiplies them by one factor, rather than raising a power per Result.
#[derive(Clone, Debug, Default)]
pub struct Frecent {
    /// The half-life `decayed` was worked out with.
    half_life: f64,
    /// When the latest pick was made: what `decayed` is decayed to.
    latest: u64,
    /// Each one's decayed number of picks as of `latest`.
    decayed: Vec<f64>,
    /// Each one's decayed number of picks as of its last pick, which it
    /// never counts more than, even at a time before that.
    score: Vec<f64>,
    at: Vec<u64>,
    /// Each one's share of each part of the day, a column a part, as
    /// [`Picks`] keeps them.
    parts: [Vec<u16>; PARTS],
    /// Each one's largest share: 0 when the shares aren't known.
    most: Vec<u16>,
}

impl Frecent {
    /// `picks`, decayed at `half_life`.
    pub fn new(picks: &[Picks], half_life: f64) -> Self {
        let latest = picks.iter().map(|picks| picks.at).max().unwrap_or(0);
        let mut frecent = Frecent {
            half_life,
            latest,
            ..Frecent::default()
        };
        for picks in picks {
            frecent.decayed.push(picks.count(latest, half_life));
            frecent.score.push(picks.score.into());
            frecent.at.push(picks.at);
            for (column, &share) in frecent.parts.iter_mut().zip(&picks.parts) {
                column.push(share);
            }
            frecent
                .most
                .push(picks.parts.iter().copied().max().unwrap_or(0));
        }
        frecent
    }

    /// The half-life it decays at.
    pub fn half_life(&self) -> f64 {
        self.half_life
    }

    /// The `n`th Picks, as kept.
    pub fn picks(&self, n: usize) -> Picks {
        Picks {
            score: self.score[n] as f32,
            at: self.at[n],
            parts: std::array::from_fn(|part| self.parts[part][n]),
        }
    }

    /// Each one's Habit at `when`, as [`Picks::habit`] works it out, into
    /// `habits`.
    pub fn habits(&self, when: When, habits: &mut Vec<Habit>) {
        self.each(when, habits, |count, usual| Habit {
            near_now: count * usual,
            overall: count,
        });
    }

    /// Each one's Frecency at `when`, as [`Picks::frecency`] works it out
    /// with `time_of_day` as [`Settings::time_of_day`], into `frecency`.
    pub fn frecency(&self, when: When, time_of_day: f64, frecency: &mut Vec<f64>) {
        self.each(when, frecency, |count, usual| {
            count * (1.0 - time_of_day * (1.0 - usual))
        });
    }

    /// `with` each one's decayed number of picks at `when` and how usual
    /// `when`'s time of day is for picking it, as [`Picks::usual`] has it,
    /// into `out`.
    #[inline]
    fn each<T: Copy + Default>(&self, when: When, out: &mut Vec<T>, with: impl Fn(f64, f64) -> T) {
        // What a pick made at `latest` counts at `when`: more than 1 when is
        // before it, which `score` caps, as `decay` does.
        let elapsed = when.at as f64 - self.latest as f64;
        let factor = 0.5f64.powf(elapsed / self.half_life);
        let [(first, near_first), (second, near_second)] = parts(when.time_of_day);
        let whole = near_first * near_first + near_second * near_second;
        let len = self.decayed.len();
        out.clear();
        out.resize(len, T::default());
        // Sliced to one length, so the loop reads them without bounds checks.
        let (out, decayed, score) = (&mut out[..len], &self.decayed[..len], &self.score[..len]);
        let (first, second, most) = (
            &self.parts[first][..len],
            &self.parts[second][..len],
            &self.most[..len],
        );
        // HAZARD: vectorised (NEON `fmul`/`fdiv`/`fcmeq`, SSE2
        // `mulpd`/`divpd`): no branch, call or early exit in this loop. It is
        // the empty query's per-Result work.
        for n in 0..len {
            let count = decayed[n] * factor;
            let count = if count < score[n] { count } else { score[n] };
            let read = near_first * f64::from(first[n]) + near_second * f64::from(second[n]);
            let usual = read / (whole * f64::from(most[n]));
            let usual = if most[n] == 0 || usual > 1.0 {
                1.0
            } else {
                usual
            };
            out[n] = with(count, usual);
        }
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
    /// The decayed number of picks of the choice, as of `at`. An `f32`, as
    /// [`Picks`] keeps its score.
    #[serde(default)]
    strength: f32,
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
    /// choice was last picked replaces it. Returns the change, to undo it.
    pub fn pick(
        &mut self,
        extension: &str,
        id: &str,
        query: &str,
        above: &[ResultKey],
        when: When,
    ) -> Change {
        let mut change = self.bump(extension, id, when);
        let at = when.at;

        let key = query_key(query);
        let Settings {
            half_life_secs: half_life,
            picks_to_learn,
            picks_to_take_over,
            ..
        } = self.settings;
        if !key.is_empty() {
            let before = self.learned.get(&key).cloned();
            let learned = self.learned.entry(key.clone()).or_insert_with(|| Learned {
                choice: None,
                strength: 0.0,
                passed_over: Vec::new(),
                contenders: Vec::new(),
                at,
            });
            let picked = |key: &ResultKey| key.0 == extension && key.1 == id;
            if learned.choice.as_ref().is_some_and(picked) {
                learned.strength =
                    (decay(learned.strength.into(), learned.at, at, half_life) + 1.0) as f32;
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
                    learned.strength = f32::from(count);
                    learned.passed_over = above.iter().take(PASSED_OVER).cloned().collect();
                    learned.contenders.clear();
                } else {
                    learned.strength =
                        decay(learned.strength.into(), learned.at, at, half_life) as f32;
                    learned.contenders.truncate(CONTENDERS - 1);
                    learned
                        .contenders
                        .insert(0, ((extension.to_owned(), id.to_owned()), count));
                }
            }
            learned.at = at;
            let after = Some(learned.clone());
            change.learned.push(Replaced { key, before, after });
        }
        self.trim(&mut change);
        change
    }

    /// Records a pick for Frecency alone, with nothing learned for the query.
    pub fn touch(&mut self, extension: &str, id: &str, when: When) -> Change {
        let mut change = self.bump(extension, id, when);
        self.trim(&mut change);
        change
    }

    fn bump(&mut self, extension: &str, id: &str, when: When) -> Change {
        self.version += 1;
        let ids = self.picks.entry(extension.to_owned()).or_default();
        let before = ids.get(id).copied();
        let mut picks = before.unwrap_or(Picks::new(when.at));
        picks.add(when, self.settings.half_life_secs);
        ids.insert(id.to_owned(), picks);
        let key = (extension.to_owned(), id.to_owned());
        Change {
            picks: vec![Replaced {
                key,
                before,
                after: Some(picks),
            }],
            learned: Vec::new(),
        }
    }

    /// Sets or removes what the History keeps for one Result.
    fn set_picks(&mut self, (extension, id): ResultKey, picks: Option<Picks>) {
        match picks {
            Some(picks) => {
                self.picks.entry(extension).or_default().insert(id, picks);
            }
            None => {
                if let Some(ids) = self.picks.get_mut(&extension) {
                    ids.remove(&id);
                    if ids.is_empty() {
                        self.picks.remove(&extension);
                    }
                }
            }
        }
    }

    /// Forgets the Results `keys` name: their picks, and every Learned choice
    /// for them or count towards one. What other Learned choices were picked
    /// over stays.
    pub fn forget(&mut self, keys: &[ResultKey]) -> Change {
        self.forget_where(|(extension, id)| {
            keys.iter().any(|key| key.0 == extension && key.1 == id)
        })
    }

    /// Forgets every Result `forgotten` is true for, as [`forget`](Self::forget) does.
    pub fn forget_where(&mut self, forgotten: impl Fn((&str, &str)) -> bool) -> Change {
        self.version += 1;
        let mut change = Change::default();
        for (extension, ids) in &mut self.picks {
            ids.retain(|id, picks| {
                let forget = forgotten((extension, id));
                if forget {
                    let key = (extension.clone(), id.clone());
                    change.picks.push(Replaced::gone(key, *picks));
                }
                !forget
            });
        }
        self.picks.retain(|_, ids| !ids.is_empty());
        let is_forgotten = |key: &ResultKey| forgotten((&key.0, &key.1));
        self.learned.retain(|query, learned| {
            let choice = learned.choice.as_ref().is_some_and(is_forgotten);
            if !choice && !learned.contenders.iter().any(|(key, _)| is_forgotten(key)) {
                return true;
            }
            let before = learned.clone();
            if choice {
                learned.choice = None;
                learned.strength = 0.0;
                learned.passed_over.clear();
            }
            learned.contenders.retain(|(key, _)| !is_forgotten(key));
            let keep = learned.choice.is_some() || !learned.contenders.is_empty();
            let after = keep.then(|| learned.clone());
            change.learned.push(Replaced {
                key: query.clone(),
                before: Some(before),
                after,
            });
            keep
        });
        change
    }

    /// Undoes a change: puts back what it replaced, except where the History
    /// changed again since, which stays as it is now.
    pub fn undo(&mut self, change: Change) {
        self.version += 1;
        for Replaced { key, before, after } in change.picks.into_iter().rev() {
            if self.picks(&key.0, &key.1).copied() == after {
                self.set_picks(key, before);
            }
        }
        for Replaced {
            key: query,
            before,
            after,
        } in change.learned.into_iter().rev()
        {
            if self.learned.get(&query) == after.as_ref() {
                match before {
                    Some(learned) => self.learned.insert(query, learned),
                    None => self.learned.remove(&query),
                };
            }
        }
    }

    /// Whether the History keeps picks of any of an Extension's Results.
    pub fn has_extension(&self, extension: &str) -> bool {
        self.picks.contains_key(extension)
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
                            learned.strength.into(),
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
    pub fn clear(&mut self) -> Change {
        self.version += 1;
        let mut change = Change::default();
        for (extension, ids) in self.picks.drain() {
            for (id, picks) in ids {
                change
                    .picks
                    .push(Replaced::gone((extension.clone(), id), picks));
            }
        }
        for (query, learned) in self.learned.drain() {
            change.learned.push(Replaced::gone(query, learned));
        }
        change
    }

    /// Forgets the weakest Results and the least recent queries past the caps,
    /// into `change`.
    fn trim(&mut self, change: &mut Change) {
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
            for (extension, ids) in &mut self.picks {
                ids.retain(|id, picks| {
                    let keep = picks.count(at, half_life) >= floor;
                    if !keep {
                        let key = (extension.clone(), id.clone());
                        change.picks.push(Replaced::gone(key, *picks));
                    }
                    keep
                });
            }
            self.picks.retain(|_, ids| !ids.is_empty());
        }
        if self.learned.len() > MAX_QUERIES {
            let mut times: Vec<u64> = self.learned.values().map(|l| l.at).collect();
            let cut = self.learned.len() - MAX_QUERIES;
            times.select_nth_unstable(cut);
            let floor = times[cut];
            self.learned.retain(|query, learned| {
                let keep = learned.at >= floor;
                if !keep {
                    change
                        .learned
                        .push(Replaced::gone(query.clone(), learned.clone()));
                }
                keep
            });
        }
    }
}

/// What one change to the History replaced, so it can be undone for a short
/// while: each entry it touched, as it was before and after.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    picks: Vec<Replaced<ResultKey, Picks>>,
    /// By query, as [`query_key`] gives it.
    learned: Vec<Replaced<String, Learned>>,
}

/// What the History kept under one key before and after a change; `None`
/// when it kept nothing.
#[derive(Clone, Debug, PartialEq)]
struct Replaced<K, V> {
    key: K,
    before: Option<V>,
    after: Option<V>,
}

impl<K, V> Replaced<K, V> {
    /// What a change removed.
    fn gone(key: K, before: V) -> Self {
        Replaced {
            key,
            before: Some(before),
            after: None,
        }
    }
}

impl Change {
    /// Whether it changed nothing.
    pub fn is_empty(&self) -> bool {
        self.picks.is_empty() && self.learned.is_empty()
    }

    /// Adds a change made after this one, so undoing undoes both.
    pub fn and(&mut self, later: Change) {
        self.picks.extend(later.picks);
        self.learned.extend(later.learned);
    }
}

/// Where the Daemon keeps its History, and its Usage log.
pub trait Store {
    /// Called after every change. Must not block.
    fn save(&mut self, history: &History);
    /// Adds an entry to the Usage log. Called only while the config turns the
    /// log on. Must not block.
    fn log(&mut self, entry: Entry);
    /// Forgets the Usage log, whether or not the config turns it on: all of
    /// it, or the picks of one Extension's Results. Must not block.
    fn forget_usage(&mut self, extension: Option<&str>);
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

    fn forget_usage(&mut self, extension: Option<&str>) {
        let extension = extension.map(str::to_owned);
        match &self.usage_log {
            Some(writer) => writer.forget(extension),
            // No thread is kept for a log that is off.
            None => usage::forget(self.usage.clone(), extension),
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

    /// 2,000 Results, the most the History keeps, picked one to seven
    /// times each over two months at all times of day, some for a query.
    fn picked_2000() -> History {
        let mut history = History::default();
        for n in 0..2000u64 {
            for k in 0..1 + n % 7 {
                let day = (n * 13 + k * 5) % 60;
                let hour = ((n * 7 + k * 11) % 24) as u32 * HOUR + (n % 3600) as u32;
                let id = format!("item-{n}");
                if n % 10 == 0 {
                    history.pick("apps", &id, &format!("q{}", n % 50), &[], when(day, hour));
                } else {
                    history.touch("apps", &id, when(day, hour));
                }
            }
        }
        history
    }

    /// Every Result, by its Frecency and Habit at `when`, then the Learned
    /// choices for a few queries, each with what it counts.
    fn ranked(history: &History, when: When) -> Vec<String> {
        let half_life = history.settings.half_life_secs;
        let mut results: Vec<_> = history
            .picked()
            .map(|(_, id, picks)| {
                let habit = picks.habit(when, half_life);
                let frecency = picks.frecency(when, &history.settings);
                (frecency, habit.near_now, habit.overall, id.to_owned())
            })
            .collect();
        results.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(b.1.total_cmp(&a.1))
                .then(a.3.cmp(&b.3))
        });
        let learned = (0..50)
            .flat_map(|n| history.learned(&format!("q{n}"), when.at))
            .map(|choice| format!("{}/{} {}", choice.extension, choice.id, choice.strength));
        results
            .into_iter()
            .map(|(frecency, near_now, overall, id)| {
                format!("{id} {frecency} {near_now} {overall}")
            })
            .chain(learned)
            .collect()
    }

    #[test]
    fn a_saved_and_reloaded_history_ranks_identically() {
        let history = picked_2000();
        let json = serde_json::to_string(&history).unwrap();
        let reloaded: History = serde_json::from_str(&json).unwrap();
        for at in [when(60, 9 * HOUR), when(75, 22 * HOUR + 1234)] {
            assert_eq!(ranked(&reloaded, at), ranked(&history, at));
        }
    }

    #[test]
    fn a_history_kept_with_full_precision_scores_loads() {
        let history: History = serde_json::from_str(
            r#"{"picks":{"apps":{"firefox":{"score":481.8121234736919,"at":1792368000}}},
            "learned":{"fi":{"choice":["apps","firefox"],"strength":3.141592653589793,
            "contenders":[],"at":1792368000}}}"#,
        )
        .unwrap();
        let at = when(0, 0);
        assert_eq!(
            history.picks("apps", "firefox").unwrap().count(at.at, DAY),
            481.8121337890625
        );
        assert_eq!(history.learned("fi", at.at)[0].strength, 3.1415927410125732);
    }

    #[test]
    fn decayed_together_picks_count_as_each_on_its_own() {
        let history = picked_2000();
        let picks: Vec<Picks> = history.picked().map(|(.., picks)| picks).collect();
        let settings = history.settings;
        let frecent = Frecent::new(&picks, settings.half_life_secs);
        let (mut habits, mut frecency) = (Vec::new(), Vec::new());
        // Also before some of the picks: a clock set back.
        for at in [when(30, 3 * HOUR), when(60, 9 * HOUR), when(400, 17 * HOUR)] {
            frecent.habits(at, &mut habits);
            frecent.frecency(at, settings.time_of_day, &mut frecency);
            for (n, picks) in picks.iter().enumerate() {
                let alone = picks.habit(at, settings.half_life_secs);
                let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
                assert!(close(habits[n].overall, alone.overall), "{n}");
                assert!(close(habits[n].near_now, alone.near_now), "{n}");
                assert!(close(frecency[n], picks.frecency(at, &settings)), "{n}");
                assert_eq!(frecent.picks(n), *picks);
            }
        }
    }

    #[test]
    fn a_result_takes_about_a_hundred_bytes_however_often_it_is_picked() {
        let mut history = History::default();
        for day in 0..100 {
            for hour in 0..24 {
                history.touch("apps", "firefox", when(day, hour * HOUR + 17 * 60));
            }
        }
        let json = serde_json::to_string(&history).unwrap();
        let entry = json.find("\"firefox\"").map(|at| &json[at..]).unwrap();
        let entry = &entry[..=entry.find('}').unwrap()];
        assert!(entry.len() <= 110, "{entry}");
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
        assert!(near > 0.8, "two hours later: {near}");
        assert_eq!(counts(&history, "mail", when(5, 19 * HOUR)), 0.5, "away");
    }

    #[test]
    fn habits_four_hours_apart_each_count_most_at_their_own_time() {
        let mut history = History::default();
        for day in 0..5 {
            history.touch("apps", "mail", when(day, 8 * HOUR));
            history.touch("apps", "news", when(day, 12 * HOUR));
        }
        assert!(counts(&history, "news", when(5, 8 * HOUR)) < 0.7);
        assert!(counts(&history, "mail", when(5, 12 * HOUR)) < 0.7);
    }

    #[test]
    fn a_result_picked_hundreds_of_times_a_day_still_learns_a_new_time_of_day() {
        let mut history = History::default();
        for day in 0..60 {
            for minute in 0..300 {
                history.touch("apps", "mail", when(day, 8 * HOUR + minute));
            }
        }
        let before = counts(&history, "mail", when(60, 20 * HOUR));
        for day in 60..74 {
            for minute in 0..300 {
                history.touch("apps", "mail", when(day, 20 * HOUR + minute));
            }
        }
        let after = counts(&history, "mail", when(74, 20 * HOUR));
        assert_eq!(before, 0.5, "never picked in the evening");
        assert!(after > 0.9, "picked as much in the evening lately: {after}");
    }

    fn key(extension: &str, id: &str) -> ResultKey {
        (extension.to_owned(), id.to_owned())
    }

    /// Firefox picked through its Identity three times for `fi`, over Files,
    /// and Fish three times for `sh`.
    fn used() -> History {
        let mut history = History::default();
        let above = [key("apps", "files")];
        for day in 0..3 {
            history.pick(IDENTITIES, "app:firefox", "fi", &above, when(day, 9 * HOUR));
            history.touch("apps", "firefox", when(day, 9 * HOUR));
            history.pick("apps", "fish", "sh", &[], when(day, 10 * HOUR));
        }
        history
    }

    /// The History as it is kept, which undoing must restore.
    fn kept(history: &History) -> serde_json::Value {
        serde_json::to_value(history).unwrap()
    }

    fn firefox() -> [ResultKey; 2] {
        [key(IDENTITIES, "app:firefox"), key("apps", "firefox")]
    }

    #[test]
    fn forgetting_a_result_drops_its_picks_and_the_learned_choices_for_it() {
        let mut history = used();
        let at = when(3, 0).at;
        assert!(!history.learned("fi", at).is_empty());
        history.forget(&firefox());
        assert!(history.picks(IDENTITIES, "app:firefox").is_none());
        assert!(history.picks("apps", "firefox").is_none());
        assert!(history.learned("fi", at).is_empty());
        assert!(history.picks("apps", "fish").is_some(), "the rest stays");
        assert_eq!(history.learned("sh", at)[0].id, "fish");
    }

    #[test]
    fn a_result_picked_towards_a_learned_choice_is_forgotten_from_it_too() {
        let mut history = History::default();
        history.pick("apps", "fish", "fi", &[], when(0, 9 * HOUR));
        history.forget(&[key("apps", "fish")]);
        assert!(history.is_empty(), "{history:?}");
    }

    #[test]
    fn undoing_a_forget_restores_the_history_exactly() {
        let mut history = used();
        let before = kept(&history);
        let change = history.forget(&firefox());
        history.undo(change);
        assert_eq!(kept(&history), before);
    }

    #[test]
    fn undoing_a_clear_restores_the_history_exactly() {
        let mut history = used();
        let before = kept(&history);
        let change = history.clear();
        assert!(history.is_empty());
        history.undo(change);
        assert_eq!(kept(&history), before);
    }

    #[test]
    fn undoing_a_pick_restores_the_history_exactly() {
        let mut history = used();
        let before = kept(&history);
        let above = [key("apps", "files")];
        let mut change = history.pick(IDENTITIES, "app:firefox", "fi", &above, when(4, 9 * HOUR));
        change.and(history.touch("apps", "firefox", when(4, 9 * HOUR)));
        history.undo(change);
        assert_eq!(kept(&history), before);

        let change = history.pick("apps", "mail", "ma", &[], when(4, 9 * HOUR));
        history.undo(change);
        assert_eq!(kept(&history), before, "a Result picked for the first time");
    }

    #[test]
    fn undoing_a_pick_puts_back_what_the_caps_trimmed_for_it() {
        let mut history = History::default();
        for n in 0..MAX_RESULTS {
            // A second apart, so one is the weakest.
            history.touch("apps", &n.to_string(), when(0, n as u32));
        }
        let before = kept(&history);
        let change = history.touch("apps", "new", when(1, 9 * HOUR));
        assert_eq!(history.picked().count(), MAX_RESULTS);
        history.undo(change);
        assert_eq!(kept(&history), before);
    }

    #[test]
    fn undo_leaves_alone_what_changed_since() {
        let mut history = used();
        let change = history.clear();
        history.touch("apps", "mail", when(4, 9 * HOUR));
        history.touch("apps", "fish", when(4, 9 * HOUR));
        let fish = *history.picks("apps", "fish").unwrap();
        history.undo(change);
        assert!(history.picks("apps", "mail").is_some(), "picked since");
        assert_eq!(
            history.picks("apps", "fish"),
            Some(&fish),
            "picked again since"
        );
        assert!(
            history.picks(IDENTITIES, "app:firefox").is_some(),
            "put back"
        );
        assert_eq!(history.learned("fi", when(4, 0).at)[0].id, "app:firefox");
    }

    #[test]
    fn forgetting_what_one_extension_offered_keeps_the_rest() {
        let mut history = used();
        history.touch("files", "notes", when(0, 9 * HOUR));
        let before = kept(&history);
        let change = history.forget_where(|(extension, id)| {
            extension == "apps" || (extension == IDENTITIES && id == "app:firefox")
        });
        let at = when(3, 0).at;
        assert!(history.learned("fi", at).is_empty());
        assert!(history.learned("sh", at).is_empty());
        let left: Vec<_> = history.picked().map(|(e, i, _)| (e, i)).collect();
        assert_eq!(left, [("files", "notes")]);
        history.undo(change);
        assert_eq!(kept(&history), before);
    }

    /// How much an app's picks count at `when`, of their decayed number.
    fn counts(history: &History, id: &str, when: When) -> f64 {
        let picks = history.picks("apps", id).unwrap();
        let count = picks.count(when.at, history.settings().half_life_secs);
        history.frecency("apps", id, when) / count
    }
}
