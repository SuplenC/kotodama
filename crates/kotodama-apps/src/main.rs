//! The apps Bundled extension: every desktop application in the XDG data
//! directories as an Indexed item of kind `app`, kept up to date as apps are
//! installed and removed. Its desktop actions, such as "New Private Window",
//! are extra Actions. See `items.rs` for its config table.
//!
//! # Owns
//!
//! - The protocol loop: the config, the Actions, and the Items sent.
//! - Watching the `applications` directories, and rescanning when they settle.
//! - Starting an app, and withdrawing its pick when it closes soon after.
//!
//! # Not here
//!
//! - Finding the apps: `scan.rs`, on `entry.rs` and `exec.rs`.
//! - The config table and the Items: `items.rs`.
//! - How an app is started and followed: `kotodama_extension::launch`.

#![forbid(unsafe_code)]

mod entry;
mod exec;
mod items;
mod scan;

use std::collections::BTreeMap;
use std::io;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::time::Duration;

use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::SentItems;
use kotodama_extension::Writer;
use kotodama_extension::launch;
use kotodama_extension::watch::RecursiveMode;
use kotodama_extension::watch::Watcher;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;

use crate::items::Settings;
use crate::scan::App;
use crate::scan::Environment;

/// How long the `applications` directories must stay still before they are
/// read again, so a package install that writes many files is read once,
/// whole.
const SETTLE: Duration = Duration::from_millis(200);

/// What the Extension's own threads send it.
enum Own {
    /// An `applications` directory changed, and this is the rescan of every one.
    Scanned(BTreeMap<String, App>),
    /// The app the run `invocation` started closed soon after it started.
    ExitedSoon { invocation: u64 },
}

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-apps", run)
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect()?;

    let exits = events.clone();
    let dirs = scan::application_dirs();
    let env = Environment::from_env();
    let apps = scan::scan(&dirs, &env);
    let watching = dirs.clone();
    // Rescans happen on the watching thread, so an Action never waits for one.
    let watcher = Watcher::start(
        "watch",
        RecursiveMode::Recursive,
        SETTLE,
        move |event, _| {
            event
                .paths
                .iter()
                .any(|path| watching.iter().any(|dir| path.starts_with(dir)))
        },
        {
            let dirs = dirs.clone();
            move |_| {
                let _ = events.send(Event::Own(Own::Scanned(scan::scan(&dirs, &env))));
            }
        },
    )
    .map_err(|err| kotodama_extension::log!("not watching for new apps: {err}"))
    .ok();
    if let Some(watcher) = &watcher {
        watcher.watch(dirs);
    }

    let mut extension = Extension {
        writer,
        apps,
        settings: Settings::default(),
        configured: false,
        sent: SentItems::default(),
        exits,
    };
    for event in received {
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Own(Own::Scanned(apps)) => {
                extension.apps = apps;
                extension.publish()?;
            }
            Event::Own(Own::ExitedSoon { invocation }) => {
                extension.writer.send(&FromExtension::WithdrawPick { invocation })?;
            }
            Event::Hangup => break,
        }
    }
    Ok(())
}

struct Extension {
    writer: Writer,
    apps: BTreeMap<String, App>,
    settings: Settings,
    /// Whether the config has arrived. No Items are sent before it.
    configured: bool,
    sent: SentItems,
    /// Where the threads reaping launched apps say one exited soon.
    exits: Sender<Event<Own>>,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), kotodama_protocol::Error> {
        match message {
            ToExtension::Config(config) => {
                self.settings = Settings::from_config(&config);
                self.configured = true;
                self.publish()
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => {
                let outcome = match self.run(invocation, &item.id, &action) {
                    Ok(()) => Outcome::Close,
                    Err(err) => Outcome::Failed {
                        message: err.to_string(),
                    },
                };
                self.writer.send(&FromExtension::ActionDone { invocation, outcome })
            }
            _ => Ok(()),
        }
    }

    /// Sends what changed in the Items since they were last sent.
    fn publish(&mut self) -> Result<(), kotodama_protocol::Error> {
        if !self.configured {
            return Ok(());
        }
        self.sent.publish(&mut self.writer, self.settings.items(&self.apps))
    }

    /// Starts the app, after the terminal if it runs in one, in its working
    /// directory if it names one. If it closes within
    /// [`Settings::withdraw_within`], everything it started gone, and no
    /// copy of it ran before, the run's pick is withdrawn.
    fn run(&self, invocation: u64, id: &str, action: &str) -> Result<(), RunError> {
        let app = self.apps.get(id).ok_or_else(|| RunError::gone(id))?;
        let command = items::command(app, action).ok_or_else(|| RunError::no_action(app, action))?;
        let terminal: &[String] = if app.terminal { &self.settings.terminal } else { &[] };
        let argv: Vec<String> = terminal.iter().chain(command).cloned().collect();
        let exits = self.exits.clone();
        // The app's own program, not the terminal's: the terminal runs
        // anyway when another of its windows is open.
        let program = command.first().map_or("", String::as_str);
        launch::launch_watched(
            &argv,
            app.dir.as_deref(),
            launch::App { id: &app.id, program },
            self.settings.withdraw_within,
            move || {
                let _ = exits.send(Event::Own(Own::ExitedSoon { invocation }));
            },
        )
        .map_err(RunError::Launch)
    }
}

/// Why an Action did not start its app. Its `Display` is the message of the
/// failed Outcome.
#[derive(Debug, thiserror::Error)]
enum RunError {
    #[error("the app `{0}` is gone")]
    Gone(String),
    #[error("{app} has no Action `{action}`")]
    NoAction { app: String, action: String },
    /// Starting the program failed; its error is the message as it is.
    #[error(transparent)]
    Launch(io::Error),
}

impl RunError {
    fn gone(id: &str) -> Self {
        RunError::Gone(id.to_owned())
    }

    fn no_action(app: &App, action: &str) -> Self {
        RunError::NoAction {
            app: app.name.clone(),
            action: action.to_owned(),
        }
    }
}
