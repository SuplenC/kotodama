//! `kotodama extension check <dir>`: starts the Extension in `dir` as the Daemon does, plays
//! the Daemon's part for a moment, and says in plain words what the Daemon would refuse,
//! drop or wait on forever. It reads the Manifest, runs the handshake, sends the config,
//! listens for Items, runs one Live query and, if asked, one Action, then hangs up.
//!
//! # Owns
//!
//! - What the check looks at, and its report.
//! - [`Seen`]: what the Extension has sent so far, and what the Daemon would make of each message.
//!
//! # Not here
//!
//! - What the real Daemon refuses: `kotodamad` (`discovery.rs` for the Manifest, `daemon.rs` and
//!   `link.rs` for messages). The check repeats those rules; a rule added there belongs here too.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_protocol::Action;
use kotodama_protocol::Channel;
use kotodama_protocol::ChannelReader;
use kotodama_protocol::ChannelWriter;
use kotodama_protocol::Clipboard;
use kotodama_protocol::ContributedTo;
use kotodama_protocol::Error;
use kotodama_protocol::FromExtension;
use kotodama_protocol::Hello;
use kotodama_protocol::Item;
use kotodama_protocol::LiveResult;
use kotodama_protocol::Manifest;
use kotodama_protocol::Outcome;
use kotodama_protocol::ToExtension;
use kotodama_protocol::ViewContent;
use kotodama_protocol::WireFormat;
use kotodama_protocol::is_reserved_shortcut;
use regex::Regex;

/// The Live query typed when `--query` isn't given: a word any Extension can answer, or not.
const DEFAULT_QUERY: &str = "test";
/// The name the Daemon's own Results go by, which no Extension may take.
const RESERVED_NAME: &str = "kotodama";
/// The Daemon keeps this many Live results of one query and drops the rest, as
/// `MAX_LIVE_RESULTS` in `kotodamad/src/daemon.rs`.
const MAX_LIVE_RESULTS: usize = 500;
/// An Extension that has said nothing for this long after the config is taken to have sent
/// its Items.
const QUIET: Duration = Duration::from_millis(500);
/// The longest the check waits for the hello, for the Items to stop coming, for a Live query's
/// `done` or for an Action's outcome. An Extension slower than that is slow in the menu too.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an Extension has to exit once its stdin closes, as it does when the Daemon exits.
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);
/// How many Items or Results the report names.
const LISTED: usize = 5;

/// What to check, from the command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) dir: PathBuf,
    /// The Live query to type, after the Manifest's first prefix.
    pub(crate) query: Option<String>,
    /// The id of an Item or a Live result whose primary Action to run.
    pub(crate) run: Option<String>,
}

impl Options {
    #[must_use]
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            query: None,
            run: None,
        }
    }
}

/// Checks the Extension in `options.dir`, writing the report to `out` as it goes. Returns
/// whether the Daemon would run it without a problem; warnings don't count.
pub(crate) fn check(options: &Options, out: &mut impl Write) -> bool {
    let mut report = Report::new(out);
    report.heading(format!("Checking the Extension in {}", options.dir.display()));
    // The program starts in the directory, where a path relative to the caller's leads nowhere.
    // The Daemon's directories are absolute.
    let dir = fs::canonicalize(&options.dir).unwrap_or_else(|_| options.dir.clone());
    if let Some(manifest) = manifest(&dir, &mut report)
        && let Some(program) = program(&dir, &manifest, &mut report)
        && let Some(mut run) = Run::start(&dir, &manifest, &program, &mut report)
    {
        run.converse(options);
    }
    report.summary()
}

/// Reads and checks `manifest.toml`, as the Daemon does when it finds the Extension.
fn manifest(dir: &Path, report: &mut Report<'_>) -> Option<Manifest> {
    let path = dir.join("manifest.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            report.problem(format!(
                "can't read {}: {err}; an Extension's directory holds its manifest.toml",
                path.display()
            ));
            return None;
        }
    };
    let manifest = match Manifest::from_toml(&text) {
        Ok(manifest) => manifest,
        Err(err) => {
            report.problem(format!("manifest.toml: {err}"));
            return None;
        }
    };
    let mut fine = true;
    if manifest.name == RESERVED_NAME {
        report.problem(format!("the name `{RESERVED_NAME}` is the Daemon's own; pick another"));
        fine = false;
    }
    for trigger in manifest.live_query.iter().flat_map(|live| &live.triggers) {
        if let Err(err) = Regex::new(trigger) {
            report.problem(format!(
                "the Live query trigger `{trigger}` isn't a regular expression in Rust's `regex` syntax, so the \
                 Daemon refuses the Manifest:\n{err}"
            ));
            fine = false;
        }
    }
    for contributed in &manifest.contributed_actions {
        if let Some(shortcut) = &contributed.action.shortcut
            && is_reserved_shortcut(shortcut)
        {
            report.problem(format!(
                "{}; the Daemon lists it among its problems",
                reserved(&contributed.action.id, shortcut)
            ));
            fine = false;
        }
    }
    if fine {
        report.ok(format!("manifest.toml names the Extension `{}`", manifest.name));
    }
    Some(manifest)
}

