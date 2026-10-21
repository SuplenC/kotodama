//! The connection to the Daemon. A thread of its own connects, reads what the
//! Daemon sends, turns it into what the model needs, and hands it to the Qt
//! thread; the Qt thread writes. When the Daemon goes away the thread connects
//! again, so the menu works whenever the Daemon does.
//!
//! # Owns
//!
//! - The connection thread: connecting, the handshake, retrying, and batching what arrives
//!   together.
//! - [`Update`], each message as the Qt thread takes it, and [`Writer`], the Qt thread's half of
//!   the socket.
//!
//! # Not here
//!
//! - What an update does to the menu: `updates`.
//! - The Wire formats and the framing: `kotodama-protocol`.

#![forbid(unsafe_code)]

use std::fmt;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use kotodama_protocol::Channel;
use kotodama_protocol::ChannelWriter;
use kotodama_protocol::Error;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::Job;
use kotodama_protocol::ListMatch;
use kotodama_protocol::Scope;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::View;
use kotodama_protocol::WireFormat;
use kotodama_protocol::socket_path;

use crate::rows::Row;
use crate::views::Contributed;

/// The wait before connecting again, doubled after each failure in a row up
/// to [`MAX_RETRY`].
const FIRST_RETRY: Duration = Duration::from_millis(100);
/// The longest wait between two tries, so a Daemon that restarts is found
/// within it.
const MAX_RETRY: Duration = Duration::from_secs(2);
/// A Daemon that takes longer than this to read a keystroke has hung. The Qt
/// thread gives up on the write rather than freeze the menu.
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);
/// A Daemon that doesn't answer the handshake within this has hung; the thread
/// tries again.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most messages handed over at once.
const MAX_BATCH: usize = 64;
/// How long a View an Extension popped waits for the one it pushes next, so
/// the search doesn't show between them: about a frame.
const PUSH_AFTER_POP: Duration = Duration::from_millis(16);

/// What the connection hands the Qt thread.
pub enum Update {
    /// The Daemon answered the handshake; the Qt thread writes through this.
    Connected(Writer),
    /// The Results, their version, and the app the query is in.
    Rows(u32, Vec<Row>, Option<Scope>),
    /// The Extensions that are down, and how many problems there are.
    Status(Vec<String>, usize),
    /// The Daemon asks the window to close.
    Close,
    /// `kotodama open`: the query to show, and the View asked for.
    Open { query: String, view: Option<String> },
    /// `kotodama toggle`.
    Toggle,
    /// Why an Action failed.
    ActionFailed(String),
    /// The Daemon replaces the query, as running an app's entry Item does.
    SetQuery(String),
    /// A View on top, with the Actions contributed to a list View's Items.
    PushView {
        extension: String,
        view: View,
        contributed: Contributed,
    },
    /// The open View with the same id changes.
    ViewUpdated {
        extension: String,
        view: View,
        contributed: Contributed,
    },
    /// `with` takes the place of the open View `view`.
    ReplaceView {
        extension: String,
        view: String,
        with: View,
        contributed: Contributed,
    },
    /// The Daemon's matches for what is typed into a list View.
    FilteredView {
        extension: String,
        view: String,
        text: String,
        matches: Vec<ListMatch>,
    },
    /// The Extension popped one of its Views.
    PopView { extension: String, view: String },
    /// A job's latest state.
    JobProgress { extension: String, job: Job },
    /// The change to the History the Toast offers to undo.
    OfferUndo { change: u64, title: String },
    /// The change can't be undone any more.
    WithdrawUndo { change: u64 },
    /// The Daemon hung up; the thread connects again.
    Disconnected,
}

impl Update {
    /// Whether it asks QML to act, by a signal, rather than changes what
    /// the menu shows.
    pub fn signals(&self) -> bool {
        matches!(
            self,
            Update::Close | Update::Open { .. } | Update::Toggle | Update::ActionFailed(_) | Update::SetQuery(_)
        )
    }
}

/// The Qt thread's half of the connection.
pub struct Writer {
    channel: ChannelWriter<UnixStream>,
    /// The socket, shut down when the writer drops.
    stream: UnixStream,
}

impl Writer {
    /// Sends `message` to the Daemon.
    ///
    /// # Errors
    ///
    /// The Daemon is gone, took longer than [`WRITE_TIMEOUT`] to read, or the
    /// message didn't encode.
    pub fn send(&mut self, message: &FromFrontEnd) -> Result<(), Error> {
        self.channel.send(message)
    }
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("stream", &self.stream)
            .finish_non_exhaustive()
    }
}

impl Drop for Writer {
    /// Wakes the reading thread, which then finds the model gone and ends.
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// Starts the connection thread. `deliver` hands updates to the Qt thread, in
/// order, and returns false once there is no one to hand them to, which ends
/// the thread. Messages the Daemon sent together come in one batch, so the
/// menu draws what they add up to: a View popped and another pushed in one
/// reply never shows the search between them.
pub fn spawn(deliver: impl Fn(Vec<Update>) -> bool + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("kotodama".into())
        .stack_size(256 * 1024)
        .spawn(move || run(&deliver));
    if let Err(err) = started {
        eprintln!("kotodama: couldn't start the connection thread: {err}");
    }
}

fn run(deliver: &impl Fn(Vec<Update>) -> bool) {
    let mut retry = FIRST_RETRY;
    loop {
        match connect() {
            Ok((reader, probe, writer)) => {
                retry = FIRST_RETRY;
                if !deliver(vec![Update::Connected(writer)]) {
                    return;
                }
                if let Err(err) = read(reader, &probe, deliver) {
                    eprintln!("kotodama: lost the Daemon: {err}");
                }
                if !deliver(vec![Update::Disconnected]) {
                    return;
                }
            }
            Err(err) => {
                if retry == FIRST_RETRY {
                    eprintln!("kotodama: can't reach the Daemon yet: {err}");
                }
            }
        }
        std::thread::sleep(retry);
        retry = (retry * 2).min(MAX_RETRY);
    }
}

type Reader = kotodama_protocol::ChannelReader<UnixStream>;

/// Why the thread couldn't connect to the Daemon. Each cause prints as the
/// error itself, so the log line reads `can't reach the Daemon yet: <cause>`.
#[derive(Debug, thiserror::Error)]
enum ConnectError {
    #[error("XDG_RUNTIME_DIR isn't set")]
    NoRuntimeDir,
    #[error(transparent)]
    Socket(io::Error),
    #[error(transparent)]
    Handshake(Error),
}

impl ConnectError {
    fn socket(err: io::Error) -> Self {
        ConnectError::Socket(err)
    }

