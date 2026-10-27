//! What the user picked, which Frecency and Learned choices rank by. It is
//! kept in `$XDG_STATE_HOME/kotodama/history.json` and written after every
//! pick, on a thread of its own so the Daemon never waits for the disk.
//!
//! Times are passed in by the caller, as [`When`], so the History is the same
//! in tests as on a real clock.
//!
//! # Owns
//!
//! - [`History`]: picks, Learned choices and Frecency, and its [`Settings`].
//! - [`Change`]: one change to it, and undoing it.
//! - [`Store`] and [`Disk`]: saving it off the Daemon's thread.
//!
//! # Not here
//!
//! - The Usage log: `usage.rs`.
//! - Ranking by what it learned: `daemon/search.rs`, `most_used.rs`.

use std::collections::HashMap;
use std::fs;
use std::hash::BuildHasherDefault;
use std::hash::DefaultHasher;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use kotodama_protocol::Table;
use kotodama_protocol::Value;
use serde::Deserialize;
use serde::Serialize;

use crate::clock::DAY_SECS;
use crate::clock::When;
use crate::host::spawn;
use crate::problem::Report;
use crate::usage;
use crate::usage::Entry;

const DAY: f64 = DAY_SECS as f64;

/// How many of the Results ranked above a Learned choice when it was picked
/// are remembered as passed over.
const PASSED_OVER: usize = 8;
/// How many Results per query are counted towards becoming its Learned choice.
const CONTENDERS: usize = 4;
/// The most Results and queries kept unless the config says otherwise; past
/// these the least recent go. 2,000 Results is years of picks for most users
/// and keeps the History file small enough to read at start in a millisecond.
pub const DEFAULT_MAX_RESULTS: usize = 2000;
pub const DEFAULT_MAX_QUERIES: usize = 1000;
/// How long after a change to the History it can be undone, unless the config
/// says otherwise: long enough to notice what Forget took.
pub const DEFAULT_UNDO_SECS: u64 = 60;

/// How the History learns and how much it counts, from the config's
/// `[history]` table:
///
/// ```toml
/// [history]
/// picks_to_learn = 3      # picks of one Result for one query that make it the query's Learned choice
/// picks_to_take_over = 2  # picks of another Result, since the choice was last picked, that replace it
/// half_life_days = 14     # how long a pick takes to count half as much
/// lift_threshold = 5.0    # the Frecency worth one Tier: about a week of daily use
/// time_of_day = 0.5       # how much less a pick counts at a time of day it is seldom made: 0 to 1
/// day_of_week = 0.3       # how much less a pick counts on a weekday or a weekend it is seldom made on: 0 to 1
/// usage_log = false       # log queries and picks, to tune ranking from real use
/// undo_seconds = 60       # how long Forget and Clear history can be undone
/// max_results = 2000      # the most Results kept; past it the least used go
/// max_queries = 1000      # the most queries whose Learned choice is kept
/// ```
///
/// A setting that doesn't parse is a problem, and keeps its default.
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
    /// The Frecency worth one Tier of how well a Result matches; each
    /// further Tier takes `1 + lift_threshold` times as much (see
    /// `crate::points`). The default, about a week of daily use, lets
    /// something used daily whose title starts with the query beat an exact
    /// title, but not a few picks.
    pub lift_threshold: f64,
    /// How much less a Result's picks count at a time of day it is seldom
    /// picked: 0 not at all, 1 for nothing at a time it is never picked. At
    /// the times it is picked most they count in full.
    pub time_of_day: f64,
    /// How much less a Result's picks count on the kind of day, a weekday
    /// or a weekend, it is seldom picked on: 0 not at all, 1 for nothing on
    /// a kind of day it is never picked on. On the kind it is picked most
    /// on, by the day, they count in full. Below `time_of_day` by default:
    /// two kinds of day are told apart from fewer picks than eight parts of
    /// a day are, so the kind of day is the weaker evidence.
    pub day_of_week: f64,
    /// Whether queries and picks go in the Usage log (see [`crate::usage`]).
    pub usage_log: bool,
    /// How long after a change to the History it can be undone.
    pub undo_secs: u64,
    /// The most Results kept; past it the least used go.
    pub max_results: usize,
    /// The most queries whose Learned choice is kept; past it the least
    /// recent go.
    pub max_queries: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            picks_to_learn: 3,
            picks_to_take_over: 2,
            half_life_secs: 14.0 * DAY,
            lift_threshold: 5.0,
            time_of_day: 0.5,
            day_of_week: 0.3,
            usage_log: false,
            undo_secs: DEFAULT_UNDO_SECS,
            max_results: DEFAULT_MAX_RESULTS,
            max_queries: DEFAULT_MAX_QUERIES,
        }
    }
}

