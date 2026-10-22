//! The keymap Hyprland hands Wayland clients: the one of its main keyboard,
//! with every layout configured, as the `wl_keyboard.keymap` event sends it
//! to a client that asks for the seat's keyboard.
//!
//! The connection is made for one keymap and closed again, so the Extension
//! holds no Wayland connection, and the keymap's text lives only while the
//! binds' keys are named (see `crate::keymap`). It is made on the calling
//! thread: a thread of its own would leave its allocator arena resident.
//!
//! # Owns
//!
//! - Connecting to Hyprland's Wayland socket, getting the keyboard's keymap, and giving up after
//!   [`TIMEOUT`].
//!
//! # Not here
//!
//! - Which socket: [`crate::hyprland::Hyprland::wayland_socket`].
//! - Reading the keymap: [`crate::keymap`].

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use rustix::event::PollFd;
use rustix::event::PollFlags;
use rustix::event::Timespec;
use rustix::io::Errno;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::EventQueue;
use wayland_client::QueueHandle;
use wayland_client::WEnum;
use wayland_client::protocol::wl_callback;
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_keyboard;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat;
use wayland_client::protocol::wl_seat::WlSeat;

/// How long the keymap may take before the binds are named without it, so a
/// hung Hyprland can't hang the Extension. A request on Hyprland's socket
/// waits as long.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Why there is no keymap.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("couldn't reach Hyprland's Wayland socket")]
    Unreachable(#[source] io::Error),
    #[error("Wayland: {0}")]
    Wayland(String),
    #[error("Hyprland's seat has no keyboard")]
    NoKeyboard,
    /// The keyboard sent no keymap, or one that isn't xkb's text.
    #[error("Hyprland's keyboard has no keymap")]
    NoKeymap,
    #[error("couldn't read the keymap")]
    Read(#[source] io::Error),
    #[error("Hyprland didn't send the keymap in time")]
    TimedOut,
}

impl Error {
    fn unreachable(err: io::Error) -> Self {
        Error::Unreachable(err)
    }

    fn wayland(err: &dyn std::fmt::Display) -> Self {
        Error::Wayland(err.to_string())
    }

    fn read(err: io::Error) -> Self {
        Error::Read(err)
    }
}

/// The keymap of the keyboard of the first seat on `socket`, in xkb's text
/// format.
///
/// # Errors
///
/// [`Error`] when the socket takes no connection, the seat has no keyboard
/// or the keyboard no keymap, or Hyprland takes longer than [`TIMEOUT`].
pub fn keymap(socket: &Path) -> Result<String, Error> {
    let deadline = Instant::now() + TIMEOUT;
    let stream = UnixStream::connect(socket).map_err(Error::unreachable)?;
    let connection = Connection::from_socket(stream).map_err(|err| Error::wayland(&err))?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let mut state = State::default();
    roundtrip(&connection, &mut queue, &mut state, deadline)?;
    let seat = state.seat.ok_or(Error::NoKeyboard)?;
    let seat: WlSeat = registry.bind(seat, 1, &qh, ());
    // A seat that never had a keyboard refuses to make one, so its
    // capabilities come first.
    roundtrip(&connection, &mut queue, &mut state, deadline)?;
    if !state.keyboard {
        return Err(Error::NoKeyboard);
    }
    let _keyboard = seat.get_keyboard(&qh, ());
    roundtrip(&connection, &mut queue, &mut state, deadline)?;
    state.keymap.unwrap_or(Err(Error::NoKeymap))
}

/// Dispatches events until Hyprland has answered every request sent so far,
/// or `deadline` passes: the Wayland crate's own roundtrip waits forever.
fn roundtrip(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    deadline: Instant,
) -> Result<(), Error> {
    state.synced = false;
    connection.display().sync(&queue.handle(), ());
    loop {
        queue.dispatch_pending(state).map_err(|err| Error::wayland(&err))?;
        if state.synced {
            return Ok(());
        }
        queue.flush().map_err(|err| Error::wayland(&err))?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let left = deadline.checked_duration_since(Instant::now()).ok_or(Error::TimedOut)?;
        let timeout = Timespec::try_from(left).map_err(|_| Error::TimedOut)?;
        let mut fds = [PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(0) => return Err(Error::TimedOut),
            Ok(_) => {
                guard.read().map_err(|err| Error::wayland(&err))?;
            }
            Err(Errno::INTR) => {}
            Err(errno) => return Err(Error::unreachable(io::Error::from(errno))),
        }
    }
}

/// What the event queue dispatches to.
#[derive(Default)]
struct State {
    /// The name of the first `wl_seat` global.
    seat: Option<u32>,
    /// Whether the seat has a keyboard.
    keyboard: bool,
    keymap: Option<Result<String, Error>>,
    /// Whether the last roundtrip's `sync` is done.
    synced: bool,
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event
            && interface == "wl_seat"
        {
            state.seat.get_or_insert(name);
        }
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(state: &mut Self, _: &WlSeat, event: wl_seat::Event, (): &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            state.keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap {
            format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
            fd,
            size,
        } = event
        {
            state.keymap = Some(read(&File::from(fd), size));
        }
    }
}

/// Reads `size` bytes of the keymap's file, from its start, without moving
/// the file's offset: Hyprland hands every client the same open file.
fn read(file: &File, size: u32) -> Result<String, Error> {
    let size = usize::try_from(size).map_err(|_| Error::NoKeymap)?;
    let mut text = vec![0; size];
    file.read_exact_at(&mut text, 0).map_err(Error::read)?;
    // The text ends with a NUL, which the size counts.
    if text.last() == Some(&0) {
        text.pop();
    }
    String::from_utf8(text).map_err(|err| Error::read(io::Error::new(io::ErrorKind::InvalidData, err)))
}