/// The program `command` names, if it is one the Daemon can run.
fn program(dir: &Path, manifest: &Manifest, report: &mut Report<'_>) -> Option<PathBuf> {
    let name = manifest.command.first()?;
    let program = dir.join(name);
    let Ok(metadata) = fs::metadata(&program) else {
        report.problem(format!(
            "`command` runs `{name}`, which isn't in the Extension's directory: the Daemon runs a relative path \
             from there, and never looks a program up on PATH. Name a script in the directory, with a `#!` line \
             for its interpreter"
        ));
        return None;
    };
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        report.problem(format!("`{name}` isn't an executable file: `chmod +x {name}`"));
        return None;
    }
    Some(program)
}

/// Says in plain words what an error reading from or writing to the Extension means.
fn plain(err: &Error) -> String {
    match err {
        Error::BadFrameHeader => "its stdout carries something that isn't a frame. A frame is the message's length in \
                                  ASCII digits, a newline, then the message; anything else, such as a log line, \
                                  goes to stderr"
            .to_owned(),
        Error::Decode { format, source } => format!("a message isn't valid {}: {source}", wire_name(*format)),
        Error::Io(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
            "it closed stdout before its hello, or in the middle of a frame".to_owned()
        }
        _ => err.to_string(),
    }
}

fn wire_name(format: WireFormat) -> &'static str {
    match format {
        WireFormat::Postcard => "postcard",
        WireFormat::Protobuf => "protobuf",
        WireFormat::MessagePack => "msgpack",
        WireFormat::Json => "JSON",
    }
}

/// Why the Daemon drops a binding, in its words.
fn reserved(action: &str, shortcut: &str) -> String {
    format!(
        "Action `{action}` is bound to `{shortcut}`, a key the menu keeps for itself; the Action is kept without the \
         binding"
    )
}

/// Why the Daemon refuses a `target_action`, in its words.
fn untargeted(action: &str) -> String {
    format!(
        "target_action names `{action}`, which the Manifest doesn't declare as a Contributed action with \
         `targeted = true`; it is offered on nothing"
    )
}

/// What the Daemon would make of one message.
#[derive(Debug, PartialEq, Eq)]
enum Finding {
    /// The Daemon refuses it and tells the Extension so, with this reason.
    Rejected(String),
    /// The Daemon ignores it, or the menu waits on something that never comes.
    Problem(String),
    /// The Daemon takes it, but drops part of it without a word to the Extension.
    Warning(String),
}

/// What the Extension has sent, and what the check has asked of it.
#[derive(Debug, Default)]
struct Seen {
    items: BTreeMap<String, Item>,
    /// The last `status`: whether it can do anything, and its note.
    status: Option<(bool, Option<String>)>,
    /// The results of each Live query the check sent, by id.
    live: BTreeMap<u64, Vec<LiveResult>>,
    /// The Live queries answered with `done`.
    finished: BTreeSet<u64>,
    /// The outcome of each Action the check ran, by invocation, once it has one.
    invocations: BTreeMap<u64, Option<Outcome>>,
}

