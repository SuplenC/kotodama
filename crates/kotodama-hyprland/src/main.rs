//! The Hyprland Bundled extension. Hyprland's open windows are Items of the
//! shared kind `window`, read once and then kept up to date from its event
//! socket, sending only the windows that changed. Its binds with a
//! description are Items that run the bind; a `Keyboard Shortcuts` Item opens
//! a View of every bind, grouped by modifier, read afresh each time it opens,
//! so `kotodama open --view hyprland/shortcuts` opens on it. Outside Hyprland
//! the Extension sends nothing and waits for the Daemon to hang up.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::thread;

use kotodama_extension::{Connection, Event, STACK_SIZE, SentItems, Writer, log};
use kotodama_hyprland::binds::{self, Bind, OPEN, RUN, SHORTCUTS};
use kotodama_hyprland::events::Change;
use kotodama_hyprland::hyprland::Hyprland;
use kotodama_hyprland::windows::{self, WORKSPACE, Windows};
use kotodama_protocol::{Error, FromExtension, ItemKind, ItemRef, Outcome, ToExtension};

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-hyprland", run)
}

/// What the event thread sends.
enum Own {
    Change(Change),
    /// The event socket closed: Hyprland exited.
    Lost,
}

fn run() -> Result<(), Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect::<Own>()?;
    let mut extension = Extension {
        writer,
        sent: SentItems::default(),
        hyprland: None,
        windows: Windows::default(),
        binds: Vec::new(),
        view_open: false,
        changed: false,
    };
    // Outside Hyprland there is nothing to follow: only the Daemon's messages
    // wake the Extension.
    if let Some(hyprland) = Hyprland::from_env() {
        // Events are followed before the windows are read, so none is missed
        // in between; one about a window read already changes nothing.
        match hyprland.events() {
            Ok(mut stream) => {
                thread::Builder::new()
                    .name("events".into())
                    .stack_size(STACK_SIZE)
                    .spawn(move || {
                        while let Some(change) = stream.next_change() {
                            if events.send(Event::Own(Own::Change(change))).is_err() {
                                return;
                            }
                        }
                        let _ = events.send(Event::Own(Own::Lost));
                    })?;
                extension.hyprland = Some(hyprland);
                extension.read_windows();
                extension.read_binds();
                extension.publish()?;
            }
            Err(err) => log!("not following Hyprland: {err}"),
        }
    }

    while let Ok(mut event) = received.recv() {
        // Handle everything already waiting, so a burst of events sends its
        // changes once.
        loop {
            if !extension.handle(event)? {
                return Ok(());
            }
            match received.try_recv() {
                Ok(next) => event = next,
                Err(_) => break,
            }
        }
        if extension.changed {
            extension.publish()?;
        }
    }
    Ok(())
}

struct Extension {
    writer: Writer,
    sent: SentItems,
    /// `None` outside Hyprland, and once it exits.
    hyprland: Option<Hyprland>,
    windows: Windows,
    binds: Vec<Bind>,
    /// Whether the Shortcuts View is open, so a config reload updates it.
    view_open: bool,
    /// Whether an Item changed since the last publish.
    changed: bool,
}

impl Extension {
    /// Handles one event; false once the Daemon hung up.
    fn handle(&mut self, event: Event<Own>) -> Result<bool, Error> {
        match event {
            Event::Message(ToExtension::RunAction {
                invocation,
                action,
                item,
                arguments,
            }) => {
                let outcome = self
                    .run(&action, &item, &arguments)
                    .unwrap_or_else(|message| Outcome::Failed { message });
                self.writer.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })?;
            }
            Event::Message(ToExtension::ViewPopped { view }) if view == binds::VIEW => {
                self.view_open = false;
            }
            Event::Message(_) => {}
            Event::Own(Own::Change(Change::ConfigReloaded)) => {
                self.read_binds();
                self.changed = true;
                if self.view_open {
                    self.writer
                        .send(&FromExtension::UpdateView(binds::view(&self.binds)))?;
                }
            }
            Event::Own(Own::Change(change)) => self.changed |= self.windows.apply(&change),
            Event::Own(Own::Lost) => {
                log!("Hyprland exited");
                self.hyprland = None;
                self.windows = Windows::default();
                self.binds = Vec::new();
                self.changed = true;
            }
            Event::Hangup => return Ok(false),
        }
        Ok(true)
    }

    fn run(
        &mut self,
        action: &str,
        item: &ItemRef,
        arguments: &BTreeMap<String, String>,
    ) -> Result<Outcome, String> {
        let hyprland = self.hyprland.as_ref().ok_or("Hyprland isn't running")?;
        match &item.kind {
            ItemKind::Window { address } => {
                let workspace = arguments.get(WORKSPACE).map(String::as_str);
                hyprland.dispatch(&windows::dispatch(action, address, workspace)?)?;
                Ok(Outcome::Close)
            }
            kind if kind.name() == binds::KIND && action == RUN => {
                let bind = binds::find(&self.binds, &item.id)
                    .ok_or("That shortcut is no longer bound in Hyprland")?;
                hyprland.dispatch(&bind.dispatch())?;
                Ok(Outcome::Close)
            }
            _ if item.id == SHORTCUTS && action == OPEN => {
                // Read afresh, so a bind added at runtime is there: Hyprland
                // sends no event for one.
                self.read_binds();
                self.publish().map_err(|err| err.to_string())?;
                self.writer
                    .send(&FromExtension::PushView(binds::view(&self.binds)))
                    .map_err(|err| err.to_string())?;
                self.view_open = true;
                Ok(Outcome::KeepOpen)
            }
            _ => Err(format!("`{}` has no Action `{action}`", item.id)),
        }
    }

    fn read_windows(&mut self) {
        let Some(hyprland) = &self.hyprland else {
            return;
        };
        match hyprland
            .request("j/clients")
            .map_err(|err| err.to_string())
            .and_then(|json| Windows::from_clients(&json))
        {
            Ok(windows) => self.windows = windows,
            Err(err) => log!("couldn't read the windows: {err}"),
        }
    }

    fn read_binds(&mut self) {
        let Some(hyprland) = &self.hyprland else {
            return;
        };
        match hyprland
            .request("j/binds")
            .map_err(|err| err.to_string())
            .and_then(|json| binds::parse(&json))
        {
            Ok(binds) => self.binds = binds,
            Err(err) => log!("couldn't read the binds: {err}"),
        }
    }

    /// Sends what changed since the last time: nothing at all outside
    /// Hyprland.
    fn publish(&mut self) -> Result<(), Error> {
        self.changed = false;
        let items = self.hyprland.is_some().then(|| {
            std::iter::once(binds::shortcuts_item())
                .chain(binds::items(&self.binds))
                .chain(self.windows.items())
        });
        self.sent
            .publish(&mut self.writer, items.into_iter().flatten())
    }
}