impl Settings {
    /// How long after a change to the History it can be undone.
    pub fn undo_for(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.undo_secs)
    }

    /// The `[history]` table of the merged config. A setting that can't be
    /// read goes in `report`.
    pub fn from_config(config: &Table, report: &mut Report) -> Self {
        let mut settings = Settings::default();
        let table = match config.get("history") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                report.add("history", "should be a table");
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
                "day_of_week" => number(value)
                    .filter(|weight| (0.0..=1.0).contains(weight))
                    .ok_or("should be a number from 0 to 1")
                    .map(|weight| settings.day_of_week = weight),
                "usage_log" => match value {
                    Value::Bool(on) => {
                        settings.usage_log = *on;
                        Ok(())
                    }
                    _ => Err("should be true or false"),
                },
                "undo_seconds" => count(value, 0, 24 * 60 * 60)
                    .ok_or("should be a number of seconds from 0 to 86400")
                    .map(|secs| settings.undo_secs = secs),
                "max_results" => count(value, 1, 1_000_000)
                    .ok_or("should be a number of Results from 1 to 1000000")
                    .map(|n| settings.max_results = n as usize),
                "max_queries" => count(value, 1, 1_000_000)
                    .ok_or("should be a number of queries from 1 to 1000000")
                    .map(|n| settings.max_queries = n as usize),
                _ => Err("isn't a setting"),
            };
            if let Err(err) = set {
                report.add(format!("history.{key}"), err);
            }
        }
        settings
    }
}

