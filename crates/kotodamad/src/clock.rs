//! The wall clock the History's times come from: seconds since the Unix epoch,
//! and the time of day where the user is, which Frecency learns by.
//!
//! # Owns
//!
//! - [`Clock`]: the wall clock and the user's time zone, read again once a minute.
//! - [`When`]: a moment as the History counts it, and [`DAY_SECS`].
//!
//! # Not here
//!
//! - What the History does with the time of day: `history.rs`, `part_of_day`.

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
    /// read from the current time zone.
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

/// The local time's offset from UTC at `at`, in the current time zone.
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
    if unsafe { libc::localtime_r(&raw const time, &raw mut tm) }.is_null() {
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
mod tests;
