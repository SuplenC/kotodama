//! Pausing the history, as before copying a password from a page that
//! doesn't mark it sensitive: for 5 minutes, an hour, or until resumed.
//! The pause is kept on disk, so a restart of the Extension doesn't end it.
//!
//! # Owns
//!
//! - Whether a copy is kept, when a pause ends, and the note the app's intro card shows.
//! - The pause's file in the state directory.
//!
//! # Not here
//!
//! - The Item and its Actions: `items.rs`.
//! - Skipping the copies and telling the Daemon: `main.rs`.

use std::fs;
use std::io;
use std::path::Path;

/// The pause's file in the state directory: the second it ends at, since
/// the Unix epoch, or [`UNTIL_RESUMED`].
const FILE: &str = "paused";
/// What the file holds for a pause without an end.
const UNTIL_RESUMED: &str = "until-resumed";

const MINUTE: u64 = 60;

/// Whether the history keeps what is copied. Times are seconds since the
/// Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pause {
    /// Everything copied is kept.
    #[default]
    Off,
    /// Nothing copied is kept until this second.
    Until(u64),
    /// Nothing copied is kept until the user resumes it.
    UntilResumed,
}

impl Pause {
    /// The pause read from `dir`, as last saved there; off when there is
    /// none, or the one there is over by `now`.
    #[must_use]
    pub fn load(dir: &Path, now: u64) -> Pause {
        let Ok(saved) = fs::read_to_string(dir.join(FILE)) else {
            return Pause::Off;
        };
        let pause = match saved.trim() {
            UNTIL_RESUMED => Pause::UntilResumed,
            until => until.parse().map_or_else(
                |_| {
                    kotodama_extension::log!("{}: not a pause; keeping copies", dir.join(FILE).display());
                    Pause::Off
                },
                Pause::Until,
            ),
        };
        pause.settled(now)
    }

    /// Writes the pause into `dir`, or removes it there when it is off.
    ///
    /// # Errors
    ///
    /// The error writing or removing its file.
    pub fn save(self, dir: &Path) -> io::Result<()> {
        let path = dir.join(FILE);
        match self {
            Pause::Off => match fs::remove_file(&path) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
                _ => Ok(()),
            },
            Pause::Until(until) => {
                fs::create_dir_all(dir)?;
                fs::write(path, until.to_string())
            }
            Pause::UntilResumed => {
                fs::create_dir_all(dir)?;
                fs::write(path, UNTIL_RESUMED)
            }
        }
    }

    /// Whether what is copied at `now` is kept.
    #[must_use]
    pub fn keeps(self, now: u64) -> bool {
        self.settled(now) == Pause::Off
    }

    /// The pause as it is at `now`: off once its end has passed.
    #[must_use]
    pub fn settled(self, now: u64) -> Pause {
        match self {
            Pause::Until(until) if until <= now => Pause::Off,
            pause => pause,
        }
    }

    /// What the user should know as they enter the clipboard app, while the
    /// history is paused at `now`.
    #[must_use]
    pub fn note(self, now: u64) -> Option<String> {
        match self.settled(now) {
            Pause::Off => None,
            Pause::Until(until) => {
                let minutes = (until - now).div_ceil(MINUTE);
                let left = if minutes == 1 {
                    "1 more minute".to_owned()
                } else {
                    format!("{minutes} more minutes")
                };
                Some(format!("Paused for {left}: nothing copied is kept"))
            }
            Pause::UntilResumed => Some("Paused until you resume it: nothing copied is kept".to_owned()),
        }
    }

    /// How many seconds from `now` until the note changes or the pause
    /// ends; `None` when neither ever happens by itself.
    #[must_use]
    pub fn next_change(self, now: u64) -> Option<u64> {
        match self.settled(now) {
            Pause::Until(until) => {
                let left = until - now;
                Some(left - (left.div_ceil(MINUTE) - 1) * MINUTE)
            }
            Pause::Off | Pause::UntilResumed => None,
        }
    }
}

#[cfg(test)]
mod tests;