impl Seen {
    /// Takes in one message from the Extension, and says what the Daemon would refuse or
    /// change in it.
    fn take(&mut self, manifest: &Manifest, message: FromExtension) -> Vec<Finding> {
        let mut findings = Vec::new();
        match message {
            FromExtension::AddItems { items } => {
                for item in items {
                    if let Some(owner) = item.kind.extension()
                        && owner != manifest.name
                    {
                        findings.push(Finding::Rejected(format!(
                            "Item `{}` has another Extension's kind `{}`; name your own kinds `{}.<kind>`",
                            item.id,
                            item.kind.name(),
                            manifest.name
                        )));
                        continue;
                    }
                    findings.extend(reserved_shortcuts(&item.actions));
                    self.items.insert(item.id.clone(), item);
                }
            }
            FromExtension::RemoveItems { ids } => {
                for id in ids {
                    self.items.remove(&id);
                }
            }
            FromExtension::LiveResults {
                live_query,
                results,
                done,
            } => {
                let Some(answered) = self
                    .live
                    .get_mut(&live_query)
                    .filter(|_| !self.finished.contains(&live_query))
                else {
                    findings.push(Finding::Problem(format!(
                        "live_results for Live query {live_query}, which the Daemon didn't send or which was already \
                         `done`: the Daemon ignores them. Answer a `live_query` with its `id`, and send nothing for \
                         it after `done`"
                    )));
                    return findings;
                };
                let answers = manifest.live_query.as_ref().is_some_and(|live| live.debounce_ms == 0);
                for result in &results {
                    findings.extend(reserved_shortcuts(&result.item.actions));
                    if result.answer && !answers {
                        findings.push(Finding::Rejected(format!(
                            "Live result `{}` is marked `answer`, which needs `debounce_ms = 0` in the Manifest's \
                             [live_query]; it is kept as a plain result",
                            result.item.id
                        )));
                    }
                }
                if answered.len() + results.len() > MAX_LIVE_RESULTS && answered.len() <= MAX_LIVE_RESULTS {
                    findings.push(Finding::Warning(format!(
                        "more than {MAX_LIVE_RESULTS} Live results for one query: the Daemon drops the rest"
                    )));
                }
                answered.extend(results);
                if done {
                    self.finished.insert(live_query);
                }
            }
            FromExtension::ActionDone { invocation, outcome } => {
                let Some(answer @ None) = self.invocations.get_mut(&invocation) else {
                    findings.push(Finding::Problem(format!(
                        "action_done for invocation {invocation}, which the Daemon didn't send or which was already \
                         answered: the Daemon ignores it. Answer each `run_action` once, with its `invocation`"
                    )));
                    return findings;
                };
                if matches!(outcome, Outcome::RunCatalogAction { .. }) && !manifest.action_catalog {
                    findings.push(Finding::Problem(
                        "the outcome `run_catalog_action` needs `action_catalog = true` in the Manifest".to_owned(),
                    ));
                }
                *answer = Some(outcome);
            }
            FromExtension::RequestActionCatalog if !manifest.action_catalog => {
                findings.push(Finding::Rejected(
                    "asked for the Action catalog without `action_catalog = true` in the Manifest".to_owned(),
                ));
            }
            FromExtension::Status { usable, note } => self.status = Some((usable, note)),
            FromExtension::TargetAction { action, .. } => findings.extend(undeclared_target(manifest, &action)),
            FromExtension::PushView(view)
            | FromExtension::UpdateView(view)
            | FromExtension::ReplaceView { with: view, .. } => {
                if let ViewContent::List { items, .. } = &view.content {
                    for item in items {
                        findings.extend(reserved_shortcuts(&item.actions));
                    }
                }
            }
            FromExtension::RequestActionCatalog
            | FromExtension::PopView { .. }
            | FromExtension::JobProgress(_)
            | FromExtension::WithdrawPick { .. } => {}
        }
        findings
    }
}

/// A rejection of a `target_action` for `action`, unless the Manifest declares
/// it targeted.
fn undeclared_target(manifest: &Manifest, action: &str) -> Option<Finding> {
    let declared = manifest
        .contributed_actions
        .iter()
        .any(|offered| offered.to == ContributedTo::Identities && offered.action.id == action);
    (!declared).then(|| Finding::Rejected(untargeted(action)))
}

/// A rejection for each of `actions` bound to a key the menu keeps for itself.
fn reserved_shortcuts(actions: &[Action]) -> impl Iterator<Item = Finding> {
    actions.iter().filter_map(|action| {
        let shortcut = action
            .shortcut
            .as_deref()
            .filter(|shortcut| is_reserved_shortcut(shortcut))?;
        Some(Finding::Rejected(reserved(&action.id, shortcut)))
    })
}

/// What the reading thread heard from the Extension.
#[derive(Debug)]
enum Heard {
    Message(FromExtension),
    /// A whole frame that didn't decode. The next one may.
    Undecodable(Error),
    /// Reading stopped: the Extension closed stdout, or wrote something that isn't a frame.
    Ended(Option<Error>),
}

