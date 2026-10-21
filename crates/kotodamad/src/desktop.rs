//! Typing and pasting into the focused window over Wayland. Text is typed
//! through the virtual-keyboard protocol with a keymap made for it, as `wtype`
//! does, so any character can be typed whatever the user's layout. A copy
//! sets the clipboard through ext-data-control, or wlr-data-control on a
//! compositor that has only that (`kotodama_extension::wayland`, shared with
//! the clipboard Extension); a paste does, then sends Ctrl+V, or Ctrl+Shift+V,
//! or types the text instead, as `[desktop] paste` says.
//!
//! The Wayland connection lives on a thread of its own, opened on the first
//! request, and stays open: the Daemon must keep serving the clipboard it set
//! until something else takes it. The compositor is found without
//! `WAYLAND_DISPLAY` too (`kotodama_extension::wayland::Display`), so a Daemon
//! started before the compositor's environment reached it still types; a
//! request that finds no compositor fails, and the next one tries again.
//!
//! Keys wait `[desktop] settle_ms` after the menu closes before they go out. The window
//! that gets the keyboard back may not take keys at once: `QtWayland` makes a
//! window active only after a round trip that follows `wl_keyboard.enter`,
//! and drops keys until then. Nothing on Wayland tells another client when
//! that is done. `zwp_input_method_v2` would deliver text without key events,
//! but a seat has one input method, which the user's own (`fcitx`, `IBus`) holds,
//! and many apps never enable a text input, so it can't replace keys. The
//! wait is the least bad bound: long enough for a client woken by `enter` to
//! get the CPU and answer, short enough not to be seen.

use std::fmt::Write as _;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::Chain;

use crate::daemon::Desktop;
use crate::daemon::Output;
use crate::engine::DesktopSettings;
use crate::host::spawn;

/// The keymap a virtual keyboard uploads to type some text: one key per
/// distinct character, so the keymap stays small.
#[derive(Debug)]
pub struct Keymap {
    chars: Vec<char>,
}

impl Keymap {
    pub fn new(text: &str) -> Self {
        let mut chars: Vec<char> = Vec::new();
        for c in text.chars() {
            if !chars.contains(&c) {
                chars.push(c);
            }
        }
        Keymap { chars }
    }

    /// The evdev key code that types `c`.
    pub fn code(&self, c: char) -> Option<u32> {
        self.chars.iter().position(|&known| known == c).map(|at| at as u32 + 1)
    }

    /// The keymap in XKB's text format. Key code `n` is XKB key code `n + 8`.
    pub fn to_xkb(&self) -> String {
        let mut xkb = String::from("xkb_keymap {\nxkb_keycodes \"(unnamed)\" {\nminimum = 8;\n");
        let _ = writeln!(xkb, "maximum = {};", self.chars.len() + 9);
        for n in 1..=self.chars.len() {
            let _ = writeln!(xkb, "<K{n}> = {};", n + 8);
        }
        xkb.push_str(
            "};\nxkb_types \"(unnamed)\" { include \"complete\" };\n\
             xkb_compatibility \"(unnamed)\" { include \"complete\" };\n\
             xkb_symbols \"(unnamed)\" {\n",
        );
        for (n, &c) in self.chars.iter().enumerate() {
            let _ = writeln!(xkb, "key <K{}> {{[{}]}};", n + 1, keysym(c));
        }
        xkb.push_str("};\n};\n");
        xkb
    }
}

fn keysym(c: char) -> String {
    match c {
        '\n' => "Return".into(),
        '\t' => "Tab".into(),
        c => format!("U{:04X}", c as u32),
    }
}

/// The modifier masks for Shift and Control in the standard XKB modifier
/// order.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SHIFT: u32 = 1;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const CONTROL: u32 = 1 << 2;

/// How much longer keys wait, `settle` after the request to send them came.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn settle_left(requested: Instant, settle: Duration, now: Instant) -> Duration {
    (requested + settle).saturating_duration_since(now)
}

