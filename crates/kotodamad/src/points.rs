//! A Result's points: how well it matches and how much the user picks it,
//! blended into one number, with the bounds that keep the order's promises
//! whatever the weights.
//!
//! A Tier is worth [`TIER`] points. Habit adds points along a curve that
//! grows with the evidence: a Frecency of `lift_threshold` is worth one Tier,
//! and each further Tier takes `1 + lift_threshold` times as much. An
//! Extension's boost multiplies the sum, squeezed into the Tier's band so
//! that the most used Results keep their order below its ceiling. The
//! bounds then hold:
//!
//! - A near miss stays below every real match, and a real match above every near miss.
//! - Only an exact match, or a word start (or initials) whose habit is worth a Tier, reaches the
//!   exact band. A match inside a word or scattered never passes an exact title on habit or boost;
//!   nor does a word start on boost alone.
//!
//! # Owns
//!
//! - [`TIER`], [`habit`], [`points`] (and its most, [`MAX`]) and [`band`].
//! - [`Boost`], an Extension's multiplier on its Results' points.
//!
//! # Not here
//!
//! - What ranks above the points (answers, fallbacks, Aliases and Learned choices) and what breaks
//!   a tie between equal points: `daemon.rs`, `Candidate::rank_key`.
//! - Frecency itself: `history.rs`.
//! - Reading an Extension's boost from the config: `apps.rs`.

use crate::rank::Tier;

/// The points a Tier is worth. A habit point is a 4,096th of a Tier: fine
/// enough that a pick made years ago still counts.
pub const TIER: u32 = 1 << 12;

/// The points of the exact band's floor: an exact title or keyword with
/// no habit.
const EXACT: u32 = TIER * Tier::Exact.coarse() as u32;

/// An Extension's boost when its `[extensions.<name>]` sets none: its
/// Results' points count as earned.
pub const DEFAULT_BOOST: f64 = 1.0;

/// How much an Extension's Results' points count, from its
/// `[extensions.<name>]` table's `boost`: a number above 0, finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boost(f64);

// `Boost::new` takes no NaN, so equality is an equivalence.
impl Eq for Boost {}

impl Default for Boost {
    fn default() -> Self {
        Boost(DEFAULT_BOOST)
    }
}

impl Boost {
    /// `boost`, if it is finite and above 0.
    #[must_use]
    pub fn new(boost: f64) -> Option<Self> {
        (boost.is_finite() && boost > 0.0).then_some(Boost(boost))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// The points a Frecency of `frecency` adds: one Tier at `lift_threshold`,
/// two at `(1 + lift_threshold)² - 1`, and so on, so each Tier takes more
/// evidence than the last. Any pick is worth at least a point, so it beats
/// no pick on an otherwise equal match.
pub fn habit(frecency: f32, lift_threshold: f64) -> u32 {
    if frecency <= 0.0 {
        return 0;
    }
    let tiers = f64::from(frecency).ln_1p() / lift_threshold.ln_1p();
    ((tiers * f64::from(TIER)) as u32).max(1)
}

/// The points of a Result that matches at `tier` with `habit` points (see
/// [`habit`]), from an Extension boosted by `boost`, held to the bounds the
/// module describes.
pub fn points(tier: Tier, habit: u32, boost: Boost) -> u32 {
    let earned = (u32::from(tier.coarse()) * TIER).saturating_add(habit);
    // Exact at a boost of 1: every `u32` is an `f64`.
    let boosted = f64::from(earned) * boost.get();
    let (floor, ceiling, room) = match tier {
        Tier::NearMiss => (0, TIER - 1, ROOM),
        Tier::Scattered | Tier::Inside => (TIER, EXACT - 1, ROOM),
        Tier::WordStart | Tier::Initials if habit < TIER => (TIER, EXACT - 1, ROOM),
        Tier::WordStart | Tier::Initials | Tier::Keyword | Tier::Exact => (TIER, MAX, TOP_ROOM),
    };
    (squeeze(boosted, ceiling, room) as u32).clamp(floor, ceiling)
}

/// The most points a Result has: what the rank key holds.
pub const MAX: u32 = (1 << 26) - 1;

/// The points below a band's ceiling that hold every Result past them,
/// squeezed: half a Tier, so habit that lifted a match to a word start
/// doesn't take it further.
const ROOM: u32 = TIER / 2;

/// [`ROOM`] for the top band: 4,096 Tiers, so a boost of 1,000 is still
/// counted as earned.
const TOP_ROOM: u32 = 1 << 24;

/// `points`, or, past `ceiling - room`, squeezed into the room left
/// below `ceiling`, rising ever more slowly towards it. The most used
/// Results still order by their use, as far as a point tells them apart,
/// rather than tie at the ceiling.
fn squeeze(points: f64, ceiling: u32, room: u32) -> f64 {
    let knee = f64::from(ceiling - room);
    if points <= knee {
        return points;
    }
    let room = f64::from(room);
    f64::from(ceiling) - room * room / (room + (points - knee))
}

/// The Tier band `points` reach, as [`Tier::coarse`] numbers them: a word
/// start used enough ranks in the exact band, with the exact matches.
pub fn band(points: u32) -> u8 {
    (points / TIER).min(u32::from(Tier::Exact.coarse())) as u8
}

#[cfg(test)]
mod tests;