/// The Extension, started and past its handshake, with the check playing the Daemon.
struct Run<'a, 'r> {
    manifest: &'a Manifest,
    report: &'a mut Report<'r>,
    child: Child,
    /// `None` once the check has hung up.
    writer: Option<ChannelWriter<ChildStdin>>,
    heard: Receiver<Heard>,
    /// Reading has stopped.
    ended: bool,
    seen: Seen,
}

impl<'a, 'r> Run<'a, 'r> {
    /// Starts the program as the Daemon does, and runs the Daemon's side of the handshake.
    fn start(dir: &Path, manifest: &'a Manifest, program: &Path, report: &'a mut Report<'r>) -> Option<Self> {
        let started = Command::new(program)
            .args(&manifest.command[1..])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();
        let mut child = match started {
            Ok(child) => child,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                report.problem(format!(
                    "couldn't start {}: {err}; is the interpreter its `#!` line names installed?",
                    program.display()
                ));
                return None;
            }
            Err(err) => {
                report.problem(format!("couldn't start {}: {err}", program.display()));
                return None;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            report.problem("couldn't open its stdin and stdout");
            stop(&mut child);
            return None;
        };
        let Some(channel) = handshake(stdin, stdout, report) else {
            stop(&mut child);
            return None;
        };
        report.ok(format!(
            "handshake: protocol {}, in {}",
            channel.peer(),
            wire_name(channel.format())
        ));
        let (reader, writer) = channel.split();
        Some(Self {
            manifest,
            report,
            child,
            writer: Some(writer),
            heard: listen(reader),
            ended: false,
            seen: Seen::default(),
        })
    }

    /// Plays the Daemon's part from the config to hanging up.
    fn converse(&mut self, options: &Options) {
        self.send(&ToExtension::Config(BTreeMap::new()));
        self.settle();
        self.report_items();
        self.live_query(options.query.as_deref());
        if let Some(id) = &options.run {
            self.run(id);
        }
        self.hang_up();
    }

    fn send(&mut self, message: &ToExtension) {
        let Some(writer) = &mut self.writer else {
            return;
        };
        if let Err(err) = writer.send(message) {
            self.report
                .problem(format!("couldn't write to its stdin: {}", plain(&err)));
            self.writer = None;
        }
    }

    /// Waits up to `wait` for one message and takes it in. Returns whether one came.
    fn hear(&mut self, wait: Duration) -> bool {
        if self.ended {
            return false;
        }
        let heard = match self.heard.recv_timeout(wait) {
            Ok(heard) => heard,
            Err(RecvTimeoutError::Timeout) => return false,
            Err(RecvTimeoutError::Disconnected) => {
                self.ended = true;
                return false;
            }
        };
        match heard {
            Heard::Message(message) => {
                for finding in self.seen.take(self.manifest, message) {
                    match finding {
                        Finding::Rejected(reason) => {
                            self.report.problem(&reason);
                            self.send(&ToExtension::Rejected { reason });
                        }
                        Finding::Problem(problem) => self.report.problem(problem),
                        Finding::Warning(warning) => self.report.warning(warning),
                    }
                }
            }
            Heard::Undecodable(err) => {
                let reason = plain(&err);
                self.report.problem(&reason);
                self.send(&ToExtension::Rejected { reason });
            }
            Heard::Ended(err) => {
                self.ended = true;
                let why = err.map_or_else(|| "it closed stdout, or exited".to_owned(), |err| plain(&err));
                self.report.problem(format!(
                    "{why}, while the Daemon was still connected: the Daemon restarts an Extension that stops, \
                     after a longer wait each time. An Extension runs until its stdin closes"
                ));
            }
        }
        true
    }

    /// Takes in whatever the Extension sends until it has been quiet for a moment.
    fn settle(&mut self) {
        let deadline = Instant::now() + ANSWER_TIMEOUT;
        while Instant::now() < deadline && self.hear(QUIET) {}
    }

    /// Takes in messages until `done` holds, for up to [`ANSWER_TIMEOUT`]. Returns whether it
    /// does.
    fn wait_for(&mut self, done: impl Fn(&Seen) -> bool) -> bool {
        let deadline = Instant::now() + ANSWER_TIMEOUT;
        loop {
            if done(&self.seen) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || self.ended {
                return false;
            }
            self.hear(left);
        }
    }

