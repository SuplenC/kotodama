//! The Folder source Bundled extension: the git repositories in the projects
//! folders, as Items of kind `project` that open in a tmux session named as
//! `tms` names it, and the executable files in the scripts folders, as Items
//! of kind `file` that run them. Both are kept up to date as the folders
//! change. See `settings.rs` for its config table.

use std::collections::BTreeMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::thread;

use kotodama_folders::items::{
    self, ARGUMENTS, COPY_PATH, FILE_MANAGER, LAZYGIT, OPEN, RUN, SHELL,
};
use kotodama_folders::launch::{self, Ended};
use kotodama_folders::projects::Project;
use kotodama_folders::scan::{Scan, Scanner};
use kotodama_folders::scripts::Script;
use kotodama_folders::settings::{Settings, fill};
use kotodama_protocol::{
    Channel, ChannelWriter, FromExtension, Hello, Job, JobState, Outcome, ToExtension, WireFormat,
};

enum Event {
    Message(ToExtension),
    Scanned(Scan),
    /// A script run in the background exited.
    Ended {
        job: String,
        title: String,
        ended: Ended,
    },
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
            eprintln!("kotodama-folders: {err}");
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
                        eprintln!("kotodama-folders: skipped a message: {err}");
                    }
                    Err(err) => {
                        eprintln!("kotodama-folders: {err}");
                        break;
                    }
                }
            }
            let _ = messages.send(Event::Hangup);
        })?;

    let scanned = events.clone();
    let scanner = Scanner::start(move |scan| {
        let _ = scanned.send(Event::Scanned(scan));
    })?;

    let mut extension = Extension {
        writer,
        events,
        scanner,
        home: std::env::var_os("HOME").map(PathBuf::from),
        settings: Settings::default(),
        projects: BTreeMap::new(),
        scripts: BTreeMap::new(),
        sent: items::SentItems::default(),
        jobs: 0,
    };
    for event in received {
        match event {
            Event::Message(message) => extension.message(message)?,
            Event::Scanned(scan) => extension.scanned(scan)?,
            Event::Ended { job, title, ended } => extension.ended(job, title, ended)?,
            Event::Hangup => break,
        }
    }
    Ok(())
}

struct Extension {
    writer: ChannelWriter<Stdout>,
    events: Sender<Event>,
    scanner: Scanner,
    home: Option<PathBuf>,
    settings: Settings,
    /// By Item id.
    projects: BTreeMap<String, Project>,
    scripts: BTreeMap<String, Script>,
    sent: items::SentItems,
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
        let (removed, added) = self.sent.update(all);
        if !removed.is_empty() {
            self.send(&FromExtension::RemoveItems { ids: removed })?;
        }
        if !added.is_empty() {
            self.send(&FromExtension::AddItems { items: added })?;
        }
        Ok(())
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
            return launch::launch(&argv, &project.path).map_err(|err| err.to_string());
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
                return launch::launch(&argv, dir).map_err(|err| err.to_string());
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
            return launch::launch(&argv, dir).map_err(|err| err.to_string());
        }
        self.jobs += 1;
        let job = format!("run-{}", self.jobs);
        let title = script.title();
        let events = self.events.clone();
        let (ended_job, ended_title) = (job.clone(), title.clone());
        launch::background(&argv, dir, move |ended| {
            let _ = events.send(Event::Ended {
                job: ended_job,
                title: ended_title,
                ended,
            });
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
