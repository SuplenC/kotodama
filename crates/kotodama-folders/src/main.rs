//! The Folder source Bundled extension: the git repositories in the projects
//! folders, as Items of kind `project` that open in a tmux session named as
//! `tms` names it, and the executable files in the scripts folders, as Items
//! of kind `file` that run them. Both are kept up to date as the folders
//! change. See `settings.rs` for its config table.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::Sender;

use kotodama_extension::launch::{self, Ended};
use kotodama_extension::{Connection, Event, SentItems, Writer};
use kotodama_folders::items::{
    self, ARGUMENTS, COPY_PATH, FILE_MANAGER, LAZYGIT, OPEN, RUN, SHELL,
};
use kotodama_folders::projects::Project;
use kotodama_folders::scan::{Scan, Scanner};
use kotodama_folders::scripts::Script;
use kotodama_folders::settings::{Settings, fill};
use kotodama_protocol::{FromExtension, Job, JobState, Outcome, ToExtension};

enum Own {
    Scanned(Scan),
    /// A script run in the background exited.
    Ended {
        job: String,
        title: String,
        ended: Ended,
    },
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

    let scanned = events.clone();
    let scanner = Scanner::start(move |scan| {
        let _ = scanned.send(Event::Own(Own::Scanned(scan)));
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
    events: Sender<Event<Own>>,
    scanner: Scanner,
    home: Option<PathBuf>,
    settings: Settings,
    /// By Item id.
    projects: BTreeMap<String, Project>,
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
            .chain(
                self.scripts
                    .values()
                    .map(|script| items::script(script, home)),
            );
        self.sent.publish(&mut self.writer, all)
    }

    fn run(
        &mut self,
        id: &str,
        action: &str,
        arguments: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if let Some(project) = self.projects.get(id) {
            let command = match action {
                OPEN => &self.settings.open,
                SHELL => &self.settings.shell,
                LAZYGIT => &self.settings.lazygit,
                FILE_MANAGER => &self.settings.file_manager,
                COPY_PATH => &self.settings.copy_path,
                _ => return Err(format!("A project has no Action `{action}`")),
            };
            let argv = fill(command, &project.path, Some(&project.session()));
            return launch::launch(&argv, Some(&project.path)).map_err(|err| err.to_string());
        }
        let script = self
            .scripts
            .get(id)
            .ok_or_else(|| format!("{id} is no longer there"))?;
        let dir = script.path.parent().unwrap_or(Path::new("/"));
        match action {
            RUN => {}
            COPY_PATH => {
                let argv = fill(&self.settings.copy_path, &script.path, None);
                return launch::launch(&argv, Some(dir)).map_err(|err| err.to_string());
            }
            _ => return Err(format!("A script has no Action `{action}`")),
        }
        let mut argv = vec![script.path.to_string_lossy().into_owned()];
        if script.header.prompt {
            let typed = arguments.get(ARGUMENTS).map_or("", String::as_str);
            argv.extend(shlex::split(typed).ok_or("The arguments have a quote that isn't closed")?);
        }
        if script.header.terminal {
            let argv: Vec<String> = self.settings.terminal.iter().cloned().chain(argv).collect();
            return launch::launch(&argv, Some(dir)).map_err(|err| err.to_string());
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
        .map_err(|err| err.to_string())?;
        self.send(&FromExtension::JobProgress(Job {
            id: job,
            title,
            state: JobState::Running,
            progress: None,
            message: None,
        }))
        .map_err(|err| err.to_string())
    }

    /// Reports a background script's end; the Daemon notifies the desktop.
    fn ended(
        &mut self,
        job: String,
        title: String,
        ended: Ended,
    ) -> Result<(), kotodama_protocol::Error> {
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
