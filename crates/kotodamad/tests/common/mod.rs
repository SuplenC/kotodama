//! The harness the Daemon's tests drive it through: Extensions and Front ends
//! are channels here, and starting a process or typing is recorded instead of
//! done.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use kotodama_protocol::*;
use kotodamad::clock::{Clock, DAY_SECS};
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, FrontEndId, Launcher, Output};
use kotodamad::discovery::Found;
use kotodamad::history::{History, Store};
use kotodamad::usage::Entry;

#[derive(Clone, Default)]
pub struct Launches(
    pub Rc<RefCell<Vec<(ExtensionId, u64)>>>,
    pub Rc<RefCell<Vec<(ExtensionId, u64)>>>,
);

impl Launcher for Launches {
    fn start(&mut self, extension: ExtensionId, generation: u64, _found: &Found) {
        self.0.borrow_mut().push((extension, generation));
    }

    fn stop(&mut self, extension: ExtensionId, generation: u64) {
        self.1.borrow_mut().push((extension, generation));
    }
}

/// How many messages a test Extension's channel holds before it counts as full.
pub const OUTBOX: usize = 64;

/// What the Daemon typed or pasted, the programs it started, and the desktop
/// notifications it sent, as summary and body.
#[derive(Clone, Default)]
pub struct Outputs(
    pub Rc<RefCell<Vec<Output>>>,
    pub Rc<RefCell<Vec<Vec<String>>>>,
    pub Rc<RefCell<Vec<(String, String)>>>,
);

impl Desktop for Outputs {
    fn output(&mut self, output: Output) {
        self.0.borrow_mut().push(output);
    }

    fn launch(&mut self, command: Vec<String>) {
        self.1.borrow_mut().push(command);
    }

    fn notify(&mut self, summary: String, body: String) {
        self.2.borrow_mut().push((summary, body));
    }
}

/// The History as last saved, what was put in the usage log, and what was
/// forgotten of the log: `None` for all of it, or an Extension's name.
#[derive(Clone, Default)]
pub struct Saved(
    pub Rc<RefCell<Option<History>>>,
    pub Rc<RefCell<Vec<Entry>>>,
    pub Rc<RefCell<Vec<Option<String>>>>,
);

impl Store for Saved {
    fn save(&mut self, history: &History) {
        *self.0.borrow_mut() = Some(history.clone());
    }

    fn log(&mut self, entry: Entry) {
        self.1.borrow_mut().push(entry);
    }

    fn forget_usage(&mut self, extension: Option<&str>) {
        self.2.borrow_mut().push(extension.map(str::to_owned));
    }
}

/// When every test's Daemon starts: 2026-10-19 at 09:00 UTC.
pub const START: u64 = 1_792_400_400;

pub struct Harness {
    pub daemon: Daemon<Launches, Outputs, Saved>,
    pub started: Instant,
    pub now: Instant,
    pub launches: Launches,
    pub outputs: Outputs,
    pub saved: Saved,
    pub next_front_end: u64,
}

/// One running Extension: what the Daemon sent it.
pub struct Ext {
    pub id: ExtensionId,
    pub generation: u64,
    pub rx: Receiver<ToExtension>,
}

pub struct FrontEnd {
    pub id: FrontEndId,
    pub rx: Receiver<ToFrontEnd>,
    /// Set once the Daemon cuts this Front end off.
    pub closed: Arc<AtomicBool>,
}

pub fn manifest(toml: &str) -> Found {
    Found::new(Manifest::from_toml(toml).unwrap(), PathBuf::from("/ext")).unwrap()
}

pub fn plain(name: &str) -> Found {
    manifest(&format!("name = \"{name}\"\ncommand = [\"x\"]"))
}

pub fn action(id: &str) -> Action {
    Action {
        id: id.into(),
        title: id.into(),
        shortcut: None,
        arguments: vec![],
    }
}

pub fn item(id: &str, title: &str, kind: ItemKind) -> Item {
    Item {
        id: id.into(),
        kind,
        title: title.into(),
        subtitle: None,
        icon: None,
        keyword: None,
        identity: None,
        keywords: vec![],
        actions: vec![action("open")],
    }
}

pub fn app(title: &str) -> Item {
    let id = title.to_lowercase().replace(' ', "-");
    item(
        &id,
        title,
        ItemKind::App {
            desktop_id: format!("{id}.desktop"),
        },
    )
}

pub fn project(id: &str) -> Item {
    item(
        id,
        id,
        ItemKind::Project {
            path: format!("/src/{id}").into(),
        },
    )
}

impl Harness {
    pub fn new(extensions: Vec<Found>) -> Self {
        Self::with_config(extensions, Table::new())
    }

    pub fn with_config(extensions: Vec<Found>, config: Table) -> Self {
        Self::with_history(extensions, config, History::default())
    }

