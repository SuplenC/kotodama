use super::*;
use crate::clock::DAY_SECS;

const HOUR: u32 = 3600;
const FRESH_SECS: u64 = 5 * 60;

/// `hour` seconds into a day, in UTC.
fn at(hour: u32) -> When {
    When {
        at: 1_792_368_000 + u64::from(hour),
        time_of_day: hour,
    }
}

fn slot(row: u32) -> Slot {
    Slot { segment: 0, row }
}

/// Rows ranked at `ranked_at`, and for the next part of the day.
fn ahead(ranked_at: When) -> Ahead {
    Ahead {
        versions: (1, 2, 3),
        ranked: vec![
            (ranked_at, vec![Source::Indexed(slot(1))]),
            (
                history::next_part(ranked_at),
                vec![Source::Indexed(slot(2))],
            ),
        ],
    }
}

#[test]
fn rows_ranked_ahead_stand_for_their_part_of_the_day_alone() {
    let ahead = ahead(at(10 * HOUR));

    let now = ahead.at((1, 2, 3), at(10 * HOUR + 60), FRESH_SECS);
    let next = ahead.at((1, 2, 3), at(12 * HOUR + 60), FRESH_SECS);
    let later = ahead.at((1, 2, 3), at(15 * HOUR), FRESH_SECS);
    let next_day = When {
        at: at(10 * HOUR).at + DAY_SECS,
        ..at(10 * HOUR)
    };
    let next_day = ahead.at((1, 2, 3), next_day, FRESH_SECS);

    assert_eq!(now, Some(&[Source::Indexed(slot(1))][..]));
    assert_eq!(next, Some(&[Source::Indexed(slot(2))][..]));
    assert_eq!(later, None, "a part later");
    assert_eq!(next_day, None, "a day later");
}

#[test]
fn rows_ranked_ahead_go_stale_with_what_they_were_ranked_from_and_with_time() {
    let ahead = ahead(at(10 * HOUR));

    let changed = ahead.at((1, 2, 4), at(10 * HOUR), FRESH_SECS);
    let old = ahead.at((1, 2, 3), at(10 * HOUR + 301), FRESH_SECS);
    let early = ahead.at((1, 2, 3), at(10 * HOUR - 1), FRESH_SECS);

    assert_eq!(changed, None, "ranked from another version");
    assert_eq!(old, None, "ranked too long before");
    assert_eq!(early, None, "ranked for later");
}

#[test]
fn the_rows_ranked_ahead_name_their_indexed_items() {
    let slots: Vec<Slot> = ahead(at(10 * HOUR)).slots().collect();

    assert_eq!(slots, [slot(1), slot(2)]);
}