    fn report_items(&mut self) {
        let items = &self.seen.items;
        if !items.is_empty() {
            let ids: Vec<&str> = items.keys().map(String::as_str).collect();
            self.report
                .ok(format!("{}: {}", counted(items.len(), "Item"), listed(&ids)));
        } else if self.manifest.live_query.is_none() {
            self.report
                .warning("it sent no Items and the Manifest has no [live_query]: nothing of it shows in the menu");
        }
        for id in &self.manifest.commands {
            if !items.contains_key(id) {
                self.report
                    .warning(format!("`commands` lists `{id}`, which isn't among its Items"));
            }
        }
        match &self.seen.status {
            Some((true, note)) => self.report.ok(format!("status: usable{}", noted(note.as_deref()))),
            Some((false, note)) => self.report.warning(format!(
                "status: it can't do anything now{}; the search leaves its app out until it can",
                noted(note.as_deref())
            )),
            None => {}
        }
    }

    /// Types `query` as the user would: after the first prefix if the Manifest has one, else
    /// in the Extension's app, which routes every query to it.
    fn live_query(&mut self, query: Option<&str>) {
        let manifest = self.manifest;
        let Some(live) = &manifest.live_query else {
            if query.is_some() {
                self.report
                    .warning("--query does nothing: the Manifest has no [live_query]");
            }
            return;
        };
        let query = query.unwrap_or(DEFAULT_QUERY);
        let prefix = live.prefixes.first().cloned();
        let typed = format!("{}{query}", prefix.as_deref().unwrap_or_default());
        let id = 1;
        self.seen.live.insert(id, Vec::new());
        self.send(&ToExtension::LiveQuery {
            id,
            query: query.to_owned(),
            prefix,
        });
        if !self.wait_for(|seen| seen.finished.contains(&id)) {
            self.report.problem(format!(
                "no `done` for the Live query `{typed}` within {} s: send `live_results` with `done: true` once the \
                 last results are out, even when there are none",
                ANSWER_TIMEOUT.as_secs()
            ));
            return;
        }
        let results = &self.seen.live[&id];
        let titles: Vec<&str> = results.iter().map(|result| result.item.title.as_str()).collect();
        let titles = if titles.is_empty() {
            String::new()
        } else {
            format!(": {}", listed(&titles))
        };
        self.report.ok(format!(
            "Live query `{typed}`: {}{titles}",
            counted(results.len(), "result")
        ));
    }

    /// Runs the primary Action of the Item or Live result `id`.
    fn run(&mut self, id: &str) {
        let item = self.seen.items.get(id).cloned().or_else(|| {
            self.seen
                .live
                .values()
                .flatten()
                .find(|result| result.item.id == id)
                .map(|result| result.item.clone())
        });
        let Some(item) = item else {
            self.report
                .problem(format!("--run: no Item or Live result has the id `{id}`"));
            return;
        };
        let Some(action) = item.actions.first() else {
            return;
        };
        let invocation = 1;
        self.seen.invocations.insert(invocation, None);
        self.send(&ToExtension::RunAction {
            invocation,
            action: action.id.clone(),
            item: item.to_ref(),
            arguments: BTreeMap::new(),
        });
        if !self.wait_for(|seen| matches!(seen.invocations.get(&invocation), Some(Some(_)))) {
            self.report.problem(format!(
                "no `action_done` for Action `{}` on `{id}` within {} s: answer each `run_action` with its \
                 `invocation`, or the menu waits",
                action.id,
                ANSWER_TIMEOUT.as_secs()
            ));
            return;
        }
        let Some(Some(outcome)) = self.seen.invocations.get(&invocation) else {
            return;
        };
        let line = format!("Action `{}` on `{id}`: {}", action.id, described(outcome));
        if matches!(outcome, Outcome::Failed { .. }) {
            self.report.warning(line);
        } else {
            self.report.ok(line);
        }
    }

    /// Closes the Extension's stdin, as the Daemon's exit does, and waits for the Extension to
    /// exit too.
    fn hang_up(&mut self) {
        self.writer = None;
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => {
                    self.report.ok("it exits once its stdin closes");
                    return;
                }
                Ok(Some(status)) => {
                    self.report
                        .warning(format!("it exits with {status} once its stdin closes"));
                    return;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Ok(None) | Err(_) => break,
            }
        }
        self.report.warning(format!(
            "it was still running {} s after its stdin closed: exit when reading stdin gives end-of-file, so that \
             it never outlives the Daemon",
            EXIT_TIMEOUT.as_secs()
        ));
        stop(&mut self.child);
    }
}

