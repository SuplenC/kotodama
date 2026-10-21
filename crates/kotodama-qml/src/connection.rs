//! The connection to the Daemon. A thread of its own connects, reads what the
//! Daemon sends, turns it into what the model needs, and hands it to the Qt
//! thread; the Qt thread writes. When the Daemon goes away the thread connects
//! again, so the menu works whenever the Daemon does.

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use kotodama_protocol::{
    Channel, ChannelWriter, Error, FromFrontEnd, Hello, Job, ListMatch, Scope, ToFrontEnd, View,
    WireFormat, socket_path,
};

use crate::rows::Row;
use crate::views::Contributed;

const FIRST_RETRY: Duration = Duration::from_millis(100);
const MAX_RETRY: Duration = Duration::from_secs(2);
/// A Daemon that takes longer than this to read a keystroke has hung. The Qt
/// thread gives up on the write rather than freeze the menu.
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most messages handed over at once.
const MAX_BATCH: usize = 64;
/// How long a View an Extension popped waits for the one it pushes next, so
/// the search doesn't show between them: about a frame.
const PUSH_AFTER_POP: Duration = Duration::from_millis(16);

/// What the connection hands the Qt thread.
pub enum Update {
    Connected(Writer),
    /// The Results, their version, and the app the query is in.
    Rows(u32, Vec<Row>, Option<Scope>),
    Down(Vec<String>),
    Close,
    Open {
        query: String,
        view: Option<String>,
    },
    Toggle,
    ActionFailed(String),
    /// The Daemon replaces the query, as running an app's entry Item does.
    SetQuery(String),
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
    PopView {
        extension: String,
        view: String,
    },
    JobProgress {
        extension: String,
        job: Job,
    },
    /// The change to the History the Toast offers to undo.
    OfferUndo {
        change: u64,
        title: String,
    },
    /// The change can't be undone any more.
    WithdrawUndo {
        change: u64,
    },
    Disconnected,
}

impl Update {
    /// Whether it asks QML to act, by a signal, rather than changes what
    /// the menu shows.
    pub fn signals(&self) -> bool {
        matches!(
            self,
            Update::Close
                | Update::Open { .. }
                | Update::Toggle
                | Update::ActionFailed(_)
                | Update::SetQuery(_)
        )
    }
}

/// The Qt thread's half of the connection.
pub struct Writer {
    channel: ChannelWriter<UnixStream>,
    stream: UnixStream,
}

impl Writer {
    pub fn send(&mut self, message: &FromFrontEnd) -> Result<(), Error> {
        self.channel.send(message)
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

/// The reader, the socket again for [`more_within`], and the writer.
fn connect() -> Result<(Reader, UnixStream, Writer), Error> {
    let socket = socket_path().ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "XDG_RUNTIME_DIR isn't set",
        ))
    })?;
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let channel = Channel::handshake(
        stream.try_clone()?,
        stream.try_clone()?,
        &Hello::new(&WireFormat::ALL),
    )?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let (reader, channel) = channel.split();
    Ok((reader, stream.try_clone()?, Writer { channel, stream }))
}

/// Reads until the Daemon hangs up or the model is gone. What has arrived
/// already goes over with the message before it, up to [`MAX_BATCH`].
///
/// A View the Extension popped waits [`PUSH_AFTER_POP`] for more, as the
/// Extension may push the next one in the same reply.
fn read(
    mut reader: Reader,
    probe: &UnixStream,
    deliver: &impl Fn(Vec<Update>) -> bool,
) -> Result<(), Error> {
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
        let more =
            reader.has_buffered() || (popped && more_within(&mut reader, probe, PUSH_AFTER_POP));
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
        ToFrontEnd::Status { down } => Update::Down(down),
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
