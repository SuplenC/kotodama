//! The connection to the Daemon. A thread of its own connects, reads what the
//! Daemon sends, turns it into what the model needs, and hands it to the Qt
//! thread; the Qt thread writes. When the Daemon goes away the thread connects
//! again, so the menu works whenever the Daemon does.

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use kotodama_protocol::{
    Channel, ChannelWriter, Error, FromFrontEnd, Hello, Job, ToFrontEnd, View, WireFormat,
    socket_path,
};

use crate::rows::Row;

const FIRST_RETRY: Duration = Duration::from_millis(100);
const MAX_RETRY: Duration = Duration::from_secs(2);
/// A Daemon that takes longer than this to read a keystroke has hung. The Qt
/// thread gives up on the write rather than freeze the menu.
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// What the connection hands the Qt thread.
pub enum Update {
    Connected(Writer),
    Rows(Vec<Row>),
    Down(Vec<String>),
    Close,
    Open { query: String, view: Option<String> },
    Toggle,
    ActionFailed(String),
    PushView { extension: String, view: View },
    ReplaceView { extension: String, view: View },
    PopView { extension: String, view: String },
    JobProgress { extension: String, job: Job },
    Disconnected,
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

/// Starts the connection thread. `deliver` hands an update to the Qt thread
/// and returns false once there is no one to hand it to, which ends the thread.
pub fn spawn(deliver: impl Fn(Update) -> bool + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("kotodama".into())
        .stack_size(256 * 1024)
        .spawn(move || run(&deliver));
    if let Err(err) = started {
        eprintln!("kotodama: couldn't start the connection thread: {err}");
    }
}

fn run(deliver: &impl Fn(Update) -> bool) {
    let mut retry = FIRST_RETRY;
    loop {
        match connect() {
            Ok((reader, writer)) => {
                retry = FIRST_RETRY;
                if !deliver(Update::Connected(writer)) {
                    return;
                }
                if let Err(err) = read(reader, deliver) {
                    eprintln!("kotodama: lost the Daemon: {err}");
                }
                if !deliver(Update::Disconnected) {
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

fn connect() -> Result<(Reader, Writer), Error> {
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
    Ok((reader, Writer { channel, stream }))
}

/// Reads until the Daemon hangs up or the model is gone.
fn read(mut reader: Reader, deliver: &impl Fn(Update) -> bool) -> Result<(), Error> {
    loop {
        let message = match reader.recv::<ToFrontEnd>() {
            Ok(Some(message)) => message,
            Ok(None) => return Ok(()),
            Err(err @ Error::Decode { .. }) => {
                eprintln!("kotodama: skipped a message: {err}");
                continue;
            }
            Err(err) => return Err(err),
        };
        let update = match message {
            ToFrontEnd::Results { results } => {
                Update::Rows(results.into_iter().map(Row::from).collect())
            }
            ToFrontEnd::Status { down } => Update::Down(down),
            ToFrontEnd::Close => Update::Close,
            ToFrontEnd::ActionFailed { message } => Update::ActionFailed(message),
            ToFrontEnd::Open { query, view } => Update::Open { query, view },
            ToFrontEnd::Toggle => Update::Toggle,
            ToFrontEnd::PushView { extension, view } => Update::PushView { extension, view },
            ToFrontEnd::UpdateView { extension, view } => Update::ReplaceView { extension, view },
            ToFrontEnd::PopView { extension, view } => Update::PopView { extension, view },
            ToFrontEnd::JobProgress { extension, job } => Update::JobProgress { extension, job },
        };
        if !deliver(update) {
            return Ok(());
        }
    }
}
