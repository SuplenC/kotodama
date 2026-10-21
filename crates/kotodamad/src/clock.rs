//! The wall clock the History's times come from: seconds since the Unix epoch,
//! and the time of day where the user is, which Frecency learns by.

use std::cell::Cell;
use std::time::Duration;
use std::time::SystemTime;

/// Seconds in a day.
pub const DAY_SECS: u64 = 24 * 60 * 60;

/// How long one reading of the time zone holds. Time zones change their
/// offset on a minute's boundary, so a reading holds for the rest of its
/// minute, and a new zone counts from the next one.
const ZONE_SECS: u64 = 60;

/// The wall clock.
#[derive(Clone, Debug)]
pub struct Clock {
    /// The time when the Daemon started, for a clock that follows the
    /// Daemon's own. `None` reads the system's, which also counts the time
    /// the machine spends suspended.
    start: Option<u64>,
    /// How far the user's local time is ahead of UTC at a time, in seconds,
    /// read from the time zone as it is now.
    utc_offset: fn(u64) -> i64,
    /// The minute, counted from the epoch, `utc_offset` was last read in,
    /// and what it read.
    zone: Cell<(u64, i64)>,
}

impl Clock {
    /// The system's clock and time zone.
    #[must_use]
    pub fn system() -> Self {
        Clock::new(None, local_utc_offset)
    }

    /// A clock in UTC that reads `start` when the Daemon starts and then
    /// follows the time the Daemon is given, as tests set it.
    #[must_use]
    pub fn starting_at(start: u64) -> Self {
        Clock::new(Some(start), |_| 0)
    }

    /// A clock in the system's time zone that reads `start` when the Daemon
    /// starts and then follows the time the Daemon is given.
    #[must_use]
    pub fn local_starting_at(start: u64) -> Self {
        Clock::new(Some(start), local_utc_offset)
    }

    fn new(start: Option<u64>, utc_offset: fn(u64) -> i64) -> Self {
        Clock {
            start,
            utc_offset,
            zone: Cell::new((u64::MAX, 0)),
        }
    }

    /// When it is, `since_start` after the Daemon started.
    pub fn when(&self, since_start: Duration) -> When {
        let at = match self.start {
            Some(start) => start + since_start.as_secs(),
            None => SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        };
        // Reading the zone costs a `stat` of `/etc/localtime`, so it is read
        // once a minute rather than on every keystroke.
        let minute = at / ZONE_SECS;
        let (read_in, mut offset) = self.zone.get();
        if read_in != minute {
            offset = (self.utc_offset)(at);
            self.zone.set((minute, offset));
        }
        let local = at as i64 + offset;
        When {
            at,
            time_of_day: local.rem_euclid(DAY_SECS as i64) as u32,
        }
    }
}

/// A moment, as the History counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct When {
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// Seconds since local midnight.
    pub time_of_day: u32,
}

/// The local time's offset from UTC at `at`, in the time zone as it is now.
/// glibc's `localtime_r` reads the zone once per process, and `tzset` reads
/// `TZ` and `/etc/localtime` again: a `stat` of the file, which it parses
/// only when it changed. So a zone the user switches to while the Daemon
/// runs, or a new rule installed for it, counts from the next call.
#[cfg(unix)]
fn local_utc_offset(at: u64) -> i64 {
    let time = at as libc::time_t;
    // SAFETY: `tzset` reads the environment, which nothing in the Daemon
    // writes to once it runs.
    unsafe { tzset() };
    // SAFETY: an all-zero `tm` is valid, and `localtime_r` only writes to it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

// POSIX `tzset`, which the `libc` crate doesn't bind.
#[cfg(unix)]
unsafe extern "C" {
    fn tzset();
}

#[cfg(not(unix))]
fn local_utc_offset(_at: u64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
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
}
