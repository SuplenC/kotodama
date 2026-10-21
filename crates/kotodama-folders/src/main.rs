//! The Folder source Bundled extension: the git repositories in the projects folders, as Items of
//! kind `project` that open in a tmux session named as `tms` names it, and the executable files in
//! the scripts folders, as Items of kind `file` that run them. Both are kept up to date as the
//! folders change. See `settings.rs` for its config table.
//!
//! # Owns
//!
//! - The protocol loop: the config, the Actions, and the Items sent.
//! - Starting what an Action runs, and reporting a background script as a Job.
//!
//! # Not here
//!
//! - Finding and watching the projects and scripts: the library's `scan`, `projects` and `scripts`.
//! - The config table, the commands and the Items: the library's `settings` and `items`.
//! - How a command is started and followed: `kotodama_extension::launch`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::Sender;

use kotodama_extension::Connection;
use kotodama_extension::Event;
use kotodama_extension::SentItems;
use kotodama_extension::Writer;
use kotodama_extension::launch;
use kotodama_extension::launch::Ended;
use kotodama_folders::items;
use kotodama_folders::items::ARGUMENTS;
use kotodama_folders::items::COPY_PATH;
use kotodama_folders::items::FILE_MANAGER;
use kotodama_folders::items::LAZYGIT;
use kotodama_folders::items::OPEN;
use kotodama_folders::items::RUN;
use kotodama_folders::items::SHELL;
use kotodama_folders::projects::Project;
use kotodama_folders::scan::Scan;
use kotodama_folders::scan::Scanner;
use kotodama_folders::scripts::Script;
use kotodama_folders::settings::Settings;
use kotodama_folders::settings::fill;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Job;
use kotodama_protocol::JobState;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;

/// What the Extension's own threads send it.
enum Own {
    /// The folders were searched, at the config's request or after they changed.
    Scanned(Scan),
    /// A script run in the background exited.
    Ended { job: String, title: String, ended: Ended },
}

fn main() -> ExitCode {
    kotodama_extension::main("kotodama-folders", run)
}

fn run() -> Result<(), kotodama_protocol::Error> {
    let Connection {
        writer,
        events,
        received,
    } = kotodama_extension::connect()?;

    let results = events.clone();
    let scanner = Scanner::start(move |scan| {
        let _ = results.send(Event::Own(Own::Scanned(scan)));
    })?;

    let mut extension = Extension {
        writer,
        events,
        scanner,
        home: std::env::var_os("HOME").map(PathBuf::from),
        settings: Settings::default(),
        projects: BTreeMap::new(),
        scripts: BTreeMap::new(),
        sent: SentItems::default(),
        jobs: 0,
    };
    for event in received {
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Own(Own::Scanned(scan)) => extension.scanned(scan)?,
            Event::Own(Own::Ended { job, title, ended }) => extension.ended(job, title, ended)?,
            Event::Hangup => break,
        }
    }
    Ok(())
}

struct Extension {
    writer: Writer,
    /// Where a background script's thread says it ended.
    events: Sender<Event<Own>>,
    scanner: Scanner,
    /// Shown as `~` in the Items, and what a leading `~` in the config stands for.
    home: Option<PathBuf>,
    settings: Settings,
    /// By Item id.
    projects: BTreeMap<String, Project>,
    /// By Item id.
    scripts: BTreeMap<String, Script>,
    sent: SentItems,
    /// How many background jobs have started, which names the next.
    jobs: u64,
}

impl Extension {
    fn message(&mut self, message: ToExtension) -> Result<(), kotodama_protocol::Error> {
        match message {
            ToExtension::Config(config) => {
                self.settings = Settings::from_config(&config, self.home.as_deref());
                // Items follow once the folders are searched.
                self.scanner.search(self.settings.folders.clone());
                Ok(())
            }
            ToExtension::RunAction {
                invocation,
                action,
                item,
                arguments,
            } => {
                let outcome = match self.run(&item.id, &action, &arguments) {
                    Ok(()) => Outcome::Close,
                    Err(err) => Outcome::Failed {
                        message: err.to_string(),
                    },
                };
                self.send(&FromExtension::ActionDone { invocation, outcome })
            }
            _ => Ok(()),
        }
    }

    /// Sends what changed in the Items since they were last sent.
    fn scanned(&mut self, scan: Scan) -> Result<(), kotodama_protocol::Error> {
        // A folder named twice, or inside another, lists a thing once.
        self.projects = scan
            .projects
            .into_iter()
            .rev()
            .map(|project| (items::id(&project.path), project))
            .collect();
        self.scripts = scan
            .scripts
            .into_iter()
            .rev()
            .map(|script| (items::id(&script.path), script))
            .collect();
        let home = self.home.as_deref();
        let all = self
            .projects
            .values()
            .map(|project| items::project(project, home))
            .chain(self.scripts.values().map(|script| items::script(script, home)));
        self.sent.publish(&mut self.writer, all)
    }

