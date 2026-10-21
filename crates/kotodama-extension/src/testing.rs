//! What the tests of Extensions and the Daemon share (feature `testing`, for
//! dev-dependencies only): [`Spawned`] runs the Extension as the Daemon does,
//! over its stdio; [`install`] and [`connect`] set up the real Daemon, in the
//! test's process or its own, and a Front end on its socket ([`front_end`] on
//! a connection made already).
//!
//! Each helper panics when what it waits for doesn't happen, as a test
//! would, with what went wrong.
//!
//! # Owns
//!
//! Spawning an Extension as the Daemon does and reading what it sends;
//! installing a Manifest; connecting a Front end.
//!
//! # Not here
//!
//! Starting the Daemon itself, which each test does its own way.

use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::Channel;
use kotodama_protocol::ChannelWriter;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Hello;
use kotodama_protocol::Item;
use kotodama_protocol::ItemRef;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;
use kotodama_protocol::WireFormat;

/// An Extension process the test talks to as the Daemon would. Dropping it
/// kills the process.
#[derive(Debug)]
pub struct Spawned {
    /// The Extension's process.
    pub child: Child,
    /// `None` once the test has hung up.
    writer: Option<ChannelWriter<ChildStdin>>,
    received: Receiver<FromExtension>,
    /// The `status` messages, apart from the rest, so a test of the Items
    /// never meets one.
    statuses: Receiver<FromExtension>,
    wait: Duration,
}

impl Spawned {
    /// Starts `command` with piped stdio and does the handshake. `wait` is how
    /// long [`Spawned::recv`] waits for a message.
    ///
    /// # Panics
    ///
    /// If the process can't start, or the handshake fails.
    pub fn start(command: &mut Command, wait: Duration) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the Extension");
        let stdout = child.stdout.take().expect("the Extension's stdout is piped");
        let stdin = child.stdin.take().expect("the Extension's stdin is piped");
        let channel = Channel::accept(stdout, stdin, &Hello::new(&WireFormat::ALL)).expect("the handshake");
        let (mut reader, writer) = channel.split();
        let (sender, received) = mpsc::channel();
        let (status_sender, statuses) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(Some(message)) = reader.recv::<FromExtension>() {
                let sender = match message {
                    FromExtension::Status { .. } => &status_sender,
                    _ => &sender,
                };
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Spawned {
            child,
            writer: Some(writer),
            received,
            statuses,
            wait,
        }
    }

    /// Sends `message` to the Extension.
    ///
    /// # Panics
    ///
    /// If the test has hung up, or the message can't be written.
    pub fn send(&mut self, message: impl Borrow<ToExtension>) {
        self.writer
            .as_mut()
            .expect("the test has hung up")
            .send(message.borrow())
            .expect("send to the Extension");
    }

    /// Closes the Extension's stdin, as the Daemon does when it goes away.
    pub fn hang_up(&mut self) {
        self.writer = None;
    }

