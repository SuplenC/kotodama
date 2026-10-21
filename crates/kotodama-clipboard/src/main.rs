//! The clipboard history Bundled extension: everything copied, kept on disk
//! so it survives a restart, and offered as Indexed items of kind
//! `clipboard.clip` that paste it back. A `Clipboard History` Item lists
//! them all, newest first, and clears them. What a password manager marks
//! sensitive is never kept. See `items.rs` for the config table.
//!
//! # Owns
//!
//! - The protocol loop: the config, the Actions, and the Items and Views sent.
//! - Expiring entries on time, and starting to watch the clipboard once the config is in.
//!
//! # Not here
//!
//! - The history on disk: `history.rs`.
//! - What the Items and Views show: `items.rs`.
//! - Watching the clipboard: `wayland.rs`. The Daemon sets it, through the `copy` and `paste`
//!   Outcomes.

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use kotodama_clipboard::copied::Copied;
use kotodama_clipboard::history::Content;
use kotodama_clipboard::history::History;
use kotodama_clipboard::history::Limits;
use kotodama_clipboard::items;
use kotodama_clipboard::items::CLEAR;
use kotodama_clipboard::items::COPY;
use kotodama_clipboard::items::DELETE;
use kotodama_clipboard::items::DETAIL_VIEW;
use kotodama_clipboard::items::HISTORY;
use kotodama_clipboard::items::LIST_VIEW;
use kotodama_clipboard::wayland::Change;
use kotodama_clipboard::wayland::Clipboard;
use kotodama_extension::Chain;
use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::Writer;
use kotodama_extension::dirs;
use kotodama_protocol::Bytes;
use kotodama_protocol::Clipboard as Paste;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-clipboard", run)
}

/// Seconds since the Unix epoch, or 0 on a clock set before it.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn run() -> Result<(), Error> {
    let state = dirs::state("clipboard").ok_or(Error::NoStateDir)?;
    let history = History::open(&state);
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect::<Change>().map_err(Error::Protocol)?;

    let mut extension = Extension {
        writer,
        history,
        limits: Limits::default(),
        watching: None,
        configured: false,
        list_open: false,
        detail: None,
    };
    loop {
        let event = match extension.next_expiry() {
            Some(wait) => match received.recv_timeout(wait) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    extension.expire()?;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match received.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        match event {
            Event::Message(message) => {
                let first = !extension.configured;
                extension.message(message)?;
                // Watching starts once the config is in, so the first copy
                // already expires by the user's limits.
                if first && extension.configured {
                    let events = events.clone();
                    extension.watching = Clipboard::watch(move |change| {
                        let _ = events.send(Event::Own(change));
                    })
                    .map_err(|err| kotodama_extension::log!("not watching the clipboard: {err}"))
                    .ok();
                }
            }
            Event::Own(Change::Copied(copied)) => extension.copied(copied)?,
            // The Daemon starts the Extension again, which connects again.
            Event::Own(Change::Lost) => return Err(Error::WaylandLost),
            Event::Hangup => break,
        }
    }
    Ok(())
}

struct Extension {
    writer: Writer,
    history: History,
    limits: Limits,
    /// The watched clipboard, held so the watching goes on: dropping it stops
    /// it. The Daemon sets the clipboard, through the `copy` Outcome, so
    /// nothing calls `Clipboard::set` here.
    watching: Option<Clipboard>,
    /// Whether the config has arrived. No Items are sent before it.
    configured: bool,
    /// Whether the list View is open, so it is updated as entries change.
    list_open: bool,
    /// The entry the detail View shows, while it is open.
    detail: Option<u64>,
}

type Sent = Result<(), Error>;