impl Drop for Run<'_, '_> {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The Daemon's side of the handshake, on a thread so that an Extension that never sends its
/// hello doesn't hang the check.
fn handshake(
    stdin: ChildStdin,
    stdout: ChildStdout,
    report: &mut Report<'_>,
) -> Option<Channel<ChildStdout, ChildStdin>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(Channel::accept(stdout, stdin, &Hello::new(&WireFormat::ALL)));
    });
    match receiver.recv_timeout(ANSWER_TIMEOUT) {
        Ok(Ok(channel)) => Some(channel),
        Ok(Err(err)) => {
            report.problem(format!("handshake: {}", plain(&err)));
            None
        }
        Err(_) => {
            report.problem(format!(
                "no hello within {} s: an Extension starts by writing its hello to stdout, such as \
                 `{{\"version\":1,\"minor\":0,\"formats\":[\"json\"]}}` in a frame",
                ANSWER_TIMEOUT.as_secs()
            ));
            None
        }
    }
}

/// Reads the Extension's messages on a thread of their own, so the check can wait on them
/// with a timeout.
fn listen(mut reader: ChannelReader<ChildStdout>) -> Receiver<Heard> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let heard = match reader.recv::<FromExtension>() {
                Ok(Some(message)) => Heard::Message(message),
                Ok(None) => Heard::Ended(None),
                Err(err @ Error::Decode { .. }) => Heard::Undecodable(err),
                Err(err) => Heard::Ended(Some(err)),
            };
            let ended = matches!(heard, Heard::Ended(_));
            if sender.send(heard).is_err() || ended {
                return;
            }
        }
    });
    receiver
}

fn described(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Close => "closes the menu".to_owned(),
        Outcome::KeepOpen => "keeps the menu open".to_owned(),
        Outcome::Type { text } => format!("types `{text}`"),
        Outcome::Paste(clipboard) => format!("pastes {}", clipped(clipboard)),
        Outcome::Copy(clipboard) => format!("copies {}", clipped(clipboard)),
        Outcome::Failed { message } => format!("fails: {message}"),
        Outcome::RunCatalogAction { extension, action, .. } => {
            format!("runs `{extension}`'s Action `{action}` from the Action catalog")
        }
    }
}

fn clipped(clipboard: &Clipboard) -> String {
    match clipboard {
        Clipboard::Text(text) => format!("`{text}`"),
        Clipboard::Image { mime, .. } => format!("an image ({mime})"),
    }
}

fn noted(note: Option<&str>) -> String {
    note.map(|note| format!(" ({note})")).unwrap_or_default()
}

/// The first few of `names`, in backticks, and how many more there are.
fn listed(names: &[&str]) -> String {
    let mut listed: Vec<String> = names.iter().take(LISTED).map(|name| format!("`{name}`")).collect();
    if names.len() > LISTED {
        listed.push(format!("and {} more", names.len() - LISTED));
    }
    listed.join(", ")
}

/// The report, written as the check goes, so a slow Extension shows where it is slow.
struct Report<'w> {
    out: &'w mut dyn Write,
    problems: usize,
    warnings: usize,
}

impl<'w> Report<'w> {
    fn new(out: &'w mut dyn Write) -> Self {
        Self {
            out,
            problems: 0,
            warnings: 0,
        }
    }

    fn line(&mut self, mark: &str, text: impl Display) {
        // A report that can't be written has no one to tell.
        let _ = writeln!(self.out, "{mark:<11}{text}");
    }

    fn heading(&mut self, text: impl Display) {
        let _ = writeln!(self.out, "{text}");
    }

    fn ok(&mut self, text: impl Display) {
        self.line("  ok", text);
    }

    fn warning(&mut self, text: impl Display) {
        self.warnings += 1;
        self.line("  warning", text);
    }

    fn problem(&mut self, text: impl Display) {
        self.problems += 1;
        self.line("  problem", text);
    }

    /// Writes the tally, and returns whether there was no problem.
    fn summary(&mut self) -> bool {
        let tally = match (self.problems, self.warnings) {
            (0, 0) => "No problems.".to_owned(),
            (0, warnings) => format!("No problems, {}.", counted(warnings, "warning")),
            (problems, warnings) => format!("{}, {}.", counted(problems, "problem"), counted(warnings, "warning")),
        };
        let _ = writeln!(self.out, "{tally}");
        self.problems == 0
    }
}

fn counted(count: usize, noun: &str) -> String {
    format!("{count} {}", plural(count, noun))
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

#[cfg(test)]
mod tests;