    /// The next message, waiting up to `timeout` for it.
    ///
    /// # Errors
    ///
    /// When none comes in time, or the Extension has hung up.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<FromExtension, RecvTimeoutError> {
        self.received.recv_timeout(timeout)
    }

    /// The next message.
    ///
    /// # Panics
    ///
    /// If none comes in time.
    pub fn recv(&self) -> FromExtension {
        self.recv_timeout(self.wait).expect("the Extension sent nothing")
    }

    /// The Items of the next message, which must be `add_items`.
    ///
    /// # Panics
    ///
    /// If none comes in time, or it is another message.
    pub fn added(&self) -> Vec<Item> {
        match self.recv() {
            FromExtension::AddItems { items } => items,
            other => panic!("expected add_items, got {other:?}"),
        }
    }

    /// The ids of the next message, which must be `remove_items`.
    ///
    /// # Panics
    ///
    /// If none comes in time, or it is another message.
    pub fn removed(&self) -> Vec<String> {
        match self.recv() {
            FromExtension::RemoveItems { ids } => ids,
            other => panic!("expected remove_items, got {other:?}"),
        }
    }

    /// The next `status` message, as whether the Extension is usable and its
    /// note. The other messages never hold one.
    ///
    /// # Panics
    ///
    /// If none comes in time.
    pub fn status(&self) -> (bool, Option<String>) {
        match self.statuses.recv_timeout(self.wait) {
            Ok(FromExtension::Status { usable, note }) => (usable, note),
            Ok(other) => unreachable!("only statuses arrive here, got {other:?}"),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                panic!("the Extension said nothing of whether it is usable")
            }
        }
    }

    /// Waits 600 ms for the Extension to stay quiet.
    ///
    /// # Panics
    ///
    /// If a message comes meanwhile.
    pub fn quiet(&self) {
        if let Ok(message) = self.recv_timeout(Duration::from_millis(600)) {
            panic!("expected nothing, got {message:?}");
        }
    }

    /// Runs `action` on `item` as `invocation`, without waiting for the outcome.
    ///
    /// # Panics
    ///
    /// If the test has hung up, or the message can't be written.
    pub fn start_run(&mut self, invocation: u64, action: &str, item: ItemRef, arguments: BTreeMap<String, String>) {
        self.send(ToExtension::RunAction {
            invocation,
            action: action.into(),
            item,
            arguments,
        });
    }

    /// The outcome of `invocation`. Messages `skip` accepts may come first;
    /// any other panics.
    ///
    /// # Panics
    ///
    /// If none comes in time, or another message `skip` doesn't accept.
    pub fn outcome(&self, invocation: u64, skip: impl Fn(&FromExtension) -> bool) -> Outcome {
        loop {
            match self.recv() {
                FromExtension::ActionDone {
                    invocation: done,
                    outcome,
                } if done == invocation => return outcome,
                other if skip(&other) => {}
                other => panic!("expected action_done, got {other:?}"),
            }
        }
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Writes `manifest` as the Manifest of the Extension `name` under
/// `extensions`, for a Daemon whose extension directory that is.
///
/// # Panics
///
/// If the directory or the Manifest can't be written.
pub fn install(extensions: &Path, name: &str, manifest: &str) {
    let dir = extensions.join(name);
    fs::create_dir_all(&dir).expect("make the Extension's directory");
    fs::write(dir.join("manifest.toml"), manifest).expect("write the Manifest");
}

/// A Bundled extension's own `manifest`, its `"./<name>"` command pointed at
/// `binary`, where cargo built it.
///
/// # Panics
///
/// If `binary` has no file name.
#[must_use]
pub fn bundled(manifest: &str, binary: &str) -> String {
    let name = Path::new(binary)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a binary path");
    manifest.replace(&format!("\"./{name}\""), &format!("{binary:?}"))
}

/// A Front end on the Daemon's `socket`, once the Daemon listens on it, with
/// reads timing out after `read_timeout`.
///
/// # Panics
///
/// If the socket isn't there within 10 s, or the handshake fails.
pub fn connect(socket: &Path, read_timeout: Duration) -> Channel<UnixStream, UnixStream> {
    let start = Instant::now();
    let stream = loop {
        if let Ok(stream) = UnixStream::connect(socket) {
            break stream;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the Daemon never opened its socket"
        );
        thread::sleep(Duration::from_millis(10));
    };
    front_end(stream, read_timeout)
}

/// A Front end on `stream`, a connection to the Daemon's socket made already,
/// with reads timing out after `read_timeout`.
///
/// # Panics
///
/// If the stream can't be set up, or the handshake fails.
pub fn front_end(stream: UnixStream, read_timeout: Duration) -> Channel<UnixStream, UnixStream> {
    stream
        .set_read_timeout(Some(read_timeout))
        .expect("set the read timeout");
    let reader = stream.try_clone().expect("clone the stream");
    Channel::handshake(reader, stream, &Hello::new(&WireFormat::ALL)).expect("the handshake")
}