/// Starts `command` as an Extension starts a program
/// (`kotodama_extension::launch::launch`): in a systemd scope of its own
/// when it can have one, and reaped when it exits. A thread of its own
/// starts it, because starting waits until the user manager has made the
/// scope, and the Daemon never waits for the user manager.
fn launch(command: Vec<String>) {
    let Some(program) = command.first() else {
        return;
    };
    spawn(format!("launch-{program}"), move || {
        if let Err(err) = kotodama_extension::launch::launch(&command, None) {
            log::error!("{err}");
        }
    });
}

/// Sends a desktop notification on a thread of its own, so a slow session bus
/// never holds up the Daemon.
fn notify(summary: String, body: String) {
    spawn("notify".into(), move || {
        if let Err(err) = crate::notify::send(&summary, &body) {
            log::warn!("couldn't show the notification “{summary}”: {}", Chain(&err));
        }
    });
}

/// Types, pastes and copies on the Wayland display in `WAYLAND_DISPLAY`.
#[derive(Debug, Default)]
pub struct Wayland {
    #[cfg(target_os = "linux")]
    worker: Option<linux::Worker>,
    settings: DesktopSettings,
}

impl Wayland {
    pub fn new() -> Self {
        Wayland::default()
    }
}

#[cfg(target_os = "linux")]
impl Desktop for Wayland {
    fn configure(&mut self, settings: DesktopSettings) {
        self.settings = settings;
    }

    fn launch(&mut self, command: Vec<String>) {
        launch(command);
    }

    fn notify(&mut self, summary: String, body: String) {
        notify(summary, body);
    }

