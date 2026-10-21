//! The `kotodama` command, for a compositor keybind: it asks the Daemon to open or toggle the menu,
//! and the Daemon passes that on to a menu. It exits with 1 and says how to start one when no menu
//! is running.
//!
//! ```sh
//! kotodama toggle
//! kotodama open --query "fir"
//! kotodama open --view clipboard/history
//! kotodama open --app emoji
//! ```
//!
//! `--view <extension>/<item>` opens the menu on the View that Item's primary Action pushes.
//! `--app <word>` opens it in the app with that word, searching that Extension alone; `--prefix`
//! is its old name, still read.
//!
//! # Owns
//!
//! - The command line: its commands, options, usage and exit codes.
//! - Sending its one `FromFrontEnd` to the Daemon as a Front end, and reading the answer.
//!
//! # Not here
//!
//! - The socket's path, the handshake and the framing: `kotodama-protocol` (`socket_path`,
//!   `Channel`).
//! - What the Daemon does with the message: `kotodamad/src/daemon.rs`.
//! - Showing the menu: the Front end, `kotodama-qml`.

#![forbid(unsafe_code)]

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kotodama_protocol::Channel;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Hello;
use kotodama_protocol::ToFrontEnd;
use kotodama_protocol::WireFormat;
use kotodama_protocol::socket_path;

const USAGE: &str = "usage: kotodama toggle
       kotodama open [--view <extension>/<item>] [--app <word>] [--query <text>]";

/// Under socket activation the first keypress also starts the Daemon, so the handshake may wait
/// for it to come up. The answer waits for it too, and for a menu, when none is connected yet.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the command line is not one `kotodama` understands. It is printed above the usage.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ParseError {
    #[error("no command given")]
    NoCommand,
    /// The whole command line, as typed.
    #[error("unknown command {0}")]
    UnknownCommand(String),
    #[error("unknown option {0}")]
    UnknownOption(String),
    /// The last argument, left without a value.
    #[error("{0} needs a value, or isn't an option")]
    NoValue(String),
}

/// Why the message did not reach the Daemon.
#[derive(Debug, thiserror::Error)]
enum SendError {
    #[error("XDG_RUNTIME_DIR isn't set")]
    NoRuntimeDir,
    /// The hint reads after the OS's reason, so the reason is part of this message and not its
    /// `source()`.
    #[error("{}: {reason}; is kotodamad running?", socket.display())]
    Connect { socket: PathBuf, reason: io::Error },
    #[error(transparent)]
    Socket(io::Error),
    #[error(transparent)]
    Handshake(kotodama_protocol::Error),
    #[error(transparent)]
    Send(kotodama_protocol::Error),
    /// Reading the answer failed, or timed out.
    #[error("the Daemon didn't answer: {0}")]
    Answer(kotodama_protocol::Error),
    #[error("the Daemon hung up before it answered")]
    HungUp,
    #[error(
        "no menu is running: start one, such as the `kotodama-menu` service of home-manager's \
         `programs.kotodama.frontEnd.enable`, or a Quickshell that places `KotodamaMenu`"
    )]
    NoMenu,
}

impl SendError {
    fn connect(socket: PathBuf, reason: io::Error) -> Self {
        Self::Connect { socket, reason }
    }

    fn socket(err: io::Error) -> Self {
        Self::Socket(err)
    }

    fn handshake(err: kotodama_protocol::Error) -> Self {
        Self::Handshake(err)
    }

    fn send(err: kotodama_protocol::Error) -> Self {
        Self::Send(err)
    }

    fn answer(err: kotodama_protocol::Error) -> Self {
        Self::Answer(err)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let message = match parse(&args) {
        Ok(message) => message,
        Err(err) => {
            eprintln!("kotodama: {err}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match send(&message) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kotodama: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the arguments after the program's name into the message for the Daemon. `help` prints
/// the usage and exits.
fn parse(args: &[String]) -> Result<FromFrontEnd, ParseError> {
    let (command, mut rest) = args.split_first().ok_or(ParseError::NoCommand)?;
    match command.as_str() {
        "toggle" if rest.is_empty() => Ok(FromFrontEnd::Toggle),
        "open" => {
            let (mut query, mut view, mut app) = (String::new(), None, None);
            while let [flag, value, tail @ ..] = rest {
                match flag.as_str() {
                    "--query" => query.clone_from(value),
                    "--view" => view = Some(value.clone()),
                    "--app" | "--prefix" => app = Some(value.as_str()),
                    _ => return Err(ParseError::UnknownOption(flag.clone())),
                }
                rest = tail;
            }
            if let [extra] = rest {
                return Err(ParseError::NoValue(extra.clone()));
            }
            if let Some(word) = app {
                query = format!("{word} {query}");
            }
            Ok(FromFrontEnd::Open { query, view })
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        _ => Err(ParseError::UnknownCommand(args.join(" "))),
    }
}

fn send(message: &FromFrontEnd) -> Result<(), SendError> {
    let socket = socket_path().ok_or(SendError::NoRuntimeDir)?;
    let stream = UnixStream::connect(&socket).map_err(|reason| SendError::connect(socket, reason))?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(SendError::socket)?;
    let reader = stream.try_clone().map_err(SendError::socket)?;
    let mut channel =
        Channel::handshake(reader, stream, &Hello::new(&WireFormat::ALL)).map_err(SendError::handshake)?;
    channel.send(message).map_err(SendError::send)?;
    match answer(&mut channel)? {
        0 => Err(SendError::NoMenu),
        _ => Ok(()),
    }
}

/// How many menus the Daemon passed the message on to. It waits a moment for a menu when none is
/// connected, well within the read timeout. Anything else sent meanwhile is skipped, but a request
/// to close is answered at once, so typing into the window underneath never waits on the command.
fn answer<R: io::Read, W: io::Write>(channel: &mut Channel<R, W>) -> Result<u32, SendError> {
    loop {
        match channel.recv().map_err(SendError::answer)? {
            Some(ToFrontEnd::Reached { menus }) => return Ok(menus),
            Some(ToFrontEnd::Close) => channel.send(&FromFrontEnd::Closed).map_err(SendError::send)?,
            Some(_) => {}
            None => return Err(SendError::HungUp),
        }
    }
}

#[cfg(test)]
mod tests;
