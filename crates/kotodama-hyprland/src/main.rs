//! The Hyprland Bundled extension. Hyprland's open windows are Items of the
//! shared kind `window`, read once and then kept up to date from its event
//! socket, sending only the windows that changed. Its binds with a
//! description are Items that run the bind; a `Keyboard Shortcuts` Item opens
//! a View of every bind, grouped by modifier, read afresh each time it opens,
//! so `kotodama open --view hyprland/shortcuts` opens on it. A bind on a
//! keycode is shown by its key's name under the current layout, from the
//! keymap Hyprland hands Wayland clients, fetched only when such a bind is
//! read and its name isn't known yet. Under Hyprland's Lua config, binds and
//! window Actions dispatch Lua. Outside Hyprland
//! the Extension sends nothing and waits, at no cost, for a Hyprland to start
//! (in `$XDG_RUNTIME_DIR/hypr/`, see `Hyprland::wait`) or the Daemon to hang
//! up; when Hyprland restarts it follows the new instance.
//!
//! # Owns
//!
//! - The protocol loop, and the thread that follows Hyprland.
//! - When the windows and the binds are read and sent, and running their Actions.
//!
//! # Not here
//!
//! - Hyprland's sockets, events, windows and binds: the library's modules.
//! - Sending only the Items that changed: `kotodama_extension::SentItems`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::io;
use std::process::ExitCode;
#[cfg(target_os = "linux")]
use std::sync::mpsc::Sender;
#[cfg(target_os = "linux")]
use std::thread;

use kotodama_extension::Chain;
use kotodama_extension::Connection;
use kotodama_extension::Event;
#[cfg(target_os = "linux")]
use kotodama_extension::STACK_SIZE;
use kotodama_extension::SentItems;
use kotodama_extension::SentStatus;
use kotodama_extension::Writer;
use kotodama_extension::log;
use kotodama_hyprland::binds;
use kotodama_hyprland::binds::Bind;
use kotodama_hyprland::binds::OPEN;
use kotodama_hyprland::binds::RUN;
use kotodama_hyprland::binds::SHORTCUTS;
use kotodama_hyprland::events::Change;
use kotodama_hyprland::hyprland::Hyprland;
use kotodama_hyprland::hyprland::Language;
use kotodama_hyprland::keymap;
use kotodama_hyprland::keymap::Keymap;
use kotodama_hyprland::windows;
use kotodama_hyprland::windows::WORKSPACE;
use kotodama_hyprland::windows::Windows;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;
use kotodama_protocol::ItemKind;
use kotodama_protocol::ItemRef;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;

/// Why the Extension can't do anything outside Hyprland, which its app's
/// intro card shows.
const NOT_RUNNING: &str = "Hyprland isn't running";

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-hyprland", run)
}

/// What the event thread sends.
#[cfg_attr(
    not(target_os = "linux"),
    expect(dead_code, reason = "only the event thread sends these, and it runs on Linux alone")
)]
enum Own {
    /// A Hyprland is running, and its events are followed.
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
        language: Language::default(),
        windows: Windows::default(),
        sent_windows: SentItems::default(),
        windows_changed: false,
        binds: Vec::new(),
        key_names: HashMap::new(),
        sent_binds: SentItems::default(),
        view_open: false,
        status: SentStatus::default(),
    };
    extension.status.unusable(&mut extension.writer, NOT_RUNNING)?;
    #[cfg(target_os = "linux")]
    follow_hyprland(events).map_err(Error::Io)?;
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

