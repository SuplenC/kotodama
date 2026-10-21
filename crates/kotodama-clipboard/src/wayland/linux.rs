//! The Wayland clipboard on Linux: the Wayland thread, which owns the
//! connection, and the read thread, which reads each offer off its source.
//!
//! # Owns
//!
//! - Both threads, and the requests [`Clipboard::set`] hands the Wayland thread through a channel
//!   and a wake-up pipe.
//!
//! # Not here
//!
//! - The platform-neutral types and errors: `wayland.rs`.

use std::fs::File;
use std::io;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::Chain;
use kotodama_extension::wayland::DataControl;
use kotodama_extension::wayland::DataControlHandler;
use kotodama_extension::wayland::Display;
use kotodama_extension::wayland::Offer;
use rustix::event::PollFd;
use rustix::event::PollFlags;
use rustix::event::Timespec;
use rustix::io::Errno;
use rustix::pipe::PipeFlags;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::EventQueue;
use wayland_client::QueueHandle;
use wayland_client::delegate_noop;
use wayland_client::globals::GlobalListContents;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat;

use super::Change;
use super::SetError;
use super::WatchError;
use crate::copied::Choice;
use crate::copied::Copied;
use crate::copied::choose;

/// The most bytes one copy may have; anything bigger isn't kept. Well
/// under the protocol's 64 MiB frame, so it can always be pasted back.
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// How long a source may go without sending a byte before reading it is
/// given up.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The stack of each thread here. Neither recurses or keeps much on its
/// stack, so both need far less than std's default of 2 MiB.
const STACK: usize = 256 * 1024;

/// What to put on the clipboard, until when (`set` has given up after),
/// and where to say it is there.
type Request = (Vec<String>, Vec<u8>, Instant, Sender<()>);

/// The watched clipboard. Dropping it stops the watching, once the
/// compositor is there.
#[derive(Debug)]
pub struct Clipboard {
    requests: Sender<Request>,
    /// The write end of the pipe that wakes the Wayland thread for a request.
    wake: OwnedFd,
}

impl Clipboard {
    /// Calls `changed` with each thing copied after this call, on a thread
    /// of its own, which first waits for the compositor if it isn't there
    /// yet.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when a thread or the pipe that wakes it can't be made.
    /// A compositor that is missing, or has no data-control, is logged on
    /// the thread instead.
    pub fn watch(changed: impl Fn(Change) + Send + Sync + 'static) -> Result<Clipboard, WatchError> {
        let (reads, to_read) = mpsc::channel::<(String, OwnedFd)>();
        let changed = Arc::new(changed);
        let reporter = Arc::clone(&changed);
        thread::Builder::new()
            .name("read".into())
            .stack_size(STACK)
            .spawn(move || read_all(&to_read, &*reporter))
            .map_err(WatchError::Thread)?;

        let (wake_rx, wake) = rustix::pipe::pipe_with(PipeFlags::CLOEXEC).map_err(WatchError::pipe)?;
        let (requests, inbox) = mpsc::channel();
        thread::Builder::new()
            .name("wayland".into())
            .stack_size(STACK)
            .spawn(move || {
                follow(&reads, &inbox, &wake_rx);
                changed(Change::Lost);
            })
            .map_err(WatchError::Thread)?;
        Ok(Clipboard { requests, wake })
    }

    /// Puts `data` on the clipboard, offered as each of `mimes`, and
    /// returns once the compositor has it there, so what is copied next
    /// comes after it. The clipboard keeps it until something else is
    /// copied.
    ///
    /// # Errors
    ///
    /// [`SetError`] when the compositor doesn't answer within
    /// `READ_TIMEOUT`, isn't there yet, or the connection to it is lost.
    pub fn set(&self, mimes: Vec<String>, data: Vec<u8>) -> Result<(), SetError> {
        let (done, copied) = mpsc::channel();
        if self
            .requests
            .send((mimes, data, Instant::now() + READ_TIMEOUT, done))
            .is_err()
            || rustix::io::write(&self.wake, &[1]).is_err()
        {
            return Err(SetError::Lost);
        }
        match copied.recv_timeout(READ_TIMEOUT) {
            Ok(()) => Ok(()),
            Err(RecvTimeoutError::Timeout) => Err(SetError::NoAnswer),
            Err(RecvTimeoutError::Disconnected) => Err(SetError::Lost),
        }
    }
}

impl WatchError {
    fn pipe(errno: Errno) -> Self {
        WatchError::Pipe(io::Error::from(errno))
    }
}

/// Waits for the compositor, then watches the clipboard on it and sets it
/// when asked, until the [`Clipboard`] is dropped or the connection is lost.
/// Runs on the Wayland thread.
fn follow(reads: &Sender<(String, OwnedFd)>, inbox: &Receiver<Request>, wake: &OwnedFd) {
    let mut waited = false;
    let connected = Display::from_env().wait(
        |conn| connect(&conn, reads.clone()),
        |err| {
            waited = true;
            kotodama_extension::log!("waiting for the compositor: {err}");
        },
    );
    let (queue, state) = match connected {
        Ok(connected) => connected,
        // Waiting longer won't help.
        Err(err) => {
            kotodama_extension::log!("not watching the clipboard: {err}");
            return;
        }
    };
    if waited {
        kotodama_extension::log!("watching the clipboard");
    }
    run(queue, state, inbox, wake);
}

