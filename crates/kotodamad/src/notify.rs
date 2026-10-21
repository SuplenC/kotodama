//! Desktop notifications through `org.freedesktop.Notifications` on the session
//! bus.
//!
//! The Daemon sends one only when a job finishes, so it connects, calls
//! `Notify`, waits for the answer and hangs up, over the small D-Bus client
//! in `kotodama_extension::dbus`.
//!
//! # Owns
//!
//! - [`send`]: one desktop notification.
//!
//! # Not here
//!
//! - The D-Bus client: `kotodama_extension::dbus`.

use kotodama_extension::dbus::Bus;
use kotodama_extension::dbus::Call;
use kotodama_extension::dbus::Error;
use kotodama_extension::dbus::Marshal;

/// Shows a notification with `summary` and `body`. Blocks until the
/// notification service has answered, so call it off the main loop.
///
/// # Errors
///
/// When there is no session bus, or the notification service doesn't answer
/// or answers with an error.
pub fn send(summary: &str, body: &str) -> Result<(), Error> {
    let mut bus = Bus::session()?;
    let call = Call::new(
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        "org.freedesktop.Notifications",
    )
    .with_body("susssasa{sv}i", notify_body(summary, body));
    bus.call(&call, |_| {})?;
    Ok(())
}

/// `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints,
/// expire_timeout)`.
fn notify_body(summary: &str, body: &str) -> Marshal {
    let mut m = Marshal::default();
    m.string("Kotodama");
    m.u32(0);
    m.string("");
    m.string(summary);
    m.string(body);
    // No actions.
    m.array(4, |_| {});
    // No hints.
    m.array(8, |_| {});
    // The server's default timeout.
    m.i32(-1);
    m
}
