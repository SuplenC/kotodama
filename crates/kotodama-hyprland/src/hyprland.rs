//! Hyprland's two sockets, in `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`:
//! `.socket.sock` answers one request per connection, as `hyprctl` sends
//! them, and `.socket2.sock` streams events, one line each.

use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::events::Change;

/// How long a request may take before it counts as failed, so a hung
/// Hyprland can't hang the Extension; `hyprctl` waits as long.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The Hyprland instance the Extension runs in.
#[derive(Clone, Debug)]
pub struct Hyprland {
    dir: PathBuf,
}

impl Hyprland {
    /// The instance named by `HYPRLAND_INSTANCE_SIGNATURE`; `None` outside
    /// Hyprland.
    pub fn from_env() -> Option<Hyprland> {
        let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
        let runtime = env::var_os("XDG_RUNTIME_DIR")?;
        Some(Hyprland {
            dir: PathBuf::from(runtime).join("hypr").join(signature),
        })
    }

    /// Sends one request, as `hyprctl` writes it: its flags, a `/`, then the
    /// command, as `j/clients` or `/dispatch …`. Returns the whole answer.
    pub fn request(&self, request: &str) -> io::Result<String> {
        let mut stream = UnixStream::connect(self.dir.join(".socket.sock"))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        stream.write_all(request.as_bytes())?;
        let mut answer = String::new();
        stream.read_to_string(&mut answer)?;
        Ok(answer)
    }

    /// Runs `dispatch <args>`; Hyprland answers `ok`, or why it didn't.
    pub fn dispatch(&self, args: &str) -> Result<(), String> {
        let answer = self
            .request(&format!("/dispatch {args}"))
            .map_err(|err| format!("Couldn't reach Hyprland: {err}"))?;
        match answer.trim() {
            "ok" => Ok(()),
            err => Err(format!("Hyprland: {err}")),
        }
    }

    /// Connects to the event socket.
    pub fn events(&self) -> io::Result<Events> {
        UnixStream::connect(self.dir.join(".socket2.sock")).map(|stream| Events {
            lines: BufReader::new(stream),
        })
    }
}

/// Hyprland's event stream, keeping only the [`Change`]s.
pub struct Events {
    lines: BufReader<UnixStream>,
}

impl Events {
    /// Waits for the next event that changes an Item; `None` once Hyprland
    /// hangs up, which is it exiting.
    pub fn next_change(&mut self) -> Option<Change> {
        let mut line = Vec::new();
        loop {
            line.clear();
            match self.lines.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    // A title cut short may end inside a character.
                    let line = String::from_utf8_lossy(&line);
                    if let Some(change) = Change::parse(line.trim_end_matches('\n')) {
                        return Some(change);
                    }
                }
            }
        }
    }
}
