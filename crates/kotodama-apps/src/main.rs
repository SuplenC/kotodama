//! The apps Bundled extension: every desktop application in the XDG data
//! directories as an Indexed item of kind `app`, kept up to date as apps are
//! installed and removed. Its desktop actions, such as "New Private Window",
//! are extra Actions. See `items.rs` for its config table.

mod entry;
mod exec;
mod items;
mod scan;

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::mpsc::Sender;
use std::time::Duration;

use kotodama_extension::watch::{RecursiveMode, Watcher};
use kotodama_extension::{Connection, Event, SentItems, Writer, launch};
use kotodama_protocol::{FromExtension, Outcome, ToExtension};

use items::Settings;
use scan::{App, Environment};

/// How long the `applications` directories must stay still before they are
/// read again, so a package install that writes many files is read once,
/// whole.
const SETTLE: Duration = Duration::from_millis(200);

/// What the Extension's own threads send it.
enum Own {
    /// An `applications` directory changed, and these are the apps now.
    Scanned(BTreeMap<String, App>),
    /// The app the run `invocation` started exited soon after it started.
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
    // Rescans happen on the watching thread, so an Action never waits for one.
    let dirs = scan::application_dirs();
    let env = Environment::from_env();
    let apps = scan::scan(&dirs, &env);
    let watching = dirs.clone();
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
                extension
                    .writer
                    .send(&FromExtension::WithdrawPick { invocation })?;
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
                    Err(message) => Outcome::Failed { message },
                };
                self.writer.send(&FromExtension::ActionDone {
                    invocation,
                    outcome,
                })
            }
            _ => Ok(()),
        }
    }

    /// Sends what changed in the Items since they were last sent.
    fn publish(&mut self) -> Result<(), kotodama_protocol::Error> {
        if !self.configured {
            return Ok(());
        }
        self.sent
            .publish(&mut self.writer, self.settings.items(&self.apps))
    }

    /// Starts the app, after the terminal if it runs in one, in its working
    /// directory if it names one. If it exits within
    /// [`Settings::withdraw_within`], the run's pick is withdrawn: this
    /// tells a mistake only when the process started is the app, not a
    /// launcher that hands off to another process and exits.
    fn run(&self, invocation: u64, id: &str, action: &str) -> Result<(), String> {
        let app = self
            .apps
            .get(id)
            .ok_or_else(|| format!("the app `{id}` is gone"))?;
        let command = items::command(app, action)
            .ok_or_else(|| format!("{} has no Action `{action}`", app.name))?;
        let terminal: &[String] = if app.terminal {
            &self.settings.terminal
        } else {
            &[]
        };
        let argv: Vec<String> = terminal.iter().chain(command).cloned().collect();
        let within = self.settings.withdraw_within;
        let exits = self.exits.clone();
        launch::launch_timed(&argv, app.dir.as_deref(), move |ran| {
            if ran < within {
                let _ = exits.send(Event::Own(Own::ExitedSoon { invocation }));
            }
        })
        .map_err(|err| err.to_string())
    }
}
