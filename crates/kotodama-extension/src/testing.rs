//! What the tests of an Extension share (feature `testing`, for
//! dev-dependencies only): [`Spawned`] runs the Extension as the Daemon does,
//! over its stdio; [`install`] and [`connect`] set up the real Daemon, run in
//! the test's process, and a Front end on its socket.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use kotodama_protocol::{
    Channel, ChannelWriter, FromExtension, Hello, Item, ItemRef, Outcome, ToExtension, WireFormat,
};

/// An Extension process the test talks to as the Daemon would. Dropping it
/// kills the process.
pub struct Spawned {
    pub child: Child,
    /// `None` once the test has hung up.
    writer: Option<ChannelWriter<ChildStdin>>,
    received: Receiver<FromExtension>,
    wait: Duration,
}

impl Spawned {
    /// Starts `command` with piped stdio and does the handshake. `wait` is how
    /// long [`Spawned::recv`] waits for a message.
    pub fn start(command: &mut Command, wait: Duration) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (stdout, stdin) = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let channel = Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL)).unwrap();
        let (mut reader, writer) = channel.split();
        let (sender, received) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(Some(message)) = reader.recv::<FromExtension>() {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Spawned {
            child,
            writer: Some(writer),
            received,
            wait,
        }
    }

    pub fn send(&mut self, message: ToExtension) {
        self.writer
            .as_mut()
            .expect("the test has hung up")
            .send(&message)
            .unwrap();
    }

    /// Closes the Extension's stdin, as the Daemon does when it goes away.
    pub fn hang_up(&mut self) {
        self.writer = None;
    }

    /// The next message, waiting up to `timeout` for it.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<FromExtension, RecvTimeoutError> {
        self.received.recv_timeout(timeout)
    }

    /// The next message; panics if none comes in time.
    pub fn recv(&self) -> FromExtension {
        self.recv_timeout(self.wait)
            .expect("the Extension sent nothing")
    }

    /// The Items of the next message, which must be `add_items`.
    pub fn added(&self) -> Vec<Item> {
        match self.recv() {
            FromExtension::AddItems { items } => items,
            other => panic!("expected add_items, got {other:?}"),
        }
    }

    /// The ids of the next message, which must be `remove_items`.
    pub fn removed(&self) -> Vec<String> {
        match self.recv() {
            FromExtension::RemoveItems { ids } => ids,
            other => panic!("expected remove_items, got {other:?}"),
        }
    }

    /// Panics if a message comes within 600 ms.
    pub fn quiet(&self) {
        if let Ok(message) = self.recv_timeout(Duration::from_millis(600)) {
            panic!("expected nothing, got {message:?}");
        }
    }

    /// Runs `action` on `item` as `invocation`, without waiting for the outcome.
    pub fn start_run(
        &mut self,
        invocation: u64,
        action: &str,
        item: ItemRef,
        arguments: BTreeMap<String, String>,
    ) {
        self.send(ToExtension::RunAction {
            invocation,
            action: action.into(),
            item,
            arguments,
        });
    }

    /// The outcome of `invocation`. Messages `skip` accepts may come first;
    /// any other panics.
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
pub fn install(extensions: &Path, name: &str, manifest: &str) {
    let dir = extensions.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.toml"), manifest).unwrap();
}

/// A Bundled extension's own `manifest`, its `"./<name>"` command pointed at
/// `binary`, where cargo built it.
pub fn bundled(manifest: &str, binary: &str) -> String {
    let name = Path::new(binary)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a binary path");
    manifest.replace(&format!("\"./{name}\""), &format!("{binary:?}"))
}

/// A Front end on the Daemon's `socket`, once the Daemon listens on it, with
/// reads timing out after `read_timeout`.
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
    stream.set_read_timeout(Some(read_timeout)).unwrap();
    Channel::handshake(
        stream.try_clone().unwrap(),
        stream,
        &Hello::new(&WireFormat::ALL),
    )
    .unwrap()
}
