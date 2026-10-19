//! The wall clock the History's times come from: seconds since the Unix epoch,
//! and the time of day where the user is, which Frecency learns by.

use std::time::{Duration, SystemTime};

/// Seconds in a day.
pub const DAY_SECS: u64 = 24 * 60 * 60;

/// The wall clock.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// The time when the Daemon started, for a clock that follows the
    /// Daemon's own. `None` reads the system's, which also counts the time
    /// the machine spends suspended.
    start: Option<u64>,
    /// How far the user's local time is ahead of UTC at a time, in seconds.
    utc_offset: fn(u64) -> i64,
}

impl Clock {
    /// The system's clock and time zone.
    pub fn system() -> Self {
        Clock {
            start: None,
            utc_offset: local_utc_offset,
        }
    }

    /// A clock in UTC that reads `start` when the Daemon starts and then
    /// follows the time the Daemon is given, as tests set it.
    pub fn starting_at(start: u64) -> Self {
        Clock {
            start: Some(start),
            utc_offset: |_| 0,
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
        let local = at as i64 + (self.utc_offset)(at);
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

#[cfg(unix)]
fn local_utc_offset(at: u64) -> i64 {
    let time = at as libc::time_t;
    // SAFETY: an all-zero `tm` is valid, and `localtime_r` only writes to it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

#[cfg(not(unix))]
fn local_utc_offset(_at: u64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_of_day_is_local() {
        let clock = Clock {
            start: Some(23 * 3600),
            utc_offset: |_| 2 * 3600,
        };
        assert_eq!(clock.when(Duration::ZERO).time_of_day, 3600);
        let clock = Clock {
            start: Some(3600),
            utc_offset: |_| -2 * 3600,
        };
        assert_eq!(
            clock.when(Duration::from_secs(60)).time_of_day,
            23 * 3600 + 60
        );
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
}
