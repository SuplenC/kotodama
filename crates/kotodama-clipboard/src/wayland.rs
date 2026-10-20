//! Watching the Wayland clipboard, and setting it, through
//! `kotodama_extension::wayland` (ext-data-control, or wlr-data-control on a
//! compositor that has only that).
//!
//! The connection lives on a thread of its own, which first waits for the
//! compositor: the Extension may start before the compositor does, or before
//! `WAYLAND_DISPLAY` reaches it (see `kotodama_extension::wayland::Display`).
//! What each new selection offers is checked before anything is read: an
//! offer a password manager marked sensitive is never read at all (see
//! [`choose`](crate::copied::choose)). The rest are read one at a time on a
//! second thread, which also decodes images for their thumbnails, so a slow
//! source or a large image never holds up the clipboard or the Daemon.

use crate::copied::Copied;

/// What the watching thread reports.
pub enum Change {
    /// Something was copied, and read.
    Copied(Copied),
    /// The connection to the compositor is gone.
    Lost,
}

#[cfg(target_os = "linux")]
pub use linux::Clipboard;

/// Without Linux there is no Wayland clipboard to watch.
#[cfg(not(target_os = "linux"))]
pub struct Clipboard;

#[cfg(not(target_os = "linux"))]
impl Clipboard {
    pub fn watch(_changed: impl Fn(Change) + Send + Sync + 'static) -> Result<Clipboard, String> {
        Err("the clipboard needs Wayland, which only Linux has".into())
    }

    pub fn set(&self, _mimes: Vec<String>, _data: Vec<u8>) -> Result<(), String> {
        Err("the clipboard needs Wayland, which only Linux has".into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::OwnedFd;
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::thread;
    use std::time::{Duration, Instant};

    use kotodama_extension::wayland::{DataControl, DataControlHandler, Display, Offer};
    use rustix::event::{PollFd, PollFlags, Timespec};
    use rustix::pipe::PipeFlags;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};

    use super::Change;
    use crate::copied::{Choice, Copied, choose};

    /// The most bytes one copy may have; anything bigger isn't kept. Well
    /// under the protocol's 64 MiB frame, so it can always be pasted back.
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    /// How long a source may go without sending a byte before reading it is
    /// given up.
    const READ_TIMEOUT: Duration = Duration::from_secs(5);
    const STACK: usize = 256 * 1024;

    /// What to put on the clipboard, until when (`set` has given up after),
    /// and where to say it is there.
    type Request = (Vec<String>, Vec<u8>, Instant, Sender<()>);

    /// The watched clipboard. Dropping it stops the watching, once the
    /// compositor is there.
    pub struct Clipboard {
        requests: Sender<Request>,
        wake: OwnedFd,
    }

    impl Clipboard {
        /// Calls `changed` with each thing copied from now on, on a thread of
        /// its own, which first waits for the compositor if it isn't there
        /// yet.
        pub fn watch(
            changed: impl Fn(Change) + Send + Sync + 'static,
        ) -> Result<Clipboard, String> {
            let (reads, to_read) = mpsc::channel::<(String, OwnedFd)>();
            let changed = Arc::new(changed);
            let reporter = Arc::clone(&changed);
            thread::Builder::new()
                .name("read".into())
                .stack_size(STACK)
                .spawn(move || read_all(&to_read, &*reporter))
                .map_err(|err| err.to_string())?;

            let (wake_rx, wake) =
                rustix::pipe::pipe_with(PipeFlags::CLOEXEC).map_err(|err| err.to_string())?;
            let (requests, inbox) = mpsc::channel();
            thread::Builder::new()
                .name("wayland".into())
                .stack_size(STACK)
                .spawn(move || {
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
                    run(queue, state, &inbox, &wake_rx);
                    changed(Change::Lost);
                })
                .map_err(|err| err.to_string())?;
            Ok(Clipboard { requests, wake })
        }

        /// Puts `data` on the clipboard, offered as each of `mimes`, and
        /// returns once the compositor has it there, so what is copied next
        /// comes after it. The clipboard keeps it until something else is
        /// copied. A compositor that doesn't answer within `READ_TIMEOUT`,
        /// or isn't there yet, fails it.
        pub fn set(&self, mimes: Vec<String>, data: Vec<u8>) -> Result<(), String> {
            let (done, copied) = mpsc::channel();
            if self.requests.send((mimes, data, Instant::now() + READ_TIMEOUT, done)).is_err()
                || rustix::io::write(&self.wake, &[1]).is_err()
            {
                return Err("lost the connection to the compositor".into());
            }
            match copied.recv_timeout(READ_TIMEOUT) {
                Ok(()) => Ok(()),
                Err(RecvTimeoutError::Timeout) => Err("the compositor did not answer".into()),
                Err(RecvTimeoutError::Disconnected) => {
                    Err("lost the connection to the compositor".into())
                }
            }
        }
    }

    /// Binds the clipboard on a new connection, and takes in what it holds.
    /// The outer error is worth another try; the inner one, a compositor
    /// with no data-control, is not.
    fn connect(
        conn: &Connection,
        reads: Sender<(String, OwnedFd)>,
    ) -> Result<Result<(EventQueue<State>, State), String>, String> {
        let (globals, mut queue) =
            registry_queue_init::<State>(conn).map_err(|err| err.to_string())?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| format!("no seat: {err}"))?;
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
    fn run(
        mut queue: EventQueue<State>,
        mut state: State,
        inbox: &Receiver<Request>,
        wake: &OwnedFd,
    ) {
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
                    // Asked before the compositor was there, and failed
                    // since: setting it now would undo a later copy.
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
            match read(fd).and_then(|data| Copied::read(&mime, data)) {
                Ok(copied) => changed(Change::Copied(copied)),
                Err(err) => kotodama_extension::log!("not kept: {err}"),
            }
        }
    }

    /// Everything the source writes until it closes its end, or an error
    /// once it is too much or the source stalls.
    fn read(fd: OwnedFd) -> Result<Vec<u8>, String> {
        let mut file = File::from(fd);
        let mut data = Vec::new();
        let mut buf = vec![0; 64 * 1024];
        let timeout = Timespec::try_from(READ_TIMEOUT).map_err(|err| err.to_string())?;
        loop {
            let mut fds = [PollFd::new(&file, PollFlags::IN)];
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err("the source stopped sending".into()),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(err.to_string()),
            }
            match file.read(&mut buf) {
                Ok(0) => return Ok(data),
                Ok(n) => {
                    if data.len() + n > MAX_BYTES {
                        return Err(format!("more than {} MiB", MAX_BYTES >> 20));
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err.to_string()),
            }
        }
    }

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
}