/// A whole number from `min` to `max`.
fn count(value: &Value, min: u64, max: u64) -> Option<u64> {
    match value {
        Value::Integer(n) => u64::try_from(*n).ok().filter(|n| (min..=max).contains(n)),
        _ => None,
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

/// A map by name that hashes with fixed keys rather than a random seed per
/// process: `SipHash` as fast as `HashMap`'s own, but a lookup runs the same
/// instructions in every Daemon, so the empty query's instruction count is
/// the same from run to run.
type ByName<V> = HashMap<String, V, BuildHasherDefault<DefaultHasher>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// By Extension name, then Item id; or by [`IDENTITIES`], then Identity.
    /// An Item with an Identity is kept both ways: its Identity's picks rank
    /// it, and its own put it ahead of the same thing from another Extension.
    #[serde(default)]
    picks: ByName<ByName<Picks>>,
    /// By query, as [`query_key`] gives it.
    #[serde(default)]
    learned: ByName<Learned>,
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

/// The two kinds of day: the weekdays, Monday to Friday, and the weekend.
const KINDS: usize = 2;
const WEEKDAYS: usize = 0;
const WEEKENDS: usize = 1;
/// How many days of the week each kind of day has.
const DAYS_OF: [f64; KINDS] = [5.0, 2.0];
/// The time of the week, in seconds from Monday's midnight, the weekend
/// starts at, and ends at.
const WEEKEND_FROM: f64 = 5.0 * DAY;
const WEEK: f64 = 7.0 * DAY;

/// How often, how recently, at what times of day and on which kind of day
/// one Result was picked.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Kept")]
pub struct Picks {
    /// The decayed number of picks, as of `at`. An `f32`, so `history.json`
    /// keeps its shortest form: ranking needs no more digits.
    score: f32,
    at: u64,
    /// How the picks made on weekdays fall across the day: in [`PARTS`]
    /// parts from midnight, each as its share of `score`, out of [`SHARES`]
    /// together with `weekends`. Both all zero when it isn't known, for
    /// picks kept before it was.
    #[serde(default, skip_serializing_if = "unknown")]
    weekdays: [u16; PARTS],
    /// The same for the picks made on weekends.
    #[serde(default, skip_serializing_if = "unknown")]
    weekends: [u16; PARTS],
}

/// [`Picks`] as a History file keeps them, which may be older than the
/// kinds of day: then `parts` holds the shares of all its picks.
#[derive(Deserialize)]
struct Kept {
    score: f32,
    at: u64,
    #[serde(default)]
    parts: [u16; PARTS],
    #[serde(default)]
    weekdays: [u16; PARTS],
    #[serde(default)]
    weekends: [u16; PARTS],
}

impl From<Kept> for Picks {
    fn from(kept: Kept) -> Self {
        let mut picks = Picks {
            score: kept.score,
            at: kept.at,
            weekdays: kept.weekdays,
            weekends: kept.weekends,
        };
        if unknown(&picks.weekdays) && unknown(&picks.weekends) {
            // Picks kept before the kinds of day were count in full on
            // either, as picks made at the same rate every day of the week
            // do: five parts in seven on weekdays, two on weekends. Scaled
            // by 5 and 2 rather than divided by 7, so the rates are equal
            // exactly; the shares stand in proportion only, and the next pick
            // brings them back to `SHARES`.
            picks.weekdays = kept.parts.map(|share| share.saturating_mul(DAYS_OF[WEEKDAYS] as u16));
            picks.weekends = kept.parts.map(|share| share.saturating_mul(DAYS_OF[WEEKENDS] as u16));
        }
        picks
    }
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

/// How much of a weekend `when` is, from 0 on a weekday to 1 on a Saturday
/// or Sunday, fading across each midnight the weekend starts or ends at over
/// as long as a time fades between two parts of the day. A pick counts
/// towards both kinds of day by it and a time reads both, so Friday night
/// and Saturday morning fade into each other rather than cut.
fn weekend(when: When) -> f64 {
    let time_of_week = f64::from(when.day_of_week) * DAY + f64::from(when.time_of_day);
    // The weekend ends at Monday's midnight, where the week starts again.
    fade(time_of_week - WEEKEND_FROM) * (1.0 - fade(time_of_week - WEEK)) + (1.0 - fade(time_of_week))
}

/// From 0 half a part of the day before a boundary to 1 half a part after.
fn fade(past_boundary: f64) -> f64 {
    (past_boundary / PART_SECS + 0.5).clamp(0.0, 1.0)
}

/// How near `when` is to each kind of day, from 0 to 1, as [`weekend`]
/// has it.
fn kinds(when: When) -> [f64; KINDS] {
    let weekend = weekend(when);
    [1.0 - weekend, weekend]
}

/// The part of the day `when` falls in: when it started, in seconds since
/// the Unix epoch, and which of the day's parts it is. The same for every
/// time in one part of one day, and different for another part, the same
/// part on another day, or the same moment in another time zone.
pub fn part_of_day(when: When) -> (u64, u32) {
    let part_secs = (DAY_SECS / PARTS as u64) as u32;
    let started = when.at.saturating_sub(u64::from(when.time_of_day % part_secs));
    (started, when.time_of_day / part_secs)
}

/// The moment the part of the day after the one `when` falls in starts, in
/// the time zone `when` was read in.
pub fn next_part(when: When) -> When {
    let part_secs = (DAY_SECS / PARTS as u64) as u32;
    let until = part_secs - when.time_of_day % part_secs;
    when.plus(i64::from(until))
}

/// How much a Result was picked, as the empty query ranks it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Habit {
    /// The decayed number of picks made near this time of day, on this kind
    /// of day.
    pub near_now: f64,
    /// The decayed number of all picks.
    pub overall: f64,
}

impl Habit {
    /// What two ways to one Result add up to.
    #[must_use]
    pub fn and(self, other: Habit) -> Habit {
        Habit {
            near_now: self.near_now + other.near_now,
            overall: self.overall + other.overall,
        }
    }

    /// The more used of the two, for each count.
    #[must_use]
    pub fn max(self, other: Habit) -> Habit {
        Habit {
            near_now: self.near_now.max(other.near_now),
            overall: self.overall.max(other.overall),
        }
    }

    /// Most used near the current time first, then most used overall.
    pub fn rank(&self, other: &Habit) -> std::cmp::Ordering {
        other
            .near_now
            .total_cmp(&self.near_now)
            .then(other.overall.total_cmp(&self.overall))
    }
}

/// The shares of one Result's picks, a kind of day at a time, as numbers
/// to work counts out from.
type Shares = [[f64; PARTS]; KINDS];

/// The share of the picks a kind of day has, per day of it, out of the
/// other kind's, from which it counts as usual as the other. The shares
/// lean towards the kind of day the last pick fell on, by up to a fifth at
/// the default half-life, since they decay with the picks; this keeps a
/// Result used every day counting in full on both kinds all week.
const AS_USUAL: f64 = 0.75;

/// What ranking normalises its reads of one Result's shares by, as
/// [`Picks::usual`] works them out and [`Frecent`] keeps them: `f32`s, as
/// ranking needs no more digits.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Usual {
    /// How usual each kind of day is for picking it: 1 on the kind it is
    /// picked most on, by the day, or near it, down to 0 on a kind it never
    /// is; both 1 when the shares aren't known.
    kinds: [f32; KINDS],
    /// Its largest share of a part of the day, both kinds together: 0 when
    /// the shares aren't known.
    most: f32,
}

impl Picks {
    fn new(at: u64) -> Self {
        Picks {
            score: 0.0,
            at,
            weekdays: [0; PARTS],
            weekends: [0; PARTS],
        }
    }

    /// The decayed number of picks at time `at`: every pick counts 1 when
    /// made, and half as much every `half_life` seconds after.
    fn count(&self, at: u64, half_life: f64) -> f64 {
        decay(f64::from(self.score), self.at, at, half_life)
    }

    /// The Frecency at `when`: the decayed number of picks, which count less
    /// at a time of day and on a kind of day the Result is seldom picked, as
    /// `settings` say.
    pub fn frecency(&self, when: When, settings: &Settings) -> f64 {
        let count = self.count(when.at, settings.half_life_secs);
        let usual = self.usual();
        count
            * (1.0 - settings.time_of_day * (1.0 - self.usual_time(when.time_of_day, usual.most.into())))
            * (1.0 - settings.day_of_week * (1.0 - usual_kind(when, usual.kinds)))
    }

    /// How much it was picked as of `when`, near that time of day on that
    /// kind of day, and overall.
    pub fn habit(&self, when: When, half_life: f64) -> Habit {
        let overall = self.count(when.at, half_life);
        let usual = self.usual();
        Habit {
            near_now: overall * self.usual_time(when.time_of_day, usual.most.into()) * usual_kind(when, usual.kinds),
            overall,
        }
    }

    /// The shares, a kind of day at a time, as numbers.
    fn shares(&self) -> Shares {
        [self.weekdays.map(f64::from), self.weekends.map(f64::from)]
    }

    /// The shares of each part of the day, both kinds of day together.
    fn parts(&self) -> [f64; PARTS] {
        std::array::from_fn(|part| f64::from(self.weekdays[part]) + f64::from(self.weekends[part]))
    }

    /// What ranking normalises its reads by.
    fn usual(&self) -> Usual {
        let shares = self.shares();
        // The share of each kind of day per day of it: a Result picked at
        // the same rate every day has five parts in seven on weekdays.
        let rates: [f64; KINDS] = std::array::from_fn(|kind| shares[kind].iter().sum::<f64>() / DAYS_OF[kind]);
        let as_usual = rates[WEEKDAYS].max(rates[WEEKENDS]) * AS_USUAL;
        Usual {
            kinds: if as_usual > 0.0 {
                rates.map(|rate| (rate / as_usual).min(1.0) as f32)
            } else {
                [1.0; KINDS]
            },
            most: self.parts().into_iter().fold(0.0, f64::max) as f32,
        }
    }

    /// How usual a time of day is for picking it: 1 at the times it is picked
    /// most, or when that isn't known, down to 0 at times it never is. `most`
    /// is the largest share of a part of the day, as [`usual`](Self::usual)
    /// has it.
    fn usual_time(&self, time_of_day: u32, most: f64) -> f64 {
        if most == 0.0 {
            return 1.0;
        }
        let shares = self.parts();
        let parts = parts(time_of_day);
        let read: f64 = parts.iter().map(|&(part, near)| near * shares[part]).sum();
        // So picks read at the time of day they were made read in full.
        let whole: f64 = parts.iter().map(|&(_, near)| near * near).sum();
        (read / (whole * most)).min(1.0)
    }

    /// `shares`, out of [`SHARES`] together, as the counts they stand for
    /// of `count` picks. Picks made at times of day that aren't known count
    /// as spread evenly over the parts, and over the kinds of day by the
    /// days each has.
    fn counts(shares: Shares, count: f64) -> Shares {
        let total: f64 = shares.iter().flatten().sum();
        if total > 0.0 {
            return shares.map(|kind| kind.map(|share| count * share / total));
        }
        let days: f64 = DAYS_OF.iter().sum();
        std::array::from_fn(|kind| [count * DAYS_OF[kind] / days / PARTS as f64; PARTS])
    }

    /// `counts`, of `counted` picks together, as shares out of [`SHARES`].
    fn set_shares(&mut self, counts: Shares, counted: f64) {
        let shares = counts.map(|kind| kind.map(|count| (count / counted * SHARES).round() as u16));
        [self.weekdays, self.weekends] = shares;
    }

    /// Adds a pick made at `when`.
    fn add(&mut self, when: When, half_life: f64) {
        let before = self.count(when.at, half_life);
        let mut counts = Picks::counts(self.shares(), before);
        for (kind, near_kind) in kinds(when).into_iter().enumerate() {
            for (part, near) in parts(when.time_of_day) {
                counts[kind][part] += near_kind * near;
            }
        }
        let score = before + 1.0;
        self.score = score as f32;
        self.at = when.at;
        self.set_shares(counts, score);
    }

    /// Takes off one pick made at `when`, as [`add`](Self::add) counted it:
    /// its weight, decayed to the latest pick, comes off the score and off
    /// the parts of the day and the kinds of day it went to. `None` once
    /// nothing is left.
    fn take_off(mut self, when: When, half_life: f64) -> Option<Picks> {
        let score = f64::from(self.score);
        let weight = decay(1.0, when.at, self.at, half_life);
        let left = score - weight;
        if left <= 0.0 {
            return None;
        }
        let mut counts = Picks::counts(self.shares(), score);
        for (kind, near_kind) in kinds(when).into_iter().enumerate() {
            for (part, near) in parts(when.time_of_day) {
                counts[kind][part] = (counts[kind][part] - weight * near_kind * near).max(0.0);
            }
        }
        // Not `left`: a part the pick went to may have held less than it,
        // say after the picks of an older History were spread evenly.
        let counted: f64 = counts.iter().flatten().sum();
        self.score = left as f32;
        // With nothing counted in any part, when the picks left were made
        // isn't known.
        if counted > 0.0 {
            self.set_shares(counts, counted);
        } else {
            self.weekdays = [0; PARTS];
            self.weekends = [0; PARTS];
        }
        Some(self)
    }
}

/// How usual `when`'s kind of day is for picking a Result whose kinds of
/// day are usual as `kinds` says: read from both by how near `when` is to
/// each, so it is 1 on the kind picked most and fades at the boundary.
fn usual_kind(when: When, kinds: [f32; KINDS]) -> f64 {
    let [weekdays, weekends] = kinds.map(f64::from);
    weekdays + weekend(when) * (weekends - weekdays)
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
    /// Each one's share of each part of the day, both kinds of day
    /// together, a column a part. An `f32` holds the sum of two shares
    /// exactly, and converts to the loop's `f64` as cheaply as a `u16`.
    parts: [Vec<f32>; PARTS],
    /// Each one's largest share of a part, as [`Usual::most`] has it.
    most: Vec<f32>,
    /// How usual each kind of day is for picking each one, a column a kind,
    /// as [`Usual::kinds`] has it.
    kinds: [Vec<f32>; KINDS],
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
            for (column, share) in frecent.parts.iter_mut().zip(picks.parts()) {
                column.push(share as f32);
            }
            let usual = picks.usual();
            frecent.most.push(usual.most);
            for (column, usual) in frecent.kinds.iter_mut().zip(usual.kinds) {
                column.push(usual);
            }
        }
        frecent
    }

    /// The half-life it decays at.
    pub fn half_life(&self) -> f64 {
        self.half_life
    }

    /// Each one's Habit at `when`, as [`Picks::habit`] works it out, into
    /// `habits`.
    pub fn habits(&self, when: When, habits: &mut Vec<Habit>) {
        self.each(when, habits, |count, usual_time, usual_kind| Habit {
            near_now: count * usual_time * usual_kind,
            overall: count,
        });
    }

    /// Each one's Frecency at `when`, as [`Picks::frecency`] works it out
    /// with `settings`' `time_of_day` and `day_of_week`, into `frecency`.
    pub fn frecency(&self, when: When, settings: &Settings, frecency: &mut Vec<f64>) {
        let (time_of_day, day_of_week) = (settings.time_of_day, settings.day_of_week);
        self.each(when, frecency, |count, usual_time, usual_kind| {
            count * (1.0 - time_of_day * (1.0 - usual_time)) * (1.0 - day_of_week * (1.0 - usual_kind))
        });
    }

    /// `with` each one's decayed number of picks at `when`, how usual
    /// `when`'s time of day is for picking it, as [`Picks::usual_time`] has
    /// it, and how usual its kind of day is, as [`usual_kind`] has it, into
    /// `out`.
    #[inline]
    fn each<T: Copy + Default>(&self, when: When, out: &mut Vec<T>, with: impl Fn(f64, f64, f64) -> T) {
        // What a pick made at `latest` counts at `when`: more than 1 when is
        // before it, which `score` caps, as `decay` does.
        let elapsed = when.at as f64 - self.latest as f64;
        let factor = 0.5f64.powf(elapsed / self.half_life);
        let [(first, near_first), (second, near_second)] = parts(when.time_of_day);
        let whole = near_first * near_first + near_second * near_second;
        let weekend = weekend(when);
        let len = self.decayed.len();
        out.clear();
        out.resize(len, T::default());
        // Sliced to one length, so the loop reads them without bounds checks.
        let (out, decayed, score) = (&mut out[..len], &self.decayed[..len], &self.score[..len]);
        let (first, second, most) = (&self.parts[first][..len], &self.parts[second][..len], &self.most[..len]);
        let (weekdays, weekends) = (&self.kinds[WEEKDAYS][..len], &self.kinds[WEEKENDS][..len]);
        // HAZARD: vectorised (NEON `fmul`/`fdiv`/`fcmgt`, SSE2
        // `mulpd`/`divpd`): no branch, call or early exit in this loop. It is
        // the empty query's per-Result work: 25k instructions for 2,000
        // Results, where a `powf` each took 160k.
        for n in 0..len {
            let count = decayed[n] * factor;
            let count = if count < score[n] { count } else { score[n] };
            let read = near_first * f64::from(first[n]) + near_second * f64::from(second[n]);
            let usual_time = read / (whole * f64::from(most[n]));
            let usual_time = if most[n] == 0.0 || usual_time > 1.0 {
                1.0
            } else {
                usual_time
            };
            let (on_weekdays, on_weekends) = (f64::from(weekdays[n]), f64::from(weekends[n]));
            let usual_kind = on_weekdays + weekend * (on_weekends - on_weekdays);
            out[n] = with(count, usual_time, usual_kind);
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

impl Learned {
    fn empty(at: u64) -> Learned {
        Learned {
            choice: None,
            strength: 0.0,
            passed_over: Vec::new(),
            contenders: Vec::new(),
            at,
        }
    }

    /// Counts a pick of `picked` at `at`, with `above` the Results ranked
    /// above it, nearest first: towards the choice's strength, or towards
    /// `picked` learning or taking over, as [`History::pick`] describes.
    fn add(&mut self, picked: &ResultKey, above: &[ResultKey], at: u64, settings: &Settings) {
        let half_life = settings.half_life_secs;
        if self.choice.as_ref() == Some(picked) {
            self.strength = (decay(self.strength.into(), self.at, at, half_life) + 1.0) as f32;
            self.contenders.clear();
            for key in above.iter().rev() {
                self.passed_over.retain(|old| old != key);
                self.passed_over.insert(0, key.clone());
            }
            self.passed_over.truncate(PASSED_OVER);
        } else {
            let count = match self.contenders.iter().position(|(key, _)| key == picked) {
                Some(n) => self.contenders.remove(n).1 + 1,
                None => 1,
            };
            let needed = if self.choice.is_some() {
                settings.picks_to_take_over
            } else {
                settings.picks_to_learn
            };
            if count >= needed {
                self.choice = Some(picked.clone());
                self.strength = f32::from(count);
                self.passed_over = above.iter().take(PASSED_OVER).cloned().collect();
                self.contenders.clear();
            } else {
                self.strength = decay(self.strength.into(), self.at, at, half_life) as f32;
                self.contenders.truncate(CONTENDERS - 1);
                self.contenders.insert(0, (picked.clone(), count));
            }
        }
        self.at = at;
    }

    /// Withdraws one pick of `picked` made at `when` from this entry, which
    /// changed since the pick turned `before` into `after`. When that pick
    /// made `picked` the choice and it still is, `before` comes back with
    /// the picks made since counted on it again, so the choice it replaced
    /// is the choice again. Otherwise the pick comes off as
    /// [`take_off`](Self::take_off) takes it. `None` once nothing is left.
    fn withdraw(
        &self,
        before: Option<&Learned>,
        after: &Learned,
        picked: &ResultKey,
        when: When,
        settings: &Settings,
    ) -> Option<Learned> {
        let made_choice =
            after.choice.as_ref() == Some(picked) && before.and_then(|before| before.choice.as_ref()) != Some(picked);
        if !made_choice || self.choice.as_ref() != Some(picked) {
            return self.clone().take_off(picked, when, settings.half_life_secs);
        }
        let mut restored = before.cloned().unwrap_or_else(|| Learned::empty(after.at));
        // The picks of `picked` since, as its strength grew past the pick's.
        let grown = f64::from(self.strength) - decay(after.strength.into(), after.at, self.at, settings.half_life_secs);
        for _ in 0..grown.round().max(0.0) as u32 {
            restored.add(picked, &[], self.at, settings);
        }
        // Then the picks of others since `picked` was last picked, oldest
        // first.
        for (key, count) in self.contenders.iter().rev() {
            for _ in 0..*count {
                restored.add(key, &[], self.at, settings);
            }
        }
        if restored.choice == self.choice {
            restored.passed_over.clone_from(&self.passed_over);
        }
        (restored.choice.is_some() || !restored.contenders.is_empty()).then_some(restored)
    }

    /// Takes off one pick of `picked` made at `when`, as it counted towards
    /// this query's Learned choice: from the choice's strength, or from its
    /// count as a contender. A choice the pick made stays the choice. `None`
    /// once nothing is left.
    fn take_off(mut self, picked: &ResultKey, when: When, half_life: f64) -> Option<Learned> {
        if self.choice.as_ref() == Some(picked) {
            let weight = decay(1.0, when.at, self.at, half_life);
            self.strength = (f64::from(self.strength) - weight).max(0.0) as f32;
        } else if let Some(n) = self.contenders.iter().position(|(key, _)| key == picked) {
            self.contenders[n].1 -= 1;
            if self.contenders[n].1 == 0 {
                self.contenders.remove(n);
            }
        }
        (self.choice.is_some() || !self.contenders.is_empty()).then_some(self)
    }
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

    /// Changes how the History learns and counts from here on. What it kept
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
    pub fn pick(&mut self, extension: &str, id: &str, query: &str, above: &[ResultKey], when: When) -> Change {
        let mut change = self.bump(extension, id, when);
        let at = when.at;

        let key = query_key(query);
        if !key.is_empty() {
            let before = self.learned.get(&key).cloned();
            let learned = self.learned.entry(key.clone()).or_insert_with(|| Learned::empty(at));
            learned.add(&(extension.to_owned(), id.to_owned()), above, at, &self.settings);
            let after = Some(learned.clone());
            change.learned.push(Replaced { key, before, after });
            if let Some(pick) = &mut change.pick {
                pick.learned = Some((extension.to_owned(), id.to_owned()));
            }
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
            pick: Some(Pick { when, learned: None }),
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
        self.forget_where(|(extension, id)| keys.iter().any(|key| key.0 == extension && key.1 == id))
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
    /// changed again since. There, a change that records a pick takes that
    /// one pick off, so withdrawing the first of two launches keeps the
    /// second; any other change leaves the entry as it is.
    pub fn undo(&mut self, change: Change) {
        self.version += 1;
        let half_life = self.settings.half_life_secs;
        let pick = change.pick;
        for Replaced { key, before, after } in change.picks.into_iter().rev() {
            let now = self.picks(&key.0, &key.1).copied();
            if now == after {
                self.set_picks(key, before);
            } else if let (Some(pick), Some(_), Some(now)) = (&pick, after, now) {
                self.set_picks(key, now.take_off(pick.when, half_life));
            }
        }
        for Replaced {
            key: query,
            before,
            after,
        } in change.learned.into_iter().rev()
        {
            let now = self.learned.get(&query);
            if now == after.as_ref() {
                match before {
                    Some(learned) => self.learned.insert(query, learned),
                    None => self.learned.remove(&query),
                };
            } else if let (
                Some(Pick {
                    when,
                    learned: Some(picked),
                }),
                Some(after),
                Some(now),
            ) = (&pick, after, now)
            {
                match now.withdraw(before.as_ref(), &after, picked, *when, &self.settings) {
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

    /// Every query's Learned choice, by Extension name and Item id (or
    /// [`IDENTITIES`] and Identity), with how many picks it was learned
    /// from, decayed to `at`.
    pub fn choices(&self, at: u64) -> impl Iterator<Item = (&str, &str, f64)> {
        let half_life = self.settings.half_life_secs;
        self.learned.values().filter_map(move |learned| {
            let (extension, id) = learned.choice.as_ref()?;
            let strength = decay(learned.strength.into(), learned.at, at, half_life);
            Some((extension.as_str(), id.as_str(), strength))
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
                !key.is_empty() && (learned_for.starts_with(key) || key.starts_with(learned_for.as_str()))
            })
            .filter_map(|(learned_for, learned)| {
                let (extension, id) = learned.choice.as_ref()?;
                Some((
                    learned_for.len(),
                    Choice {
                        extension,
                        id,
                        strength: decay(learned.strength.into(), learned.at, at, self.settings.half_life_secs),
                        passed_over: &learned.passed_over,
                    },
                ))
            })
            .collect();
        choices.sort_by(|(a_len, a), (b_len, b)| b.strength.total_cmp(&a.strength).then(b_len.cmp(a_len)));
        choices.into_iter().map(|(_, choice)| choice).collect()
    }

    /// Forgets everything.
    pub fn clear(&mut self) -> Change {
        self.version += 1;
        let mut change = Change::default();
        for (extension, ids) in self.picks.drain() {
            for (id, picks) in ids {
                change.picks.push(Replaced::gone((extension.clone(), id), picks));
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
        let results: usize = self.picks.values().map(ByName::len).sum();
        let (max_results, max_queries) = (self.settings.max_results, self.settings.max_queries);
        if results > max_results {
            let at = self.picked().map(|(.., p)| p.at).max().unwrap_or(0);
            let half_life = self.settings.half_life_secs;
            let mut scores: Vec<f64> = self.picked().map(|(.., p)| p.count(at, half_life)).collect();
            let cut = results - max_results;
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
        if self.learned.len() > max_queries {
            let mut times: Vec<u64> = self.learned.values().map(|l| l.at).collect();
            let cut = self.learned.len() - max_queries;
            times.select_nth_unstable(cut);
            let floor = times[cut];
            self.learned.retain(|query, learned| {
                let keep = learned.at >= floor;
                if !keep {
                    change.learned.push(Replaced::gone(query.clone(), learned.clone()));
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
    /// The pick it records, for a change that records one.
    pick: Option<Pick>,
}

/// A pick a [`Change`] records, so undoing it can take that one pick off an
/// entry that changed since.
#[derive(Clone, Debug, PartialEq)]
struct Pick {
    when: When,
    /// The Result it counted towards the query's Learned choice for, if the
    /// query wasn't empty.
    learned: Option<ResultKey>,
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
        match (&mut self.pick, later.pick) {
            (Some(pick), Some(later)) if pick.learned.is_none() => pick.learned = later.learned,
            (pick @ None, later) => *pick = later,
            _ => {}
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
#[derive(Debug)]
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
#[expect(
    clippy::float_cmp,
    reason = "the History's counts are exact: a test checks the very number a pick decays to"
)]
mod tests;