/// Starts the thread that waits for Hyprland, follows its events, and once it exits waits for the
/// next one: it may start after the Extension, or restart under a new signature. Waiting costs
/// nothing, so outside Hyprland only the Daemon's messages wake the Extension.
#[cfg(target_os = "linux")]
fn follow_hyprland(events: Sender<Event<Own>>) -> io::Result<()> {
    thread::Builder::new()
        .name("events".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            loop {
                let (hyprland, mut stream) = Hyprland::wait(|err| log!("waiting for Hyprland: {err}"));
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
    Ok(())
}

/// The windows and the binds are sent apart, each as what changed since it
/// was last sent, so a window event never makes the binds' Items again.
struct Extension {
    writer: Writer,
    /// `None` outside Hyprland, and once it exits.
    hyprland: Option<Hyprland>,
    /// The language of the running Hyprland's dispatches, asked once when it
    /// is found: Hyprland can't change it without restarting.
    language: Language,
    windows: Windows,
    sent_windows: SentItems,
    /// Whether a window changed since the windows were last sent.
    windows_changed: bool,
    binds: Vec<Bind>,
    /// The names of the keys the binds on a keycode use, by keycode, `None`
    /// for one the keymap doesn't name. Forgotten when the config or a
    /// layout changes, and fetched again when the binds are next read.
    key_names: HashMap<u32, Option<String>>,
    /// The Shortcuts Item and the binds' Items.
    sent_binds: SentItems,
    /// Whether the Shortcuts View is open, so a config reload updates it.
    view_open: bool,
    /// Usable while a Hyprland is running.
    status: SentStatus,
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
                self.writer.send(&FromExtension::ActionDone { invocation, outcome })?;
            }
            Event::Message(ToExtension::ViewPopped { view }) if view == binds::VIEW => {
                self.view_open = false;
            }
            Event::Message(_) => {}
            Event::Own(Own::Up(hyprland)) => {
                self.language = hyprland.language();
                self.hyprland = Some(hyprland);
                self.key_names.clear();
                self.read_binds();
                self.publish_binds()?;
                if let Some(windows) = self.read("j/clients", Windows::from_clients) {
                    self.windows = windows;
                }
                self.windows_changed = true;
                self.status.usable(&mut self.writer)?;
            }
            Event::Own(Own::Change(Change::ConfigReloaded)) => {
                self.key_names.clear();
                self.read_binds();
                self.publish_binds()?;
                if self.view_open {
                    self.writer.send(&FromExtension::UpdateView(binds::view(&self.binds)))?;
                }
            }
            // Named again when the binds are next read, not on the event: a virtual
            // keyboard, as the Daemon types with, sends this too, and while
            // it was last used its keymap is the one Hyprland hands out.
            Event::Own(Own::Change(Change::LayoutChanged)) => self.key_names.clear(),
            Event::Own(Own::Change(change)) => {
                self.windows_changed |= self.windows.apply(&change);
            }
            Event::Own(Own::Lost) => {
                log!("Hyprland exited");
                self.hyprland = None;
                self.windows = Windows::default();
                self.binds = Vec::new();
                self.key_names.clear();
                self.publish_binds()?;
                self.windows_changed = true;
                self.status.unusable(&mut self.writer, NOT_RUNNING)?;
            }
            Event::Hangup => return Ok(false),
        }
        Ok(true)
    }

    fn run(&mut self, action: &str, item: &ItemRef, arguments: &BTreeMap<String, String>) -> Result<Outcome, String> {
        let hyprland = self.hyprland.as_ref().ok_or(NOT_RUNNING)?;
        match &item.kind {
            ItemKind::Window { address } => {
                let workspace = arguments.get(WORKSPACE).map(String::as_str);
                let dispatch =
                    windows::dispatch(action, address, workspace, self.language).map_err(|err| err.to_string())?;
                hyprland.dispatch(&dispatch).map_err(|err| Chain(&err).to_string())?;
                Ok(Outcome::Close)
            }
            kind if kind.name() == binds::KIND && action == RUN => {
                let bind = binds::find(&self.binds, &item.id).ok_or("That shortcut is no longer bound in Hyprland")?;
                if bind.mouse {
                    return Err("A mouse bind only runs with the mouse".into());
                }
                let dispatch = bind.dispatch().ok_or("Hyprland gives this shortcut nothing to run")?;
                hyprland.dispatch(&dispatch).map_err(|err| Chain(&err).to_string())?;
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
    fn read<T, E>(&self, request: &str, parse: impl FnOnce(&str) -> Result<T, E>) -> Option<T>
    where
        E: std::error::Error,
    {
        let answer = self.hyprland.as_ref()?.request(request);
        answer
            .map_err(|err| err.to_string())
            .and_then(|json| parse(&json).map_err(|err| Chain(&err).to_string()))
            .map_err(|err| log!("{request}: {err}"))
            .ok()
    }

    /// Reads the binds again, and names the keys of those on a keycode;
    /// keeps the last ones if they can't be read.
    fn read_binds(&mut self) {
        let Some(mut binds) = self.read("j/binds", binds::parse) else {
            return;
        };
        if binds.iter().any(Bind::lacks_keycode)
            && let Some(hyprland) = &self.hyprland
        {
            match hyprland.request("/binds") {
                Ok(listing) => binds::keycodes_from_listing(&mut binds, &listing),
                Err(err) => log!("binds: {err}"),
            }
        }
        let codes: Vec<u32> = binds
            .iter()
            .filter(|bind| bind.on_keycode())
            .map(|bind| bind.keycode)
            .collect();
        if codes.iter().any(|code| !self.key_names.contains_key(code)) {
            self.key_names = self.name_keycodes(&codes);
        }
        binds::name_keys(&mut binds, |code| self.key_names.get(&code).cloned().flatten());
        self.binds = binds;
    }

    /// The names of the keys `codes` under the main keyboard's current
    /// layout, from the keymap Hyprland hands Wayland clients; each `None`
    /// without a keymap.
    fn name_keycodes(&self, codes: &[u32]) -> HashMap<u32, Option<String>> {
        let layout = self
            .hyprland
            .as_ref()
            .and_then(|hyprland| hyprland.request("j/devices").ok())
            .and_then(|devices| keymap::active_layout(&devices))
            .unwrap_or(0);
        let text = self
            .keymap()
            .map_err(|err| log!("naming the keys of the binds on a keycode: {err}"))
            .ok();
        let keymap = text.as_deref().map(Keymap::parse).unwrap_or_default();
        codes
            .iter()
            .map(|&code| (code, keymap.keysym(code, layout).map(str::to_owned)))
            .collect()
    }

    /// The keymap Hyprland hands Wayland clients.
    #[cfg(target_os = "linux")]
    fn keymap(&self) -> Result<String, String> {
        let socket = self
            .hyprland
            .as_ref()
            .and_then(Hyprland::wayland_socket)
            .ok_or("Hyprland names no Wayland socket")?;
        kotodama_hyprland::keyboard::keymap(&socket).map_err(|err| Chain(&err).to_string())
    }

    #[cfg(not(target_os = "linux"))]
    #[expect(clippy::unused_self, reason = "the Linux one reads the running Hyprland's socket")]
    fn keymap(&self) -> Result<String, String> {
        Err("the keymap is read on Linux only".to_owned())
    }

    /// Sends the Shortcuts Item and the binds' Items that changed: all gone
    /// outside Hyprland.
    fn publish_binds(&mut self) -> Result<(), Error> {
        let items = self
            .hyprland
            .is_some()
            .then(|| std::iter::once(binds::shortcuts_item()).chain(binds::items(&self.binds)));
        self.sent_binds.publish(&mut self.writer, items.into_iter().flatten())
    }

    /// Sends the windows that changed.
    fn publish_windows(&mut self) -> Result<(), Error> {
        self.windows_changed = false;
        self.sent_windows.publish(&mut self.writer, self.windows.items())
    }
}