    fn output(&mut self, output: Output) {
        if self.worker.is_none() {
            self.worker = linux::Worker::start()
                .map_err(|err| log::error!("can't type or paste: {err}"))
                .ok();
        }
        if let Some(worker) = &self.worker
            && !worker.send(output, self.settings)
        {
            self.worker = None;
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Desktop for Wayland {
    fn configure(&mut self, settings: DesktopSettings) {
        self.settings = settings;
    }

    fn launch(&mut self, command: Vec<String>) {
        launch(command);
    }

    fn notify(&mut self, summary: String, body: String) {
        notify(summary, body);
    }

    fn output(&mut self, _output: Output) {
        log::error!("typing, pasting and copying need Wayland, which only Linux has");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::AsFd;
    use std::os::fd::OwnedFd;
    use std::sync::mpsc;
    use std::sync::mpsc::Receiver;
    use std::sync::mpsc::Sender;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use kotodama_extension::wayland::DataControl;
    use kotodama_extension::wayland::DataControlHandler;
    use kotodama_extension::wayland::Display;
    use kotodama_protocol::Clipboard;
    use rustix::event::PollFd;
    use rustix::event::PollFlags;
    use wayland_client::Connection;
    use wayland_client::Dispatch;
    use wayland_client::EventQueue;
    use wayland_client::QueueHandle;
    use wayland_client::delegate_noop;
    use wayland_client::globals::GlobalListContents;
    use wayland_client::globals::registry_queue_init;
    use wayland_client::protocol::wl_keyboard::KeyState;
    use wayland_client::protocol::wl_registry;
    use wayland_client::protocol::wl_seat;
    use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
    use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

    use super::CONTROL;
    use super::Keymap;
    use super::Output;
    use super::SHIFT;
    use super::settle_left;
    use crate::engine::DesktopSettings;
    use crate::engine::Paste;
    use crate::host::spawn;

    /// What to do, when the Daemon asked, once the menu had closed, and how.
    type Request = (Output, Instant, DesktopSettings);

    /// The handle the Daemon holds: requests go down a channel, and a byte
    /// down a pipe wakes the thread, which also waits on the Wayland socket.
    #[derive(Debug)]
    pub struct Worker {
        requests: Sender<Request>,
        wake: OwnedFd,
    }

    impl Worker {
        pub fn start() -> Result<Self, String> {
            let (wake_rx, wake) = rustix::pipe::pipe().map_err(|err| err.to_string())?;
            let (requests, inbox) = mpsc::channel();
            spawn("desktop".into(), move || run(&inbox, &wake_rx));
            Ok(Worker { requests, wake })
        }

        /// False once the thread has stopped.
        pub fn send(&self, output: Output, settings: DesktopSettings) -> bool {
            self.requests.send((output, Instant::now(), settings)).is_ok()
                && rustix::io::write(&self.wake, &[1]).is_ok()
        }
    }

    fn run(inbox: &Receiver<Request>, wake: &OwnedFd) {
        let mut client: Option<Client> = None;
        loop {
            // Wait for a request, or for the compositor while we hold the
            // clipboard and may be asked for its contents.
            let guard = client.as_mut().and_then(|client| {
                let _ = client.queue.dispatch_pending(&mut client.state);
                let _ = client.queue.flush();
                client.queue.prepare_read()
            });
            let (woken, readable) = {
                let mut fds = vec![PollFd::new(wake, PollFlags::IN)];
                if let Some(guard) = &guard {
                    fds.push(PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN));
                }
                if rustix::event::poll(&mut fds, None).is_err() {
                    continue;
                }
                let ready = |fd: &PollFd| !fd.revents().is_empty();
                (ready(&fds[0]), fds.get(1).is_some_and(ready))
            };
            if let Some(guard) = guard {
                if readable {
                    if guard.read().is_err() {
                        log::warn!("lost the Wayland connection");
                        client = None;
                    }
                } else {
                    drop(guard);
                }
            }
            if woken {
                let mut buf = [0; 64];
                if matches!(rustix::io::read(wake, &mut buf), Ok(0)) {
                    return;
                }
                for (output, requested, settings) in inbox.try_iter() {
                    if client.is_none() {
                        client = Client::connect()
                            .map_err(|err| log::error!("can't type or paste: {err}"))
                            .ok();
                    }
                    if let Some(c) = &mut client
                        && let Err(err) = c.output(output, requested, settings)
                    {
                        log::error!("can't type or paste: {err}");
                        client = None;
                    }
                }
            }
        }
    }

    struct Client {
        queue: EventQueue<State>,
        state: State,
        qh: QueueHandle<State>,
        keyboard: ZwpVirtualKeyboardV1,
        data: Result<DataControl, String>,
        start: Instant,
    }

    /// Nothing to keep: what we put on the clipboard lives with its source
    /// (`kotodama_extension::wayland::SourceData`).
    struct State;

    impl Client {
        fn connect() -> Result<Self, String> {
            let conn = Display::from_env().connect()?;
            let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|err| err.to_string())?;
            let qh = queue.handle();
            let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|err| format!("no seat: {err}"))?;
            let manager: ZwpVirtualKeyboardManagerV1 = globals
                .bind(&qh, 1..=1, ())
                .map_err(|err| format!("the compositor has no virtual keyboards: {err}"))?;
            let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
            // Typing needs no clipboard, so it works without one.
            let data = DataControl::bind(&globals, &seat, &qh);
            let mut state = State;
            queue.roundtrip(&mut state).map_err(|err| err.to_string())?;
            Ok(Client {
                queue,
                state,
                qh,
                keyboard,
                data,
                start: Instant::now(),
            })
        }

        /// Does what was asked at `requested`, once the menu had closed.
        fn output(&mut self, output: Output, requested: Instant, settings: DesktopSettings) -> Result<(), String> {
            let wait = Wait {
                requested,
                settle: settings.settle,
            };
            match (output, settings.paste) {
                (Output::Type(text), _) | (Output::Paste(Clipboard::Text(text)), Paste::Type) => {
                    self.type_text(&text, wait)
                }
                (Output::Paste(clipboard), paste) => self.paste(clipboard, paste, wait),
                (Output::Copy(clipboard), _) => self.copy(clipboard),
            }
        }

        fn type_text(&mut self, text: &str, wait: Wait) -> Result<(), String> {
            let keymap = Keymap::new(text);
            self.upload(&keymap)?;
            wait.settle();
            for c in text.chars() {
                if let Some(code) = keymap.code(c) {
                    self.tap(code);
                }
            }
            self.roundtrip()
        }

        fn paste(&mut self, clipboard: Clipboard, paste: Paste, wait: Wait) -> Result<(), String> {
            self.copy(clipboard)?;
            let keymap = Keymap::new("v");
            self.upload(&keymap)?;
            wait.settle();
            let modifiers = match paste {
                Paste::CtrlShiftV => CONTROL | SHIFT,
                Paste::CtrlV | Paste::Type => CONTROL,
            };
            self.keyboard.modifiers(modifiers, 0, 0, 0);
            self.tap(keymap.code('v').unwrap_or(1));
            self.keyboard.modifiers(0, 0, 0, 0);
            self.roundtrip()
        }

        /// Puts `clipboard` on the clipboard, and serves it until something
        /// else takes it.
        fn copy(&mut self, clipboard: Clipboard) -> Result<(), String> {
            let data_control = self.data.as_ref().map_err(|err| format!("can't copy: {err}"))?;
            let (mimes, data) = match clipboard {
                Clipboard::Text(text) => (
                    vec![
                        "text/plain;charset=utf-8".to_owned(),
                        "text/plain".into(),
                        "UTF8_STRING".into(),
                        "STRING".into(),
                        "TEXT".into(),
                    ],
                    text.into_bytes(),
                ),
                Clipboard::Image { mime, data } => (vec![mime], data.0),
            };
            data_control.set_selection(mimes, data, &self.qh);
            let protocol = data_control.protocol();
            self.roundtrip()?;
            log::debug!("copied to the clipboard through {protocol}");
            Ok(())
        }

        fn upload(&mut self, keymap: &Keymap) -> Result<(), String> {
            let mut xkb = keymap.to_xkb().into_bytes();
            xkb.push(0);
            let fd = rustix::fs::memfd_create("kotodama-keymap", rustix::fs::MemfdFlags::CLOEXEC)
                .map_err(|err| err.to_string())?;
            let mut file = File::from(fd);
            file.write_all(&xkb).map_err(|err| err.to_string())?;
            // 1 is WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1.
            self.keyboard.keymap(1, file.as_fd(), xkb.len() as u32);
            // Let the compositor take the keymap before keys arrive with it.
            self.roundtrip()
        }

        fn tap(&self, code: u32) {
            let time = self.start.elapsed().as_millis() as u32;
            self.keyboard.key(time, code, KeyState::Pressed.into());
            self.keyboard.key(time, code, KeyState::Released.into());
        }

        fn roundtrip(&mut self) -> Result<(), String> {
            self.queue
                .roundtrip(&mut self.state)
                .map(drop)
                .map_err(|err| err.to_string())
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    /// When the Daemon asked, and how long keys wait after that.
    #[derive(Clone, Copy)]
    struct Wait {
        requested: Instant,
        settle: Duration,
    }

    impl Wait {
        /// Waits out what is left of it.
        fn settle(self) {
            thread::sleep(settle_left(self.requested, self.settle, Instant::now()));
        }
    }

    /// The Daemon never reads the clipboard: every offer is dropped as it
    /// comes.
    impl DataControlHandler for State {}

    delegate_noop!(State: ignore wl_seat::WlSeat);
    delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
    delegate_noop!(State: ZwpVirtualKeyboardV1);
    kotodama_extension::delegate_data_control!(State);
}

#[cfg(test)]
mod tests;