    fn handshake(err: Error) -> Self {
        ConnectError::Handshake(err)
    }
}

/// The reader, the socket again for [`more_within`], and the writer.
fn connect() -> Result<(Reader, UnixStream, Writer), ConnectError> {
    let socket = socket_path().ok_or(ConnectError::NoRuntimeDir)?;
    let stream = UnixStream::connect(socket).map_err(ConnectError::socket)?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(ConnectError::socket)?;
    let channel = Channel::handshake(
        stream.try_clone().map_err(ConnectError::socket)?,
        stream.try_clone().map_err(ConnectError::socket)?,
        &Hello::new(&WireFormat::ALL),
    )
    .map_err(ConnectError::handshake)?;
    stream.set_read_timeout(None).map_err(ConnectError::socket)?;
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(ConnectError::socket)?;
    let (reader, channel) = channel.split();
    let probe = stream.try_clone().map_err(ConnectError::socket)?;
    Ok((reader, probe, Writer { channel, stream }))
}

/// Reads until the Daemon hangs up or the model is gone. What has arrived
/// already goes over with the message before it, up to [`MAX_BATCH`].
///
/// A View the Extension popped waits [`PUSH_AFTER_POP`] for more, as the
/// Extension may push the next one in the same reply.
fn read(mut reader: Reader, probe: &UnixStream, deliver: &impl Fn(Vec<Update>) -> bool) -> Result<(), Error> {
    let mut batch = Vec::new();
    loop {
        let message = match reader.recv::<ToFrontEnd>() {
            Ok(Some(message)) => Some(message),
            Ok(None) => {
                if !batch.is_empty() {
                    deliver(batch);
                }
                return Ok(());
            }
            Err(err @ Error::Decode { .. }) => {
                eprintln!("kotodama: skipped a message: {err}");
                None
            }
            Err(err) => {
                if !batch.is_empty() {
                    deliver(batch);
                }
                return Err(err);
            }
        };
        if let Some(update) = message.and_then(update) {
            batch.push(update);
        }
        let popped = matches!(batch.last(), Some(Update::PopView { .. }));
        let more = reader.has_buffered() || (popped && more_within(&mut reader, probe, PUSH_AFTER_POP));
        if batch.is_empty() || (more && batch.len() < MAX_BATCH) {
            continue;
        }
        if !deliver(std::mem::take(&mut batch)) {
            return Ok(());
        }
    }
}

/// What `message` changes; nothing for the answer to a request for a menu,
/// which the menu never makes.
fn update(message: ToFrontEnd) -> Option<Update> {
    Some(match message {
        ToFrontEnd::Results {
            version,
            results,
            scope,
        } => Update::Rows(version, results.into_iter().map(Row::from).collect(), scope),
        ToFrontEnd::Status { down, problems } => Update::Status(down, problems.len()),
        ToFrontEnd::Close => Update::Close,
        ToFrontEnd::ActionFailed { message } => Update::ActionFailed(message),
        ToFrontEnd::Open { query, view } => Update::Open { query, view },
        ToFrontEnd::Toggle => Update::Toggle,
        ToFrontEnd::PushView {
            extension,
            view,
            contributed,
        } => Update::PushView {
            extension,
            view,
            contributed,
        },
        ToFrontEnd::UpdateView {
            extension,
            view,
            contributed,
        } => Update::ViewUpdated {
            extension,
            view,
            contributed,
        },
        ToFrontEnd::ReplaceView {
            extension,
            view,
            with,
            contributed,
        } => Update::ReplaceView {
            extension,
            view,
            with,
            contributed,
        },
        ToFrontEnd::PopView { extension, view } => Update::PopView { extension, view },
        ToFrontEnd::FilteredView {
            extension,
            view,
            text,
            matches,
        } => Update::FilteredView {
            extension,
            view,
            text,
            matches,
        },
        ToFrontEnd::JobProgress { extension, job } => Update::JobProgress { extension, job },
        ToFrontEnd::OfferUndo { change, title } => Update::OfferUndo { change, title },
        ToFrontEnd::WithdrawUndo { change } => Update::WithdrawUndo { change },
        ToFrontEnd::SetQuery { text } => Update::SetQuery(text),
        ToFrontEnd::Reached { .. } => return None,
    })
}

/// Whether the Daemon sends something within `wait`, or hangs up. The
/// reader is between messages, so the socket's timeout is free to set.
fn more_within(reader: &mut Reader, probe: &UnixStream, wait: Duration) -> bool {
    if probe.set_read_timeout(Some(wait)).is_err() {
        return false;
    }
    let more = reader.wait_for_more();
    let _ = probe.set_read_timeout(None);
    more
}
