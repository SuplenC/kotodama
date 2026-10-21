
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
