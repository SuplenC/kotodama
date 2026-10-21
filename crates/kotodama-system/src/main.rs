//! The system Bundled extension. Power Off, Reboot, Suspend, Hibernate (when
//! logind can), Lock Screen and Log Out are Items that call logind; the three
//! that end the session ask the Front end to confirm them. The open windows of a
//! compositor with wlr-foreign-toplevel-management are Items of the shared
//! kind `window`, kept up to date as the compositor reports them, sending only
//! the windows that changed. Without the protocol, or on Hyprland, whose own
//! Extension has its windows, the Extension sends no windows and only the
//! Daemon's messages wake it.
//!
//! # Owns
//!
//! - The protocol loop, and answering a run and the windows' changes.
//!
//! # Not here
//!
//! - logind, the power Items and the windows: the library's modules.
//! - Sending only the Items that changed: `kotodama_extension::SentItems`.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use kotodama_extension::Chain;
use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::SentItems;
use kotodama_extension::SentStatus;
use kotodama_extension::Writer;
use kotodama_extension::log;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;
use kotodama_protocol::ItemKind;
use kotodama_protocol::ItemRef;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;
use kotodama_system::logind;
use kotodama_system::power;
use kotodama_system::power::Power;
#[cfg(target_os = "linux")]
use kotodama_system::toplevels;
#[cfg(target_os = "linux")]
use kotodama_system::toplevels::Change;
#[cfg(target_os = "linux")]
use kotodama_system::toplevels::Compositor;
use kotodama_system::windows::Windows;

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-system", run)
}

/// What the toplevels thread sends.
#[cfg(target_os = "linux")]
type Own = Change;
#[cfg(not(target_os = "linux"))]
type Own = std::convert::Infallible;

fn run() -> Result<(), Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect::<Own>()?;
    let hibernate = logind::can_hibernate().unwrap_or_else(|err| {
        log!("not offering Hibernate: {}", Chain(&err));
        false
    });
    let mut extension = Extension {
        writer,
        #[cfg(target_os = "linux")]
        compositor: None,
        windows: Windows::default(),
        sent_windows: SentItems::default(),
        windows_changed: false,
    };
    SentItems::default().publish(&mut extension.writer, power::items(hibernate))?;
    // The power Items work on any compositor, with or without its windows.
    SentStatus::default().usable(&mut extension.writer)?;
    #[cfg(target_os = "linux")]
    toplevels::follow(move |change| events.send(Event::Own(change)).is_ok()).map_err(Error::Io)?;
    #[cfg(not(target_os = "linux"))]
    drop(events);

    while let Ok(mut event) = received.recv() {
        // Handle everything already waiting, so a burst of window changes
        // sends them once.
        loop {
            if !extension.handle(event)? {
                return Ok(());
            }
            match received.try_recv() {
                Ok(next) => event = next,
                Err(_) => break,
            }
        }
        if extension.windows_changed {
            extension.windows_changed = false;
            extension
                .sent_windows
                .publish(&mut extension.writer, extension.windows.items())?;
        }
    }
    Ok(())
}

struct Extension {
    writer: Writer,
    /// `None` until a compositor with the protocol is there, and once it exits.
    #[cfg(target_os = "linux")]
    compositor: Option<Compositor>,
    windows: Windows,
    sent_windows: SentItems,
    /// Whether a window's Item changed since the windows were last sent.
    windows_changed: bool,
}

impl Extension {
    /// Handles one event; false once the Daemon hung up.
    fn handle(&mut self, event: Event<Own>) -> Result<bool, Error> {
        match event {
            Event::Message(ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            }) => {
                let outcome = self
                    .run(&action, &item)
                    .unwrap_or_else(|message| Outcome::Failed { message });
                self.writer.send(&FromExtension::ActionDone { invocation, outcome })?;
            }
            Event::Message(_) => {}
            #[cfg(target_os = "linux")]
            Event::Own(change) => self.apply(change),
            #[cfg(not(target_os = "linux"))]
            Event::Own(never) => match never {},
            Event::Hangup => return Ok(false),
        }
        Ok(true)
    }

    fn run(&mut self, action: &str, item: &ItemRef) -> Result<Outcome, String> {
        match &item.kind {
            ItemKind::Window { address } => {
                let (number, request) = self.windows.request(action, address).map_err(|err| err.to_string())?;
                self.request(number, request)?;
                Ok(Outcome::Close)
            }
            kind if kind.name() == power::KIND && action == power::RUN => {
                let power = Power::from_id(&item.id).ok_or_else(|| format!("`{}` isn't a power Item", item.id))?;
                logind::run(power).map_err(|err| failure(power, &err))?;
                Ok(Outcome::Close)
            }
            _ => Err(format!("`{}` has no Action `{action}`", item.id)),
        }
    }

    #[cfg(target_os = "linux")]
    fn request(&self, number: u64, request: kotodama_system::windows::Request) -> Result<(), String> {
        self.compositor
            .as_ref()
            .ok_or("The compositor is gone")?
            .request(number, request)
    }

    #[cfg(not(target_os = "linux"))]
    fn request(&self, _number: u64, _request: kotodama_system::windows::Request) -> Result<(), String> {
        Err("Windows need Wayland, which only Linux has".into())
    }

    #[cfg(target_os = "linux")]
    fn apply(&mut self, change: Change) {
        match change {
            Change::Up(compositor) => self.compositor = Some(compositor),
            Change::Window { number, window } => self.windows_changed |= self.windows.update(number, window),
            Change::Closed { number } => self.windows_changed |= self.windows.remove(number),
            Change::Lost => {
                self.compositor = None;
                self.windows.clear();
                self.windows_changed = true;
            }
        }
    }
}

/// What the user reads when `power` failed.
fn failure(power: Power, err: &logind::Error) -> String {
    format!("Couldn't {}: {}", power.title().to_lowercase(), Chain(err))
}
