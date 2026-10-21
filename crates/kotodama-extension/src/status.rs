//! Telling the Daemon whether the Extension can do anything now.

use std::io::Write;

use kotodama_protocol::ChannelWriter;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;

/// What the Daemon was last told of whether the Extension can do anything,
/// so each change is sent once. The search shows the Extension's app only
/// while it can.
///
/// An Extension that never says counts as able once it has sent Items, or
/// from the start if its Manifest asks for Live queries. Say so when that
/// isn't true: an Extension for one compositor running under another, or one
/// that needs a setting the user hasn't made.
///
/// ```no_run
/// use kotodama_extension::{SentStatus, Writer};
///
/// fn connected(writer: &mut Writer, running: bool) -> Result<(), kotodama_protocol::Error> {
///     let mut status = SentStatus::default();
///     if running {
///         status.usable(writer)
///     } else {
///         status.unusable(writer, "Hyprland isn't running")
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct SentStatus(Option<(bool, Option<String>)>);

impl SentStatus {
    /// Says the Extension can do something.
    ///
    /// # Errors
    ///
    /// When the message can't be written to the Daemon.
    pub fn usable<W: Write>(&mut self, writer: &mut ChannelWriter<W>) -> Result<(), Error> {
        self.send(writer, true, None)
    }

    /// Says the Extension can do something, and what the user should know as
    /// they enter its app, such as that it is paused; the app's intro card
    /// shows `note`.
    ///
    /// # Errors
    ///
    /// When the message can't be written to the Daemon.
    pub fn noted<W: Write>(&mut self, writer: &mut ChannelWriter<W>, note: impl AsRef<str>) -> Result<(), Error> {
        self.send(writer, true, Some(note.as_ref()))
    }

    /// Says the Extension can't do anything now, and why, such as `Hyprland
    /// isn't running`: the search leaves its app out until it says it can.
    ///
    /// # Errors
    ///
    /// When the message can't be written to the Daemon.
    pub fn unusable<W: Write>(&mut self, writer: &mut ChannelWriter<W>, why: impl AsRef<str>) -> Result<(), Error> {
        self.send(writer, false, Some(why.as_ref()))
    }

    fn send<W: Write>(&mut self, writer: &mut ChannelWriter<W>, usable: bool, note: Option<&str>) -> Result<(), Error> {
        match self.update(usable, note) {
            Some(message) => writer.send(&message),
            None => Ok(()),
        }
    }

    /// Makes `usable` and `note` what the Daemon was told, without sending
    /// anything. Returns the message to send, or `None` when it was told so
    /// already.
    fn update(&mut self, usable: bool, note: Option<&str>) -> Option<FromExtension> {
        if let Some((sent, sent_note)) = &self.0
            && *sent == usable
            && sent_note.as_deref() == note
        {
            return None;
        }
        let note = note.map(str::to_owned);
        self.0 = Some((usable, note.clone()));
        Some(FromExtension::Status { usable, note })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_change_is_sent_once() {
        let mut status = SentStatus::default();

        let first = status.update(false, Some("Hyprland isn't running"));
        let again = status.update(false, Some("Hyprland isn't running"));
        let up = status.update(true, None);
        let noted = status.update(true, Some("Paused"));

        assert_eq!(
            first,
            Some(FromExtension::Status {
                usable: false,
                note: Some("Hyprland isn't running".into()),
            })
        );
        assert_eq!(again, None);
        assert_eq!(
            up,
            Some(FromExtension::Status {
                usable: true,
                note: None,
            })
        );
        assert!(noted.is_some());
    }
}