    pub fn with_history(extensions: Vec<Found>, config: Table, history: History) -> Self {
        let now = Instant::now();
        let launches = Launches::default();
        let outputs = Outputs::default();
        let saved = Saved::default();
        let mut daemon = Daemon::new(
            extensions,
            config,
            history,
            launches.clone(),
            outputs.clone(),
            saved.clone(),
            now,
        );
        daemon.set_clock(Clock::starting_at(START));
        Harness {
            daemon,
            started: now,
            now,
            launches,
            outputs,
            saved,
            next_front_end: 0,
        }
    }

    pub fn event(&mut self, event: Event) {
        self.daemon.handle(event, self.now);
    }

    pub fn advance(&mut self, by: Duration) {
        self.now += by;
        self.daemon.tick(self.now);
    }

    /// Waits for the next `hour` o'clock, in UTC, the Daemon's time zone in
    /// tests.
    pub fn advance_to(&mut self, hour: u64) {
        let time_of_day = (START + self.elapsed().as_secs()) % DAY_SECS;
        let wait = (hour * 3600 + DAY_SECS - time_of_day) % DAY_SECS;
        self.advance(Duration::from_secs(if wait == 0 { DAY_SECS } else { wait }));
    }

    /// How long the Daemon has run.
    pub fn elapsed(&self) -> Duration {
        self.now.duration_since(self.started)
    }

    /// Completes the handshake of the latest launch of `extension`.
    pub fn up(&mut self, extension: usize) -> Ext {
        self.up_with_room(extension, OUTBOX)
    }

    /// Like `up`, with room for only `room` messages the Extension hasn't read.
    pub fn up_with_room(&mut self, extension: usize, room: usize) -> Ext {
        let id = ExtensionId(extension);
        let generation = self
            .launches
            .0
            .borrow()
            .iter()
            .rev()
            .find(|(ext, _)| *ext == id)
            .expect("the extension was never started")
            .1;
        let (tx, rx) = mpsc::sync_channel(room);
        self.event(Event::ExtensionUp {
            extension: id,
            generation,
            sender: tx,
        });
        Ext { id, generation, rx }
    }

    pub fn send(&mut self, ext: &Ext, message: FromExtension) {
        self.event(Event::FromExtension {
            extension: ext.id,
            generation: ext.generation,
            message,
        });
    }

    pub fn add(&mut self, ext: &Ext, items: Vec<Item>) {
        self.send(ext, FromExtension::AddItems { items });
    }

    pub fn down(&mut self, ext: &Ext) {
        self.event(Event::ExtensionDown {
            extension: ext.id,
            generation: ext.generation,
        });
    }

    pub fn front_end(&mut self) -> FrontEnd {
        self.front_end_with_room(OUTBOX)
    }

    /// A Front end with room for only `room` messages it hasn't read.
    pub fn front_end_with_room(&mut self, room: usize) -> FrontEnd {
        let id = FrontEndId(self.next_front_end);
        self.next_front_end += 1;
        let (tx, rx) = mpsc::sync_channel(room);
        let closed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&closed);
        self.event(Event::FrontEndUp {
            front_end: id,
            sender: tx,
            disconnect: Box::new(move || flag.store(true, Ordering::SeqCst)),
        });
        let front_end = FrontEnd { id, rx, closed };
        front_end.drain();
        front_end
    }

    pub fn front_end_sends(&mut self, front_end: &FrontEnd, message: FromFrontEnd) {
        self.event(Event::FromFrontEnd {
            front_end: front_end.id,
            message,
        });
    }

    pub fn query(&mut self, front_end: &FrontEnd, text: &str) -> Vec<ShownResult> {
        self.front_end_sends(front_end, FromFrontEnd::Query { text: text.into() });
        front_end.results().expect("a query is always answered")
    }
}

impl Ext {
    pub fn drain(&self) -> Vec<ToExtension> {
        self.rx.try_iter().collect()
    }
}

impl FrontEnd {
    pub fn drain(&self) -> Vec<ToFrontEnd> {
        self.rx.try_iter().collect()
    }

    /// The latest Results sent, if any were.
    pub fn results(&self) -> Option<Vec<ShownResult>> {
        self.drain()
            .into_iter()
            .rev()
            .find_map(|message| match message {
                ToFrontEnd::Results { results } => Some(results),
                _ => None,
            })
    }
}

pub fn titles(rows: &[ShownResult]) -> Vec<&str> {
    rows.iter().map(|row| row.item.title.as_str()).collect()
}

pub fn run(h: &mut Harness, ui: &FrontEnd, owner: &str, id: &str, by: &str, action: &str) {
    h.front_end_sends(
        ui,
        FromFrontEnd::RunAction {
            result: ResultRef {
                extension: owner.into(),
                id: id.into(),
            },
            action: ActionRef {
                extension: by.into(),
                id: action.into(),
            },
            arguments: BTreeMap::new(),
        },
    );
}

/// The merged config of one file holding `toml`.
pub fn config(toml: &str) -> Table {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.toml"), toml).unwrap();
    kotodamad::config::load(&[dir.path().to_owned()]).unwrap()
}