impl Extension {
    fn message(&mut self, message: ToExtension) -> Sent {
        match message {
            ToExtension::Config(config) => {
                self.limits = items::limits(&config);
                let removed = self.history.expire(now(), &self.limits);
                if self.configured {
                    self.removed(&removed)
                } else {
                    self.configured = true;
                    let items = std::iter::once(items::history_item())
                        .chain(
                            self.history
                                .entries()
                                .map(|entry| items::clip_item(&self.history, entry)),
                        )
                        .collect();
                    self.send(&FromExtension::AddItems { items })
                }
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => {
                let outcome = self.run(&item.id, &action).unwrap_or_else(|err| Outcome::Failed {
                    message: Chain(&err).to_string(),
                });
                self.send(&FromExtension::ActionDone { invocation, outcome })
            }
            ToExtension::ViewPopped { view } => {
                if view == LIST_VIEW {
                    self.list_open = false;
                } else if view == DETAIL_VIEW {
                    self.detail = None;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Keeps what was just copied, and tells the Daemon.
    fn copied(&mut self, copied: Copied) -> Sent {
        let recorded = match self.history.record(copied, now(), &self.limits) {
            Ok(recorded) => recorded,
            Err(err) => {
                kotodama_extension::log!("couldn't keep what was copied: {err}");
                return Ok(());
            }
        };
        if !recorded.removed.is_empty() {
            self.removed(&recorded.removed)?;
        }
        if let Some(entry) = self.history.get(recorded.id) {
            let item = items::clip_item(&self.history, entry);
            self.send(&FromExtension::AddItems { items: vec![item] })?;
        }
        self.update_list()
    }

    fn expire(&mut self) -> Sent {
        let removed = self.history.expire(now(), &self.limits);
        self.removed(&removed)
    }

    /// How long until the next entry turns too old.
    /// `None` until the config is in: the user's limits aren't known yet.
    fn next_expiry(&self) -> Option<Duration> {
        if !self.configured {
            return None;
        }
        let at = self.history.next_expiry(&self.limits)?;
        Some(Duration::from_secs(at.saturating_sub(now())))
    }

    /// Runs one Action on an entry or on the history Item.
    fn run(&mut self, id: &str, action: &str) -> Result<Outcome, RunError> {
        if id == HISTORY {
            return match action {
                items::OPEN => {
                    self.list_open = true;
                    self.send(&FromExtension::PushView(items::list_view(&self.history)))
                        .map_err(RunError::Send)?;
                    Ok(Outcome::KeepOpen)
                }
                CLEAR => self.clear(),
                _ => Err(RunError::no_history_action(action)),
            };
        }
        let entry = id
            .parse::<u64>()
            .ok()
            .and_then(|id| self.history.get(id))
            .ok_or(RunError::Gone)?;
        let entry_id = entry.id;
        match action {
            items::PASTE => Ok(Outcome::Paste(self.paste(entry_id)?)),
            // The Daemon copies, as it pastes, and serves the clipboard from
            // then on.
            COPY => Ok(Outcome::Copy(self.paste(entry_id)?)),
            items::SHOW => {
                self.detail = Some(entry_id);
                let view = items::detail_view(&self.history, entry, now());
                self.send(&FromExtension::PushView(view)).map_err(RunError::Send)?;
                Ok(Outcome::KeepOpen)
            }
            DELETE => {
                self.history.delete(entry_id);
                self.removed(&[entry_id]).map_err(RunError::Send)?;
                Ok(Outcome::KeepOpen)
            }
            CLEAR => self.clear(),
            _ => Err(RunError::no_clip_action(action)),
        }
    }

    /// What to paste for an entry, read from disk.
    fn paste(&self, id: u64) -> Result<Paste, RunError> {
        let entry = self.history.get(id).ok_or(RunError::Gone)?;
        let data = self.history.read(id).map_err(RunError::Read)?;
        Ok(match &entry.content {
            Content::Text { .. } => Paste::Text(
                String::from_utf8(data).unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned()),
            ),
            Content::Image { mime, .. } => Paste::Image {
                mime: mime.clone(),
                data: Bytes(data),
            },
        })
    }

    fn clear(&mut self) -> Result<Outcome, RunError> {
        let removed = self.history.clear();
        self.removed(&removed).map_err(RunError::Send)?;
        Ok(Outcome::KeepOpen)
    }

    /// Tells the Daemon these entries are gone, and closes or updates the
    /// Views that showed them.
    fn removed(&mut self, ids: &[u64]) -> Sent {
        if ids.is_empty() {
            return Ok(());
        }
        if let Some(shown) = self.detail
            && ids.contains(&shown)
        {
            self.detail = None;
            self.send(&FromExtension::PopView {
                view: DETAIL_VIEW.to_owned(),
            })?;
        }
        self.send(&FromExtension::RemoveItems {
            ids: ids.iter().map(u64::to_string).collect(),
        })?;
        self.update_list()
    }

    fn update_list(&mut self) -> Sent {
        if self.list_open {
            self.send(&FromExtension::UpdateView(items::list_view(&self.history)))?;
        }
        Ok(())
    }

    fn send(&mut self, message: &FromExtension) -> Sent {
        self.writer.send(message).map_err(Error::Protocol)
    }
}

/// Why the Extension stopped. `kotodama_extension::main` logs it.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("neither XDG_STATE_HOME nor HOME is set")]
    NoStateDir,
    /// Talking to the Daemon failed; its error is the message as it is.
    #[error(transparent)]
    Protocol(kotodama_protocol::Error),
    #[error("lost the Wayland connection")]
    WaylandLost,
}

/// Why an Action failed. Its `Display`, with its causes after it, is the
/// failed Outcome's message.
#[derive(Debug, thiserror::Error)]
enum RunError {
    #[error("That clip is no longer in the clipboard history")]
    Gone,
    #[error("Clipboard History has no Action `{0}`")]
    NoHistoryAction(String),
    #[error("A clip has no Action `{0}`")]
    NoClipAction(String),
    #[error("Couldn't read the clip")]
    Read(#[source] io::Error),
    /// A View couldn't be sent to the Daemon; its error is the message as it
    /// is.
    #[error(transparent)]
    Send(Error),
}

impl RunError {
    fn no_history_action(action: &str) -> Self {
        RunError::NoHistoryAction(action.to_owned())
    }

    fn no_clip_action(action: &str) -> Self {
        RunError::NoClipAction(action.to_owned())
    }
}
