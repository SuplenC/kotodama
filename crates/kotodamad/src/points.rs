//! A Result's points: how well it matches and how much the user picks it,
//! blended into one number, with the bounds that keep the order's promises
//! whatever the weights.
//!
//! A Tier is worth [`TIER`] points. Habit adds points along a curve that
//! grows with the evidence: a Frecency of `lift_threshold` is worth one Tier,
//! and each further Tier takes `1 + lift_threshold` times as much. An
//! Extension's boost multiplies the sum. The bounds then hold:
//!
//! - A near miss stays below every real match, and a real match above every near miss.
//! - Only an exact match, or a word start (or initials) whose habit is worth a Tier, reaches the
//!   exact band. A match inside a word or scattered never passes an exact title on habit or boost;
//!   nor does a word start on boost alone.
//!
//! # Owns
//!
//! - [`TIER`], [`habit`], [`points`] and [`band`].
//!
//! # Not here
//!
//! - What ranks above the points (answers, fallbacks, Aliases and Learned choices) and what breaks
//!   a tie between equal points: `daemon.rs`, `Candidate::rank_key`.
//! - Frecency itself: `history.rs`.

use crate::rank::Tier;

/// The points a Tier is worth. A habit point is a 4,096th of a Tier: fine
/// enough that a pick made years ago still counts.
pub const TIER: u32 = 1 << 12;

/// The points of the exact band's floor: an exact title or keyword with
/// no habit.
const EXACT: u32 = TIER * Tier::Exact.coarse() as u32;

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
pub fn points(tier: Tier, habit: u32, boost: f64) -> u32 {
    let earned = (u32::from(tier.coarse()) * TIER).saturating_add(habit);
    // Exact at a boost of 1: every `u32` is an `f64`.
    let boosted = (f64::from(earned) * boost) as u32;
    let (floor, ceiling) = match tier {
        Tier::NearMiss => (0, TIER - 1),
        Tier::Scattered | Tier::Inside => (TIER, EXACT - 1),
        Tier::WordStart | Tier::Initials if habit < TIER => (TIER, EXACT - 1),
        Tier::WordStart | Tier::Initials | Tier::Keyword | Tier::Exact => (TIER, u32::MAX),
    };
    boosted.clamp(floor, ceiling)
}

/// The Tier band `points` reach, as [`Tier::coarse`] numbers them: a word
/// start used enough ranks in the exact band, with the exact matches.
pub fn band(points: u32) -> u8 {
    (points / TIER).min(u32::from(Tier::Exact.coarse())) as u8
}

#[cfg(test)]
mod tests;
