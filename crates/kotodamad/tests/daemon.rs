//! The Daemon's core, driven through its events: Extensions and Front ends are
//! channels here, and starting a process or typing is recorded instead of done.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use kotodama_protocol::*;
use kotodamad::daemon::{Daemon, Desktop, Event, ExtensionId, FrontEndId, Launcher, Output};
use kotodamad::discovery::Found;

#[derive(Clone, Default)]
struct Launches(Rc<RefCell<Vec<(ExtensionId, u64)>>>);

impl Launcher for Launches {
    fn start(&mut self, extension: ExtensionId, generation: u64, _found: &Found) {
        self.0.borrow_mut().push((extension, generation));
    }
}

#[derive(Clone, Default)]
struct Outputs(Rc<RefCell<Vec<Output>>>);

impl Desktop for Outputs {
    fn output(&mut self, output: Output) {
        self.0.borrow_mut().push(output);
    }
}

struct Harness {
    daemon: Daemon<Launches, Outputs>,
    now: Instant,
    launches: Launches,
    outputs: Outputs,
    next_front_end: u64,
}

/// One running Extension: what the Daemon sent it.
struct Ext {
    id: ExtensionId,
    generation: u64,
    rx: Receiver<ToExtension>,
}

struct FrontEnd {
    id: FrontEndId,
    rx: Receiver<ToFrontEnd>,
}

fn manifest(toml: &str) -> Found {
    Found::new(Manifest::from_toml(toml).unwrap(), PathBuf::from("/ext")).unwrap()
}

fn plain(name: &str) -> Found {
    manifest(&format!("name = \"{name}\"\ncommand = [\"x\"]"))
}

fn action(id: &str) -> Action {
    Action {
        id: id.into(),
        title: id.into(),
        shortcut: None,
        arguments: vec![],
    }
}

fn item(id: &str, title: &str, kind: ItemKind) -> Item {
    Item {
        id: id.into(),
        kind,
        title: title.into(),
        subtitle: None,
        icon: None,
        keyword: None,
        keywords: vec![],
        actions: vec![action("open")],
    }
}

fn app(title: &str) -> Item {
    let id = title.to_lowercase().replace(' ', "-");
    item(
        &id,
        title,
        ItemKind::App {
            desktop_id: format!("{id}.desktop"),
        },
    )
}

fn project(id: &str) -> Item {
    item(
        id,
        id,
        ItemKind::Project {
            path: format!("/src/{id}").into(),
        },
    )
}

impl Harness {
    fn new(extensions: Vec<Found>) -> Self {
        Self::with_config(extensions, Table::new())
    }

    fn with_config(extensions: Vec<Found>, config: Table) -> Self {
        let now = Instant::now();
        let launches = Launches::default();
        let outputs = Outputs::default();
        let daemon = Daemon::new(extensions, config, launches.clone(), outputs.clone(), now);
        Harness {
            daemon,
            now,
            launches,
            outputs,
            next_front_end: 0,
        }
    }

    fn event(&mut self, event: Event) {
        self.daemon.handle(event, self.now);
    }

    fn advance(&mut self, by: Duration) {
        self.now += by;
        self.daemon.tick(self.now);
    }

    /// Completes the handshake of the latest launch of `extension`.
    fn up(&mut self, extension: usize) -> Ext {
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
        let (tx, rx) = mpsc::channel();
        self.event(Event::ExtensionUp {
            extension: id,
            generation,
            sender: tx,
        });
        Ext { id, generation, rx }
    }

    fn send(&mut self, ext: &Ext, message: FromExtension) {
        self.event(Event::FromExtension {
            extension: ext.id,
            generation: ext.generation,
            message,
        });
    }

    fn add(&mut self, ext: &Ext, items: Vec<Item>) {
        self.send(ext, FromExtension::AddItems { items });
    }

    fn down(&mut self, ext: &Ext) {
        self.event(Event::ExtensionDown {
            extension: ext.id,
            generation: ext.generation,
        });
    }

