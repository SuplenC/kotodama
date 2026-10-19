//! The apps Bundled extension: every desktop application in the XDG data
//! directories as an Indexed item of kind `app`, kept up to date as apps are
//! installed and removed. Its desktop actions, such as "New Private Window",
//! are extra Actions. See `items.rs` for its config table.

mod entry;
mod exec;
mod items;
mod launch;
mod scan;
mod watch;

use std::collections::BTreeMap;
use std::io::{self, Stdout};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use kotodama_protocol::{
    Channel, ChannelWriter, FromExtension, Hello, Outcome, ToExtension, WireFormat,
};

use items::{DESKTOP_ACTION, OPEN, Sent, Settings};
use scan::{App, Environment};

enum Event {
    Message(ToExtension),
    /// An `applications` directory changed.
    Changed,
    /// The Daemon hung up, or its messages can't be read any more.
    Hangup,
}

/// Keeps resident memory down, as the Daemon does: one malloc arena, and no
/// transparent huge pages.
fn lean_memory() {
    #[cfg(target_os = "linux")]
    // SAFETY: both calls only set process-wide flags, before any thread starts.
    unsafe {
        libc::prctl(libc::PR_SET_THP_DISABLE, 1, 0, 0, 0);
        #[cfg(target_env = "gnu")]
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
}

fn main() -> ExitCode {
    lean_memory();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kotodama-apps: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let channel = Channel::handshake(io::stdin(), io::stdout(), &Hello::new(&WireFormat::ALL))?;
    let (mut reader, writer) = channel.split();
    let (events, received) = mpsc::channel();

    let messages = events.clone();
    thread::Builder::new()
        .name("read".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            loop {
                match reader.recv::<ToExtension>() {
                    Ok(Some(message)) => {
                        if messages.send(Event::Message(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(err @ kotodama_protocol::Error::Decode { .. }) => {
                        eprintln!("kotodama-apps: skipped a message: {err}");
                    }
                    Err(err) => {
                        eprintln!("kotodama-apps: {err}");
                        break;
                    }
                }
            }
            let _ = messages.send(Event::Hangup);
        })?;

    let dirs = scan::application_dirs();
    let _watcher = watch::watch(dirs.clone(), move || {
        let _ = events.send(Event::Changed);
    })
    .map_err(|err| eprintln!("kotodama-apps: not watching for new apps: {err}"))
    .ok();

    let env = Environment::from_env();
    let mut extension = Extension {
        writer,
        apps: scan::scan(&dirs, &env),
        settings: None,
        sent: Sent::default(),
    };
    for event in received {
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Changed => {
                extension.apps = scan::scan(&dirs, &env);
                extension.publish()?;
            }
            Event::Hangup => break,
        }
    }
    Ok(())
}

struct Extension {
    writer: ChannelWriter<Stdout>,
    apps: BTreeMap<String, App>,
    /// `None` until the config arrives, before which no Items are sent.
    settings: Option<Settings>,
    sent: Sent,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), kotodama_protocol::Error> {
        match message {
            ToExtension::Config(config) => {
                self.settings = Some(Settings::from_config(&config));
                self.publish()
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => {
                let outcome = match self.run(&item.id, &action) {
                    Ok(()) => Outcome::Close,
                    Err(message) => Outcome::Failed { message },
                };
                self.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })
            }
            _ => Ok(()),
        }
    }

    /// Sends what changed in the Items since they were last sent.
    fn publish(&mut self) -> Result<(), kotodama_protocol::Error> {
        let Some(settings) = &self.settings else {
            return Ok(());
        };
        let (removed, added) = self.sent.update(settings.items(&self.apps));
        if !removed.is_empty() {
            self.send(&FromExtension::RemoveItems { ids: removed })?;
        }
        if !added.is_empty() {
            self.send(&FromExtension::AddItems { items: added })?;
        }
        Ok(())
    }

    fn run(&self, id: &str, action: &str) -> Result<(), String> {
        let app = self
            .apps
            .get(id)
            .ok_or_else(|| format!("the app `{id}` is gone"))?;
        let command = if action == OPEN {
            &app.command
        } else {
            &action
                .strip_prefix(DESKTOP_ACTION)
                .and_then(|own| app.actions.iter().find(|action| action.id == own))
                .ok_or_else(|| format!("{} has no Action `{action}`", app.name))?
                .command
        };
        let default;
        let settings = match &self.settings {
            Some(settings) => settings,
            None => {
                default = Settings::default();
                &default
            }
        };
        let terminal = app.terminal.then_some(settings.terminal.as_slice());
        launch::launch(command, terminal, app.dir.as_deref()).map_err(|err| err.to_string())
    }

    fn send(&mut self, message: &FromExtension) -> Result<(), kotodama_protocol::Error> {
        self.writer.send(message)
    }
}
