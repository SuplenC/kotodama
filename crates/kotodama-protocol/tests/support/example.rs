//! Runs one of the example Extensions in `examples/` the way the Daemon does:
//! its Manifest's command, from its directory, over its stdio.

use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;

use kotodama_protocol::Channel;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Hello;
use kotodama_protocol::Manifest;
use kotodama_protocol::ToExtension;
use kotodama_protocol::WireFormat;

pub fn dir(example: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(example)
}

pub fn manifest(example: &str) -> Manifest {
    let toml = std::fs::read_to_string(dir(example).join("manifest.toml")).expect("read the Manifest");
    Manifest::from_toml(&toml).expect("a valid Manifest")
}

/// An example Extension, killed on drop.
pub struct Example {
    child: Child,
    channel: Option<Channel<ChildStdout, ChildStdin>>,
}

impl Example {
    /// Starts the example and does the handshake, offering every Wire format.
    pub fn start(example: &str) -> Self {
        let manifest = manifest(example);
        let mut child = Command::new(dir(example).join(&manifest.command[0]))
            .args(&manifest.command[1..])
            .current_dir(dir(example))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the example");
        let (stdout, stdin) = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let channel = Channel::accept(stdout, stdin, &Hello::new(&WireFormat::ALL)).expect("the handshake");
        Example {
            child,
            channel: Some(channel),
        }
    }

    pub fn format(&self) -> WireFormat {
        self.channel().format()
    }

    pub fn send(&mut self, message: ToExtension) {
        self.channel_mut().send(&message).expect("send to the example");
    }

    pub fn recv(&mut self) -> FromExtension {
        self.channel_mut()
            .recv()
            .expect("a message the Daemon reads")
            .expect("the example closed the connection")
    }

    /// Closes the example's stdin, as the Daemon does when it goes away, and
    /// waits for it to exit.
    pub fn hang_up(mut self) -> bool {
        self.channel = None;
        self.child.wait().expect("wait for the example").success()
    }

    fn channel(&self) -> &Channel<ChildStdout, ChildStdin> {
        self.channel.as_ref().expect("the test has hung up")
    }

    fn channel_mut(&mut self) -> &mut Channel<ChildStdout, ChildStdin> {
        self.channel.as_mut().expect("the test has hung up")
    }
}

impl Drop for Example {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
