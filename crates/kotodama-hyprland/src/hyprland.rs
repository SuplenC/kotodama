//! Hyprland's two sockets, in `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`:
//! `.socket.sock` answers one request per connection, as `hyprctl` sends
//! them, and `.socket2.sock` streams events, one line each.
//!
//! # Owns
//!
//! - Finding the running Hyprland instance, and waiting for one.
//! - Requests and dispatches on `.socket.sock`, and which language its dispatches are written in.
//! - Where its Wayland socket is.
//! - Reading `.socket2.sock` line by line.
//!
//! # Not here
//!
//! - What an event line means: [`crate::events`].
//! - What a request's answer means: [`crate::binds`] and [`crate::windows`].

use std::ffi::OsStr;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use serde::Deserialize;

use crate::events::Change;

/// How long a request may take before it counts as failed, so a hung
/// Hyprland can't hang the Extension; `hyprctl` waits as long.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Why Hyprland could not be found or did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No instance's event socket took a connection.
    #[error("no Hyprland in {}", .dir.display())]
    NotRunning { dir: PathBuf },
    #[error("Couldn't reach Hyprland")]
    Unreachable(#[source] io::Error),
    /// Hyprland answered a dispatch with something other than `ok`.
    #[error("Hyprland: {answer}")]
    Refused { answer: String },
}

impl Error {
    fn not_running(dir: PathBuf) -> Self {
        Error::NotRunning { dir }
    }

    fn unreachable(err: io::Error) -> Self {
        Error::Unreachable(err)
    }

    fn refused(answer: &str) -> Self {
        Error::Refused {
            answer: answer.to_owned(),
        }
    }
}

/// The language Hyprland's config is written in, which is also the language
/// of its dispatches. Hyprland picks it once, when it starts, from the
/// config file's extension, so it holds for the instance's whole life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// `hyprland.conf`: a dispatch is a dispatcher and its argument, as
    /// `focuswindow address:0x…`.
    #[default]
    Hyprlang,
    /// `hyprland.lua`, from Hyprland 0.56: a dispatch is a Lua expression
    /// Hyprland runs as `hl.dispatch(…)`, as `hl.dsp.focus({ window = … })`.
    Lua,
}

impl Language {
    /// The language `j/status` names in its `configProvider`. Hyprlang for
    /// anything else, which is what a Hyprland older than 0.56 answers: it
    /// has no `status` request, and only hyprlang.
    pub fn from_status(answer: &str) -> Language {
        #[derive(Deserialize)]
        struct Status {
            #[serde(rename = "configProvider")]
            config_provider: String,
        }
        match serde_json::from_str::<Status>(answer) {
            Ok(status) if status.config_provider == "lua" => Language::Lua,
            _ => Language::Hyprlang,
        }
    }
}

/// The Hyprland instance the Extension runs in.
#[derive(Clone, Debug)]
pub struct Hyprland {
    dir: PathBuf,
}

