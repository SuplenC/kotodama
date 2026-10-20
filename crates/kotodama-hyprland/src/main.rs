//! The Hyprland Bundled extension. Hyprland's open windows are Items of the
//! shared kind `window`, read once and then kept up to date from its event
//! socket, sending only the windows that changed. Its binds with a
//! description are Items that run the bind; a `Keyboard Shortcuts` Item opens
//! a View of every bind, grouped by modifier, read afresh each time it opens,
//! so `kotodama open --view hyprland/shortcuts` opens on it. Outside Hyprland
//! the Extension sends nothing and waits, at no cost, for a Hyprland to start
//! (in `$XDG_RUNTIME_DIR/hypr/`, see `Hyprland::wait`) or the Daemon to hang
//! up; when Hyprland restarts it follows the new instance.

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
    /// A Hyprland is running, and its events are followed.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Up(Hyprland),
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
        hyprland: None,
        windows: Windows::default(),
        sent_windows: SentItems::default(),
        windows_changed: false,
        binds: Vec::new(),
        sent_binds: SentItems::default(),
        view_open: false,
    };
    // Waits for Hyprland, follows it, and once it exits waits for the next
    // one: it may start after the Extension, or restart under a new
    // signature. Waiting costs nothing, so outside Hyprland only the
    // Daemon's messages wake the Extension.
    #[cfg(target_os = "linux")]
    thread::Builder::new()
        .name("events".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            loop {
                let (hyprland, mut stream) =
                    Hyprland::wait(|err| log!("waiting for Hyprland: {err}"));
                // Events are followed before the windows are read, so none is
                // missed in between; one about a window read already changes
                // nothing.
                if events.send(Event::Own(Own::Up(hyprland))).is_err() {
                    return;
                }
                while let Some(change) = stream.next_change() {
                    if events.send(Event::Own(Own::Change(change))).is_err() {
                        return;
                    }
                }
                if events.send(Event::Own(Own::Lost)).is_err() {
                    return;
                }
            }
        })?;
    #[cfg(not(target_os = "linux"))]
    drop(events);

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
        if extension.windows_changed {
            extension.publish_windows()?;
        }
    }
    Ok(())
}

/// The windows and the binds are sent apart, each as what changed since it
/// was last sent, so a window event never makes the binds' Items again.
struct Extension {
    writer: Writer,
    /// `None` outside Hyprland, and once it exits.
    hyprland: Option<Hyprland>,
    windows: Windows,
    sent_windows: SentItems,
    /// Whether a window changed since the windows were last sent.
    windows_changed: bool,
    binds: Vec<Bind>,
    /// The Shortcuts Item and the binds' Items.
    sent_binds: SentItems,
    /// Whether the Shortcuts View is open, so a config reload updates it.
    view_open: bool,
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
            Event::Own(Own::Up(hyprland)) => {
                self.hyprland = Some(hyprland);
                self.read_binds();
                self.publish_binds()?;
                if let Some(windows) = self.read("j/clients", Windows::from_clients) {
                    self.windows = windows;
                }
                self.windows_changed = true;
            }
            Event::Own(Own::Change(Change::ConfigReloaded)) => {
                self.read_binds();
                self.publish_binds()?;
                if self.view_open {
                    self.writer
                        .send(&FromExtension::UpdateView(binds::view(&self.binds)))?;
                }
            }
            Event::Own(Own::Change(change)) => {
                self.windows_changed |= self.windows.apply(&change);
            }
            Event::Own(Own::Lost) => {
                log!("Hyprland exited");
                self.hyprland = None;
                self.windows = Windows::default();
                self.binds = Vec::new();
                self.publish_binds()?;
                self.windows_changed = true;
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
                if bind.mouse {
                    return Err("A mouse bind only runs with the mouse".into());
                }
                hyprland.dispatch(&bind.dispatch())?;
                Ok(Outcome::Close)
            }
            _ if item.id == SHORTCUTS && action == OPEN => {
                // Read afresh, so a bind added at runtime is there: Hyprland
                // sends no event for one.
                self.read_binds();
                self.publish_binds().map_err(|err| err.to_string())?;
                self.writer
                    .send(&FromExtension::PushView(binds::view(&self.binds)))
                    .map_err(|err| err.to_string())?;
                self.view_open = true;
                Ok(Outcome::KeepOpen)
            }
            _ => Err(format!("`{}` has no Action `{action}`", item.id)),
        }
    }

    /// Sends `request` and reads the answer with `parse`; `None`, logged,
    /// if either fails.
    fn read<T>(&self, request: &str, parse: impl FnOnce(&str) -> Result<T, String>) -> Option<T> {
        let answer = self.hyprland.as_ref()?.request(request);
        answer
            .map_err(|err| err.to_string())
            .and_then(|json| parse(&json))
            .map_err(|err| log!("{request}: {err}"))
            .ok()
    }

    /// Reads the binds again; keeps the last ones if they can't be read.
    fn read_binds(&mut self) {
        if let Some(binds) = self.read("j/binds", binds::parse) {
            self.binds = binds;
        }
    }

    /// Sends the Shortcuts Item and the binds' Items that changed: all gone
    /// outside Hyprland.
    fn publish_binds(&mut self) -> Result<(), Error> {
        let items = self
            .hyprland
            .is_some()
            .then(|| std::iter::once(binds::shortcuts_item()).chain(binds::items(&self.binds)));
        self.sent_binds
            .publish(&mut self.writer, items.into_iter().flatten())
    }

    /// Sends the windows that changed.
    fn publish_windows(&mut self) -> Result<(), Error> {
        self.windows_changed = false;
        self.sent_windows
            .publish(&mut self.writer, self.windows.items())
    }
}