    fn front_end(&mut self) -> FrontEnd {
        let id = FrontEndId(self.next_front_end);
        self.next_front_end += 1;
        let (tx, rx) = mpsc::channel();
        self.event(Event::FrontEndUp {
            front_end: id,
            sender: tx,
        });
        let front_end = FrontEnd { id, rx };
        front_end.drain();
        front_end
    }

    fn from_front_end(&mut self, front_end: &FrontEnd, message: FromFrontEnd) {
        self.event(Event::FromFrontEnd {
            front_end: front_end.id,
            message,
        });
    }

    fn query(&mut self, front_end: &FrontEnd, text: &str) -> Vec<ResultRow> {
        self.from_front_end(front_end, FromFrontEnd::Query { text: text.into() });
        front_end.results().expect("a query is always answered")
    }
}

impl Ext {
    fn drain(&self) -> Vec<ToExtension> {
        self.rx.try_iter().collect()
    }
}

impl FrontEnd {
    fn drain(&self) -> Vec<ToFrontEnd> {
        self.rx.try_iter().collect()
    }

    /// The latest Results sent, if any were.
    fn results(&self) -> Option<Vec<ResultRow>> {
        self.drain()
            .into_iter()
            .rev()
            .find_map(|message| match message {
                ToFrontEnd::Results { results } => Some(results),
                _ => None,
            })
    }
}

fn titles(rows: &[ResultRow]) -> Vec<&str> {
    rows.iter().map(|row| row.item.title.as_str()).collect()
}

#[test]
fn indexed_items_are_found_by_a_fuzzy_query_best_match_first() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(
        &apps,
        vec![
            app("Files"),
            app("Firefox"),
            app("Calculator"),
            app("Fire Alarm"),
        ],
    );
    let ui = h.front_end();

    let rows = h.query(&ui, "firef");
    assert_eq!(titles(&rows), ["Firefox"]);
    assert_eq!(rows[0].extension, "apps");
    assert_eq!(rows[0].highlights, [0, 1, 2, 3, 4]);

    assert_eq!(titles(&h.query(&ui, "fi")).len(), 3);
}

fn calculator() -> Found {
    manifest(
        "name = \"calc\"\ncommand = [\"x\"]\n[live_query]\ntriggers = ['^[0-9]']\ndebounce_ms = 0",
    )
}

fn web() -> Found {
    manifest(
        "name = \"web\"\ncommand = [\"x\"]\n[live_query]\nprefixes = [\"g \"]\ndebounce_ms = 100",
    )
}

fn live_queries(ext: &Ext) -> Vec<(u64, String, Option<String>)> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::LiveQuery { id, query, prefix } => Some((id, query, prefix)),
            _ => None,
        })
        .collect()
}

fn live(title: &str, score: f32, answer: bool) -> LiveResult {
    LiveResult {
        item: item(
            title,
            title,
            ItemKind::Url {
                url: format!("https://{title}"),
            },
        ),
        score: Fraction::new(score).unwrap(),
        answer,
    }
}

#[test]
fn a_live_query_waits_for_the_debounce_and_drops_the_routing_prefix() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    h.up(0);
    let web = h.up(1);
    web.drain();
    let ui = h.front_end();

    h.query(&ui, "g ru");
    h.advance(Duration::from_millis(50));
    h.query(&ui, "g rust");
    h.advance(Duration::from_millis(60));
    assert_eq!(live_queries(&web), [], "still inside the debounce");

    h.advance(Duration::from_millis(40));
    let sent = live_queries(&web);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].1.as_str(), sent[0].2.as_deref()),
        ("rust", Some("g "))
    );
}