impl Hyprland {
    /// The instances that may be running, best first: the one named by
    /// `HYPRLAND_INSTANCE_SIGNATURE`, then every other in
    /// `$XDG_RUNTIME_DIR/hypr/`, newest first. The variable may be missing,
    /// when the Extension started before Hyprland's environment reached the
    /// service manager, or stale, when Hyprland restarted since.
    pub fn candidates(signature: Option<&OsStr>, runtime: &Path) -> Vec<Hyprland> {
        let hypr = runtime.join("hypr");
        let named = signature
            .filter(|signature| !signature.is_empty())
            .map(|signature| hypr.join(signature));
        let mut others: Vec<(SystemTime, PathBuf)> = hypr
            .read_dir()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|dir| Some(dir) != named.as_ref())
            .filter_map(|dir| {
                let made = dir.join(".socket2.sock").metadata().ok()?.modified().ok()?;
                Some((made, dir))
            })
            .collect();
        others.sort_unstable_by(|a, b| b.cmp(a));
        named
            .into_iter()
            .chain(others.into_iter().map(|(_, dir)| dir))
            .map(|dir| Hyprland { dir })
            .collect()
    }

    /// The first of the [`candidates`](Hyprland::candidates) whose event
    /// socket takes a connection, and its events.
    ///
    /// # Errors
    ///
    /// [`Error::NotRunning`] when no candidate's event socket takes a connection.
    pub fn connect(signature: Option<&OsStr>, runtime: &Path) -> Result<(Hyprland, Events), Error> {
        Hyprland::candidates(signature, runtime)
            .into_iter()
            .find_map(|hyprland| Some((hyprland.clone(), hyprland.events().ok()?)))
            .ok_or_else(|| Error::not_running(runtime.join("hypr")))
    }

    /// Waits until a Hyprland runs, from this process's environment, and
    /// connects to it. Waiting costs nothing
    /// ([`runtime::wait_for`](kotodama_extension::runtime::wait_for));
    /// `waiting` hears why the first try failed. Without
    /// `XDG_RUNTIME_DIR` it waits forever.
    #[cfg(target_os = "linux")]
    pub fn wait(waiting: impl FnOnce(&str)) -> (Hyprland, Events) {
        let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE");
        let Some(runtime) = kotodama_extension::runtime::dir() else {
            waiting("XDG_RUNTIME_DIR isn't set");
            loop {
                std::thread::park();
            }
        };
        let hypr = runtime.join("hypr");
        kotodama_extension::runtime::wait_for(
            &[&hypr, &runtime],
            || Hyprland::connect(signature.as_deref(), &runtime).map_err(|err| err.to_string()),
            waiting,
        )
    }

    /// Sends one request, as `hyprctl` writes it: its flags, a `/`, then the
    /// command, as `j/clients` or `/dispatch …`. Returns the whole answer.
    ///
    /// # Errors
    ///
    /// The socket's error when it cannot be connected to, written to or read from, or when
    /// Hyprland takes longer than [`TIMEOUT`].
    pub fn request(&self, request: &str) -> io::Result<String> {
        let mut stream = UnixStream::connect(self.dir.join(".socket.sock"))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        stream.write_all(request.as_bytes())?;
        let mut answer = String::new();
        stream.read_to_string(&mut answer)?;
        Ok(answer)
    }

    /// Runs `dispatch <args>`.
    ///
    /// # Errors
    ///
    /// [`Error::Unreachable`] when the request fails, and [`Error::Refused`] with Hyprland's answer
    /// when it is not `ok`.
    pub fn dispatch(&self, args: &str) -> Result<(), Error> {
        let answer = self.request(&format!("/dispatch {args}")).map_err(Error::unreachable)?;
        match answer.trim() {
            "ok" => Ok(()),
            answer => Err(Error::refused(answer)),
        }
    }

    /// The language of this instance's config and dispatches, asked once
    /// with `j/status`; hyprlang when Hyprland doesn't answer.
    pub fn language(&self) -> Language {
        self.request("j/status")
            .map(|answer| Language::from_status(&answer))
            .unwrap_or_default()
    }

    /// The Wayland socket this instance serves, from the `hyprland.lock` it
    /// writes in its directory: its pid, then the socket's name in
    /// `$XDG_RUNTIME_DIR`, one per line. `None` without the file.
    pub fn wayland_socket(&self) -> Option<PathBuf> {
        let lock = std::fs::read_to_string(self.dir.join("hyprland.lock")).ok()?;
        let name = lock.lines().nth(1).map(str::trim).filter(|name| !name.is_empty())?;
        // The instance's directory is `$XDG_RUNTIME_DIR/hypr/<signature>`.
        let runtime = self.dir.parent()?.parent()?;
        Some(runtime.join(name))
    }

    /// Connects to the event socket.
    ///
    /// # Errors
    ///
    /// The socket's error when it takes no connection.
    pub fn events(&self) -> io::Result<Events> {
        UnixStream::connect(self.dir.join(".socket2.sock")).map(|stream| Events {
            lines: BufReader::new(stream),
        })
    }
}

/// Hyprland's event stream, keeping only the [`Change`]s.
#[derive(Debug)]
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

#[cfg(test)]
mod tests;