    /// Runs `action` on the project or script whose Item is `id`.
    fn run(&mut self, id: &str, action: &str, arguments: &BTreeMap<String, String>) -> Result<(), RunError> {
        if let Some(project) = self.projects.get(id) {
            return self.run_project(project, action);
        }
        let script = self.scripts.get(id).ok_or_else(|| RunError::gone(id))?;
        let dir = script.path.parent().unwrap_or(Path::new("/"));
        match action {
            RUN => {}
            COPY_PATH => {
                let argv = fill(&self.settings.copy_path, &script.path, None);
                return launch::launch(&argv, Some(dir)).map_err(RunError::Launch);
            }
            _ => return Err(RunError::no_script_action(action)),
        }
        let mut argv = vec![script.path.to_string_lossy().into_owned()];
        if script.header.prompt {
            let typed = arguments.get(ARGUMENTS).map_or("", String::as_str);
            argv.extend(shlex::split(typed).ok_or(RunError::UnclosedQuote)?);
        }
        if script.header.terminal {
            let argv: Vec<String> = self.settings.terminal.iter().cloned().chain(argv).collect();
            return launch::launch(&argv, Some(dir)).map_err(RunError::Launch);
        }
        self.jobs += 1;
        let job = format!("run-{}", self.jobs);
        let title = script.title();
        let events = self.events.clone();
        let (ended_job, ended_title) = (job.clone(), title.clone());
        launch::background(&argv, Some(dir), move |ended| {
            let _ = events.send(Event::Own(Own::Ended {
                job: ended_job,
                title: ended_title,
                ended,
            }));
        })
        .map_err(RunError::Launch)?;
        self.send(&FromExtension::JobProgress(Job {
            id: job,
            title,
            state: JobState::Running,
            progress: None,
            message: None,
        }))
        .map_err(RunError::Send)
    }

    /// Runs the project Action `action`'s command in the project's folder.
    fn run_project(&self, project: &Project, action: &str) -> Result<(), RunError> {
        let command = match action {
            OPEN => &self.settings.open,
            SHELL => &self.settings.shell,
            LAZYGIT => &self.settings.lazygit,
            FILE_MANAGER => &self.settings.file_manager,
            COPY_PATH => &self.settings.copy_path,
            _ => return Err(RunError::no_project_action(action)),
        };
        let argv = fill(command, &project.path, Some(&project.session()));
        launch::launch(&argv, Some(&project.path)).map_err(RunError::Launch)
    }

    /// Reports a background script's end; the Daemon notifies the desktop.
    fn ended(&mut self, job: String, title: String, ended: Ended) -> Result<(), kotodama_protocol::Error> {
        let succeeded = ended.status.is_some_and(|status| status.success());
        let message = if succeeded {
            ended.last_line
        } else {
            let how = match ended.status {
                Some(status) => match status.code() {
                    Some(code) => format!("Exited with status {code}"),
                    None => format!("Stopped: {status}"),
                },
                None => "Lost track of it".to_owned(),
            };
            Some(match ended.last_line {
                Some(line) => format!("{how}: {line}"),
                None => how,
            })
        };
        self.send(&FromExtension::JobProgress(Job {
            id: job,
            title,
            state: if succeeded {
                JobState::Succeeded
            } else {
                JobState::Failed
            },
            progress: None,
            message,
        }))
    }

    fn send(&mut self, message: &FromExtension) -> Result<(), kotodama_protocol::Error> {
        self.writer.send(message)
    }
}

/// Why an Action failed. Its `Display` is the message of the failed Outcome.
#[derive(Debug, thiserror::Error)]
enum RunError {
    /// The Item is neither a project nor a script any more: the folders changed since it was sent.
    #[error("{0} is no longer there")]
    Gone(String),
    #[error("A project has no Action `{0}`")]
    NoProjectAction(String),
    #[error("A script has no Action `{0}`")]
    NoScriptAction(String),
    /// The arguments typed for a script with `args=prompt` don't split into words.
    #[error("The arguments have a quote that isn't closed")]
    UnclosedQuote,
    /// Starting the command failed; its error is the message as it is.
    #[error(transparent)]
    Launch(io::Error),
    /// The background script started, but the Job saying so couldn't be sent.
    #[error(transparent)]
    Send(kotodama_protocol::Error),
}

impl RunError {
    fn gone(id: &str) -> Self {
        RunError::Gone(id.to_owned())
    }

    fn no_project_action(action: &str) -> Self {
        RunError::NoProjectAction(action.to_owned())
    }

    fn no_script_action(action: &str) -> Self {
        RunError::NoScriptAction(action.to_owned())
    }
}