/// Binds the clipboard on a new connection, and takes in what it holds.
/// The outer error is worth another try; the inner one, a compositor
/// with no data-control, is not.
fn connect(
    conn: &Connection,
    reads: Sender<(String, OwnedFd)>,
) -> Result<Result<(EventQueue<State>, State), String>, String> {
    let (globals, mut queue) = registry_queue_init::<State>(conn).map_err(|err| err.to_string())?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|err| format!("no seat: {err}"))?;
    let control = match DataControl::bind(&globals, &seat, &qh) {
        Ok(control) => control,
        Err(err) => return Ok(Err(err)),
    };
    let mut state = State {
        control,
        reads,
        finished: false,
    };
    queue.roundtrip(&mut state).map_err(|err| err.to_string())?;
    Ok(Ok((queue, state)))
}

/// Dispatches Wayland events and sets the clipboard when asked, until
/// the [`Clipboard`] is dropped or the connection is lost.
fn run(mut queue: EventQueue<State>, mut state: State, inbox: &Receiver<Request>, wake: &OwnedFd) {
    let qh = queue.handle();
    loop {
        if queue.dispatch_pending(&mut state).is_err() || state.finished {
            return;
        }
        if queue.flush().is_err() {
            return;
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let (woken, readable) = {
            let mut fds = [
                PollFd::new(wake, PollFlags::IN),
                PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN),
            ];
            if rustix::event::poll(&mut fds, None).is_err() {
                continue;
            }
            (!fds[0].revents().is_empty(), !fds[1].revents().is_empty())
        };
        if readable {
            if guard.read().is_err() {
                kotodama_extension::log!("lost the Wayland connection");
                return;
            }
        } else {
            drop(guard);
        }
        if woken {
            let mut buf = [0; 64];
            if matches!(rustix::io::read(wake, &mut buf), Ok(0) | Err(_)) {
                return;
            }
            let mut waiting = Vec::new();
            let now = Instant::now();
            for (mimes, data, until, done) in inbox.try_iter() {
                // Asked before the compositor was there, and failed since:
                // setting it at this point would undo a later copy.
                if until < now {
                    continue;
                }
                state.control.set_selection(mimes, data, &qh);
                waiting.push(done);
            }
            // A round trip: the compositor has handled the requests sent.
            if !waiting.is_empty() {
                if queue.roundtrip(&mut state).is_err() {
                    kotodama_extension::log!("lost the Wayland connection");
                    return;
                }
                for done in waiting {
                    let _ = done.send(());
                }
            }
        }
    }
}

/// Reads each offer handed over, one at a time, and reports what it
/// becomes.
fn read_all(to_read: &Receiver<(String, OwnedFd)>, changed: &dyn Fn(Change)) {
    for (mime, fd) in to_read {
        let data = match read(fd) {
            Ok(data) => data,
            Err(err) => {
                kotodama_extension::log!("not kept: {err}");
                continue;
            }
        };
        match Copied::read(&mime, data) {
            Ok(copied) => changed(Change::Copied(copied)),
            Err(err) => kotodama_extension::log!("not kept: {}", Chain(&err)),
        }
    }
}

/// Everything the source writes until it closes its end.
///
/// # Errors
///
/// [`SourceError`] once it is too much, the source stalls, or reading fails.
fn read(fd: OwnedFd) -> Result<Vec<u8>, SourceError> {
    let mut file = File::from(fd);
    let mut data = Vec::new();
    let mut buf = vec![0; 64 * 1024];
    let timeout = Timespec::try_from(READ_TIMEOUT).map_err(SourceError::other)?;
    loop {
        let mut fds = [PollFd::new(&file, PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(0) => return Err(SourceError::Stalled),
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(errno) => return Err(SourceError::Io(io::Error::from(errno))),
        }
        match file.read(&mut buf) {
            Ok(0) => return Ok(data),
            Ok(n) => {
                if data.len() + n > MAX_BYTES {
                    return Err(SourceError::TooBig);
                }
                data.extend_from_slice(&buf[..n]);
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(SourceError::Io(err)),
        }
    }
}

/// Why an offer wasn't read off its source.
#[derive(Debug, thiserror::Error)]
enum SourceError {
    #[error("the source stopped sending")]
    Stalled,
    #[error("more than {} MiB", MAX_BYTES >> 20)]
    TooBig,
    /// Polling or reading the pipe failed; its error is the message as it is.
    #[error(transparent)]
    Io(io::Error),
}

impl SourceError {
    fn other(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        SourceError::Io(io::Error::other(err))
    }
}

/// What the Wayland thread's event queue dispatches to.
struct State {
    control: DataControl,
    /// Hands an offer's read end to the reading thread.
    reads: Sender<(String, OwnedFd)>,
    /// The compositor took the data device away.
    finished: bool,
}

impl DataControlHandler for State {
    /// A new selection: reads it unless it is sensitive or holds nothing
    /// worth keeping.
    fn selected(&mut self, offer: &Offer, mimes: Vec<String>) {
        match choose(mimes.iter().map(String::as_str)) {
            Choice::Read(mime) => match rustix::pipe::pipe_with(PipeFlags::CLOEXEC) {
                Ok((read, write)) => {
                    offer.receive(mime.clone(), &write);
                    // The request carries its own copy of the write end;
                    // ours must close, or the read never ends.
                    drop(write);
                    let _ = self.reads.send((mime, read));
                }
                Err(err) => kotodama_extension::log!("no pipe to read through: {err}"),
            },
            Choice::Sensitive | Choice::Nothing => {}
        }
    }

    fn finished(&mut self) {
        self.finished = true;
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

delegate_noop!(State: ignore wl_seat::WlSeat);
kotodama_extension::delegate_data_control!(State);
