//! logind (`org.freedesktop.login1`) on the system bus: what each power Item
//! asks it, and whether it can hibernate.
//!
//! Every call opens the system bus, makes the call and hangs up: they come
//! one at a time when the user picks an Item, so a connection kept open would
//! cost memory and a socket for the Extension's whole life to save a few
//! milliseconds.
//!
//! The session is `auto`: the caller's own session, or, for a caller outside
//! any session such as this Extension under the user's service manager, the
//! user's graphical one. Locking it makes logind send the session's `Lock`
//! signal, which the user's screen locker answers.
//!
//! # Owns
//!
//! - The method each [`Power`] calls, and its arguments.
//! - Reading `CanHibernate`.
//!
//! # Not here
//!
//! - Talking D-Bus: `kotodama_extension::dbus`.

use kotodama_extension::dbus::Bus;
use kotodama_extension::dbus::Call;
use kotodama_extension::dbus::Marshal;

use crate::power::Power;

/// The bus name logind owns.
pub const DESTINATION: &str = "org.freedesktop.login1";
/// The path of logind's manager object.
pub const PATH: &str = "/org/freedesktop/login1";
/// The interface of logind's manager object.
pub const MANAGER: &str = "org.freedesktop.login1.Manager";
/// The session logind resolves to the caller's, or to the user's graphical one.
pub const AUTO: &str = "auto";

/// Why logind couldn't be asked, or turned the call down.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No system bus, or it refused us.
    #[error("couldn't reach the system bus")]
    Bus(#[source] kotodama_extension::dbus::Error),
    /// logind isn't there, or said no, as polkit does when another user is
    /// logged in.
    #[error("logind turned it down")]
    Call(#[source] kotodama_extension::dbus::Error),
}

impl Error {
    fn bus(err: kotodama_extension::dbus::Error) -> Self {
        Error::Bus(err)
    }

    fn call(err: kotodama_extension::dbus::Error) -> Self {
        Error::Call(err)
    }
}

/// The manager's method `power` calls.
#[must_use]
pub fn method(power: Power) -> &'static str {
    match power {
        Power::PowerOff => "PowerOff",
        Power::Reboot => "Reboot",
        Power::Suspend => "Suspend",
        Power::Hibernate => "Hibernate",
        Power::Lock => "LockSession",
        Power::LogOut => "TerminateSession",
    }
}

/// Asks logind to do `power`. It returns once logind has taken the request:
/// the machine powers off, or the session ends, after.
///
/// # Errors
///
/// [`Error`] when the system bus can't be reached or logind turns it down.
pub fn run(power: Power) -> Result<(), Error> {
    let mut body = Marshal::default();
    let signature = match power {
        Power::Lock | Power::LogOut => {
            body.string(AUTO);
            "s"
        }
        // `interactive` is false: nothing would show a polkit prompt for a
        // menu that has closed, so a call that needs a password fails at once.
        Power::PowerOff | Power::Reboot | Power::Suspend | Power::Hibernate => {
            body.u32(0);
            "b"
        }
    };
    let call = Call::new(PATH, MANAGER, method(power), DESTINATION).with_body(signature, body);
    let mut bus = Bus::system().map_err(Error::bus)?;
    bus.call(&call, |_| {}).map_err(Error::call)?;
    Ok(())
}

/// Whether logind says it can hibernate: `yes`, and not `no`, `na` or
/// `challenge` (only with a password).
///
/// # Errors
///
/// [`Error`] when the system bus can't be reached or logind doesn't answer.
pub fn can_hibernate() -> Result<bool, Error> {
    let call = Call::new(PATH, MANAGER, "CanHibernate", DESTINATION);
    let mut bus = Bus::system().map_err(Error::bus)?;
    let answer = bus.call(&call, |_| {}).map_err(Error::call)?;
    Ok(answer.body().string() == Some("yes"))
}
