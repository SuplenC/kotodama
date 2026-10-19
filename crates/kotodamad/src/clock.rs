//! The wall clock the History's times come from: seconds since the Unix epoch,
//! and the time of day where the user is, which Frecency learns by. The
//! Daemon reads it once, at start, and follows it with the monotonic clock,
//! so tests can set it.

/// Seconds in a day.
pub const DAY_SECS: u64 = 24 * 60 * 60;

/// The wall clock.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Seconds since the Unix epoch at the `now` the Daemon started with.
    pub epoch: u64,
    /// How far the user's local time is ahead of UTC at a time, in seconds.
    pub utc_offset: fn(u64) -> i64,
}

impl Clock {
    /// The system's clock and time zone.
    pub fn system() -> Self {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        Clock {
            epoch,
            utc_offset: local_utc_offset,
        }
    }

    /// A clock that starts at `epoch` in UTC.
    pub fn utc(epoch: u64) -> Self {
        Clock {
            epoch,
            utc_offset: |_| 0,
        }
    }

    /// When it is `elapsed` seconds after the Daemon started.
    pub fn when(&self, elapsed: u64) -> When {
        let at = self.epoch + elapsed;
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
            epoch: 23 * 3600,
            utc_offset: |_| 2 * 3600,
        };
        assert_eq!(clock.when(0).time_of_day, 3600);
        let clock = Clock {
            epoch: 3600,
            utc_offset: |_| -2 * 3600,
        };
        assert_eq!(clock.when(60).time_of_day, 23 * 3600 + 60);
    }
}
