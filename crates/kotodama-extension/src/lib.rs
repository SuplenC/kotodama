//! What every Rust Extension of Kotodama needs, so each one only holds what it
//! does. The Bundled extensions are built on it, and an Extension installed
//! separately can be too.
//!
//! - [`main`] keeps resident memory down, names the Extension in what it logs,
//!   and turns the run's error into the exit code.
//! - [`connect`] does the handshake on stdio and starts a thread that reads
//!   the Daemon's messages into a channel, which the Extension's own threads
//!   (a scan, a watcher, a finished download) send their [`Event::Own`]s to as
//!   well. The Extension handles one event at a time, so nothing is shared.
//! - [`SentItems`] sends Indexed items as changes: only the Items added or
//!   changed since the last time, and the ids of the ones gone.
//! - [`config`] reads settings from the Extension's config table, logging the
//!   ones of the wrong shape.
//! - [`dirs`] finds where the Extension keeps its state and cache.
//! - [`launch`] starts the programs Actions run, detached from the Extension.
//! - `watch` (with the `watch` feature) watches directories, and acts once
//!   they settle after a change.
//!
//! An Extension is installed as a directory of its own in
//! `$XDG_DATA_DIRS/kotodama/extensions/` or `~/.config/kotodama/extensions/`,
//! holding its `manifest.toml` and, by the Bundled extensions' convention,
//! its binary beside it, which the Manifest runs as
//! `command = ["./<binary>"]`.
//!
//! A whole Extension, which offers one Item titled by its config:
//!
//! ```no_run
//! use std::process::ExitCode;
//!
//! use kotodama_extension::{Connection, Event, SentItems, config};
//! use kotodama_protocol::{Error, FromExtension, Item, ItemKind, ToExtension};
//!
//! fn main() -> ExitCode {
//!     kotodama_extension::main("kotodama-hello", run)
//! }
//!
//! fn run() -> Result<(), Error> {
//!     let Connection {
//!         mut writer,
//!         received,
//!         ..
//!     } = kotodama_extension::connect::<()>()?;
//!     let mut sent = SentItems::default();
//!     for event in received {
//!         match event {
//!             Event::Message(ToExtension::Config(table)) => {
//!                 let greeting = config::string(&table, "greeting").unwrap_or("Hello");
//!                 let item = Item {
//!                     id: "hello".into(),
//!                     kind: ItemKind::Url {
//!                         url: "https://example.com".into(),
//!                     },
//!                     identity: None,
//!                     title: greeting.into(),
//!                     subtitle: None,
//!                     icon: None,
//!                     keyword: None,
//!                     keywords: Vec::new(),
//!                     hidden: false,
//!                     actions: Vec::new(),
//!                 };
//!                 sent.publish(&mut writer, [item])?;
//!             }
//!             Event::Message(_) | Event::Own(()) => {}
//!             Event::Hangup => break,
//!         }
//!     }
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod dirs;
mod items;
pub mod launch;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(feature = "watch")]
pub mod watch;

use std::fmt::Display;
use std::io::{self, Stdout};
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use kotodama_protocol::{Channel, ChannelWriter, Error, Hello, ToExtension, WireFormat};

pub use items::SentItems;

/// Small, as the threads here only wait, and enough for decoding the deepest
/// message serde_json allows.
pub const STACK_SIZE: usize = 256 * 1024;

/// What sends messages to the Daemon.
pub type Writer = ChannelWriter<Stdout>;

/// One thing for the Extension to handle, in the order they happened.
#[derive(Debug)]
pub enum Event<T> {
    /// A message from the Daemon.
    Message(ToExtension),
    /// Something the Extension's own threads sent.
    Own(T),
    /// The Daemon hung up, or its messages can't be read any more. The
    /// Extension should exit.
    Hangup,
}

/// The connection to the Daemon, once the handshake is done.
pub struct Connection<T> {
    pub writer: Writer,
    /// Sends to `received`, for the Extension's own threads. The Daemon's
    /// messages arrive there already.
    pub events: Sender<Event<T>>,
    pub received: Receiver<Event<T>>,
}

/// Runs an Extension: keeps its resident memory down, names it `name` in what
/// it logs, and runs `run`, which exits the process with success, or with
/// failure once its error is logged. Call it first in `main`, before any
/// thread starts.
pub fn main<E: Display>(name: &'static str, run: impl FnOnce() -> Result<(), E>) -> ExitCode {
    lean_memory();
    let _ = NAME.set(name.to_owned());
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Does the handshake with the Daemon on stdio, offering every Wire format,
/// and starts a thread that reads its messages into the channel it returns.
/// A message that can't be decoded is logged and skipped; once the Daemon
/// hangs up, or a message can't be read at all, the thread sends
/// [`Event::Hangup`] and ends.
pub fn connect<T: Send + 'static>() -> Result<Connection<T>, Error> {
    let channel = Channel::handshake(io::stdin(), io::stdout(), &Hello::new(&WireFormat::ALL))?;
    let (mut reader, writer) = channel.split();
    let (events, received) = mpsc::channel();
    let messages = events.clone();
    thread::Builder::new()
        .name("read".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            loop {
                match reader.recv::<ToExtension>() {
                    Ok(Some(message)) => {
                        if messages.send(Event::Message(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(err @ Error::Decode { .. }) => log!("skipped a message: {err}"),
                    Err(err) => {
                        log!("{err}");
                        break;
                    }
                }
            }
            let _ = messages.send(Event::Hangup);
        })?;
    Ok(Connection {
        writer,
        events,
        received,
    })
}

/// Keeps resident memory down, as the Daemon does. Each thread that allocates
/// would get its own glibc malloc arena, and with transparent huge pages set
/// to `always` each arena costs a 2 MiB page however little it holds. A
/// process whose threads mostly wait loses no speed to one arena. Call it
/// before any thread starts; [`main`] does.
pub fn lean_memory() {
    #[cfg(target_os = "linux")]
    // SAFETY: both calls only set process-wide flags, before any thread starts.
    unsafe {
        libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0);
        #[cfg(target_env = "gnu")]
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

static NAME: OnceLock<String> = OnceLock::new();

/// The name the Extension logs under: the one given to [`main`], or else its
/// program's file name.
pub fn name() -> &'static str {
    NAME.get_or_init(|| {
        std::env::args_os()
            .next()
            .as_deref()
            .and_then(|program| Path::new(program).file_name())
            .map_or_else(
                || "extension".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
    })
}

/// Writes a line to stderr, after the Extension's [`name`]. An Extension's
/// stderr is the Daemon's, which goes to its log.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        ::std::eprintln!("{}: {}", $crate::name(), ::std::format_args!($($arg)*))
    };
}
