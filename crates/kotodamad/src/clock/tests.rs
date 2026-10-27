use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use super::*;

#[test]
fn the_time_of_day_is_local() {
    let clock = Clock::new(Some(23 * 3600), |_| 2 * 3600);
    assert_eq!(clock.when(Duration::ZERO).time_of_day, 3600);
    let clock = Clock::new(Some(3600), |_| -2 * 3600);
    assert_eq!(clock.when(Duration::from_secs(60)).time_of_day, 23 * 3600 + 60);
}

/// A Monday, 2026-10-19, at `hour` o'clock UTC.
fn monday(hour: u64) -> u64 {
    1_792_368_000 + hour * 3600
}

#[test]
fn the_day_of_the_week_is_local() {
    assert_eq!(When::utc(monday(12)).day_of_week, 0);
    let clock = Clock::new(Some(monday(23)), |_| 2 * 3600);
    assert_eq!(clock.when(Duration::ZERO).day_of_week, 1, "Tuesday already");
    let clock = Clock::new(Some(monday(1)), |_| -2 * 3600);
    assert_eq!(clock.when(Duration::ZERO).day_of_week, 6, "still Sunday");
}

#[test]
fn a_moment_moved_by_some_seconds_keeps_its_time_zone() {
    let clock = Clock::new(Some(monday(1)), |_| 2 * 3600);
    let three = clock.when(Duration::ZERO);
    let earlier = three.plus(-4 * 3600);
    assert_eq!(earlier.at, monday(1) - 4 * 3600);
    assert_eq!(earlier.time_of_day, 23 * 3600);
    assert_eq!(earlier.day_of_week, 6);
    let later = three.plus(7 * DAY_SECS as i64 - 3 * 3600);
    assert_eq!(later.time_of_day, 0);
    assert_eq!(later.day_of_week, 0);
}

#[test]
fn the_system_clock_is_read_each_time() {
    let clock = Clock::system();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let at = clock.when(Duration::ZERO).at;
    assert!(at.abs_diff(now) <= 1, "{at} {now}");
}

/// How often the zone of `counted` was read.
static READS: AtomicU64 = AtomicU64::new(0);

/// A zone one hour ahead of UTC that counts its readings.
fn counted(_at: u64) -> i64 {
    READS.fetch_add(1, Ordering::Relaxed);
    3600
}

#[test]
fn the_time_zone_is_read_once_a_minute() {
    let clock = Clock::new(Some(10 * ZONE_SECS), counted);

    for secs in [0, 1, 30, 59] {
        assert_eq!(
            clock.when(Duration::from_secs(secs)).time_of_day,
            3600 + 600 + secs as u32
        );
    }
    assert_eq!(READS.load(Ordering::Relaxed), 1, "once in the first minute");
    clock.when(Duration::from_secs(60));
    clock.when(Duration::from_secs(61));
    assert_eq!(READS.load(Ordering::Relaxed), 2, "again in the next");
}