#[test]
fn only_extensions_whose_prefix_or_trigger_matches_get_a_live_query() {
    let mut h = Harness::new(vec![calculator(), web()]);
    let calc = h.up(0);
    let web = h.up(1);
    let ui = h.front_end();

    h.query(&ui, "firefox");
    h.advance(Duration::from_secs(1));
    assert_eq!(live_queries(&calc), []);
    assert_eq!(live_queries(&web), []);

    h.query(&ui, "2+2");
    assert_eq!(live_queries(&calc).len(), 1, "debounce 0 sends at once");
    h.advance(Duration::from_secs(1));
    assert_eq!(live_queries(&web), []);
}

#[test]
fn changing_the_query_cancels_the_live_query_in_flight_and_drops_its_results() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    calc.drain();
    let ui = h.front_end();

    h.query(&ui, "2+2");
    let (id, ..) = live_queries(&calc)[0].clone();
    h.query(&ui, "2+3");
    let sent = calc.drain();
    assert!(
        sent.contains(&ToExtension::CancelLiveQuery { id }),
        "{sent:?}"
    );

    // A straggler for the old query is ignored.
    h.send(
        &calc,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("4", 1.0, true)],
            done: true,
        },
    );
    assert_eq!(ui.results(), None);
}

#[test]
fn a_late_result_goes_below_the_visible_rows_however_well_it_scores() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(
        &apps,
        vec![app("g rust one"), app("g rust two"), app("g rust three")],
    );
    let ui = h.front_end();
    h.from_front_end(&ui, FromFrontEnd::Viewport { rows: 2 });

    let shown = titles(&h.query(&ui, "g rust")).join(",");
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("perfect", 1.0, false)],
            done: true,
        },
    );

    let rows = ui.results().unwrap();
    let titles = titles(&rows);
    assert_eq!(
        titles[..2].join(","),
        shown.split(',').take(2).collect::<Vec<_>>().join(",")
    );
    assert_eq!(titles[2], "perfect", "first row below the visible ones");
}

#[test]
fn below_the_visible_rows_live_results_compete_with_indexed_items_on_score() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(&apps, vec![app("g rust"), app("g xrxuxsxt")]);
    let ui = h.front_end();
    h.from_front_end(&ui, FromFrontEnd::Viewport { rows: 1 });

    assert_eq!(titles(&h.query(&ui, "g rust")), ["g rust", "g xrxuxsxt"]);
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("perfect", 1.0, false), live("poor", 0.0, false)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust", "perfect", "g xrxuxsxt", "poor"]
    );
}

#[test]
fn an_answer_from_a_zero_debounce_extension_takes_the_top_row() {
    let mut h = Harness::new(vec![plain("apps"), calculator()]);
    let apps = h.up(0);
    let calc = h.up(1);
    h.add(&apps, vec![app("2048 game"), app("2 player chess")]);
    let ui = h.front_end();

    h.query(&ui, "2");
    let (id, ..) = live_queries(&calc)[0].clone();
    h.send(
        &calc,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("= 2", 0.1, true)],
            done: true,
        },
    );
    assert_eq!(titles(&ui.results().unwrap())[0], "= 2");
}

#[test]
fn an_answer_from_a_debounced_extension_is_ranked_like_any_late_result() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(&apps, vec![app("g rust")]);
    let ui = h.front_end();

    h.query(&ui, "g rust");
    h.advance(Duration::from_millis(100));
    let (id, ..) = live_queries(&web)[0].clone();
    h.send(
        &web,
        FromExtension::LiveResults {
            live_query: id,
            results: vec![live("claims to answer", 1.0, true)],
            done: true,
        },
    );
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust", "claims to answer"]
    );
}

#[test]
fn an_item_added_while_typing_is_found_by_the_next_keystroke() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    let ui = h.front_end();

    assert_eq!(titles(&h.query(&ui, "fir")), ["Firefox"]);
    h.add(&apps, vec![app("Fire Alarm")]);
    ui.drain();
    assert_eq!(titles(&h.query(&ui, "fire")), ["Firefox", "Fire Alarm"]);
}
