use super::*;

const THRESHOLD: f64 = 5.0;

const REAL: [Tier; 7] = [
    Tier::Scattered,
    Tier::WeakKeyword,
    Tier::Inside,
    Tier::WordStart,
    Tier::Initials,
    Tier::Keyword,
    Tier::Exact,
];

const BOOSTS: [f64; 7] = [0.01, 0.5, 1.0, 1.5, 2.0, 10.0, 1000.0];

fn boost(boost: f64) -> Boost {
    Boost::new(boost).unwrap()
}

const ONE: Boost = Boost(DEFAULT_BOOST);

const FRECENCIES: [f32; 9] = [0.0, 1e-30, 0.01, 1.0, 4.99, 5.0, 35.0, 1e4, f32::MAX];

#[test]
fn with_no_habit_and_no_boost_a_result_has_the_points_of_its_tier() {
    for tier in REAL {
        assert_eq!(points(tier, 0, ONE), u32::from(tier.coarse()) * TIER, "{tier:?}");
        assert_eq!(band(points(tier, 0, ONE)), tier.coarse(), "{tier:?}");
    }
}

#[test]
fn any_pick_is_worth_a_point_and_more_picks_more() {
    assert_eq!(habit(0.0, THRESHOLD), 0);
    assert!(habit(1e-30, THRESHOLD) >= 1);
    for pair in FRECENCIES.windows(2) {
        assert!(habit(pair[0], THRESHOLD) <= habit(pair[1], THRESHOLD), "{pair:?}");
    }
}

#[test]
fn a_frecency_of_the_lift_threshold_is_worth_one_tier_and_each_further_tier_takes_more() {
    assert!(habit(4.99, THRESHOLD) < TIER);
    assert_eq!(habit(5.0, THRESHOLD), TIER);
    // Two Tiers take (1 + 5)² - 1 = 35, three (1 + 5)³ - 1 = 215.
    assert!(habit(34.9, THRESHOLD) < 2 * TIER);
    assert!(habit(35.0, THRESHOLD) >= 2 * TIER - 1);
    assert!(habit(215.0, THRESHOLD) >= 3 * TIER - 1);
}

#[test]
fn a_near_miss_stays_below_every_real_match_whatever_its_habit_and_boost() {
    for frecency in FRECENCIES {
        for b in BOOSTS {
            let near_miss = points(Tier::NearMiss, habit(frecency, THRESHOLD), boost(b));
            for tier in REAL {
                for other in BOOSTS {
                    assert!(
                        near_miss < points(tier, 0, boost(other)),
                        "{frecency} {b} {tier:?} {other}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_match_inside_a_word_or_scattered_never_reaches_an_exact_title() {
    let exact = points(Tier::Exact, 0, ONE);
    for tier in [Tier::Scattered, Tier::WeakKeyword, Tier::Inside] {
        for frecency in FRECENCIES {
            for b in BOOSTS {
                let points = points(tier, habit(frecency, THRESHOLD), boost(b));
                assert!(points < exact, "{tier:?} {frecency} {b}");
                assert!(band(points) < Tier::Exact.coarse());
            }
        }
    }
}

#[test]
fn a_match_inside_a_weak_keyword_never_reaches_a_word_start() {
    let word_start = points(Tier::WordStart, 0, ONE);
    for frecency in FRECENCIES {
        for b in BOOSTS {
            let points = points(Tier::WeakKeyword, habit(frecency, THRESHOLD), boost(b));
            assert!(points < word_start, "{frecency} {b}");
            assert!(band(points) <= Tier::Inside.coarse());
        }
    }
}

#[test]
fn a_word_start_reaches_an_exact_title_by_habit_and_never_by_boost_alone() {
    let exact = points(Tier::Exact, 0, ONE);
    for tier in [Tier::WordStart, Tier::Initials] {
        for b in BOOSTS {
            assert!(points(tier, habit(4.99, THRESHOLD), boost(b)) < exact, "{tier:?} {b}");
        }
        assert!(points(tier, habit(5.0, THRESHOLD), ONE) >= exact, "{tier:?}");
        assert_eq!(band(points(tier, habit(5.0, THRESHOLD), ONE)), Tier::Exact.coarse());
    }
}

#[test]
fn habit_climbs_a_scattered_match_a_tier_at_a_time() {
    let at = |frecency| band(points(Tier::Scattered, habit(frecency, THRESHOLD), ONE));

    assert_eq!(at(0.0), Tier::Scattered.coarse());
    assert_eq!(at(5.0), Tier::Inside.coarse());
    assert_eq!(at(36.0), Tier::WordStart.coarse());
    assert_eq!(at(1e6), Tier::WordStart.coarse(), "never past a word start");
}

#[test]
fn a_boost_orders_matches_of_one_tier_and_more_habit_still_wins_at_one_boost() {
    let word_start = |frecency, b| points(Tier::WordStart, habit(frecency, THRESHOLD), boost(b));

    assert!(word_start(0.0, 1.5) > word_start(0.0, 1.0));
    assert!(word_start(1.0, 0.5) < word_start(0.0, 1.0));
    assert!(word_start(2.0, 1.0) > word_start(1.0, 1.0));
}

#[test]
fn a_boost_is_a_finite_number_above_0() {
    for b in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(Boost::new(b), None, "{b}");
    }
    assert_eq!(Boost::new(0.25).map(Boost::get), Some(0.25));
    assert_eq!(Boost::default(), Boost::new(DEFAULT_BOOST).unwrap());
}

/// Whether the points of a match at `tier` boosted by `b` rise strictly
/// with `frecencies`, rising themselves.
fn rising(tier: Tier, b: f64, frecencies: &[f32]) -> bool {
    let points: Vec<u32> = frecencies
        .iter()
        .map(|&frecency| points(tier, habit(frecency, THRESHOLD), boost(b)))
        .collect();
    points.is_sorted_by(|a, b| a < b)
}

#[test]
fn heavy_use_past_a_bands_ceiling_still_orders_by_frecency() {
    let heavy = [50.0, 500.0, 5e3, 1e5, 1e7];
    for tier in [Tier::Scattered, Tier::WeakKeyword, Tier::Inside, Tier::WordStart] {
        for b in [0.5, 1.0, 2.0] {
            assert!(rising(tier, b, &heavy), "{tier:?} {b}");
        }
    }
}

#[test]
fn heavily_boosted_exact_matches_still_order_by_habit() {
    for b in [1000.0, 1e6, 1e9] {
        assert!(rising(Tier::Exact, b, &[0.0, 1.0, 10.0, 100.0, 1e4]), "{b}");
    }
}

#[test]
fn points_stay_within_what_the_rank_key_holds() {
    for frecency in FRECENCIES {
        assert!(points(Tier::Exact, habit(frecency, THRESHOLD), boost(f64::MAX)) <= MAX);
    }
}
