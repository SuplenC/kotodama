//! The Daemon's state machine, driven through its events: Extensions and Front ends are
//! channels here, and starting a process or typing is recorded instead of done.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use kotodama_protocol::*;
use kotodamad::daemon::{
    Daemon, Desktop, Event, ExtensionId, FrontEndId, HANDSHAKE_TIMEOUT, Launcher, Output,
};
use kotodamad::discovery::Found;

#[derive(Clone, Default)]
struct Launches(
    Rc<RefCell<Vec<(ExtensionId, u64)>>>,
    Rc<RefCell<Vec<(ExtensionId, u64)>>>,
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
const OUTBOX: usize = 64;

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
        self.up_with_room(extension, OUTBOX)
    }

    /// Like `up`, with room for only `room` messages the Extension hasn't read.
    fn up_with_room(&mut self, extension: usize, room: usize) -> Ext {
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

    fn front_end_sends(&mut self, front_end: &FrontEnd, message: FromFrontEnd) {
        self.event(Event::FromFrontEnd {
            front_end: front_end.id,
            message,
        });
    }

    fn query(&mut self, front_end: &FrontEnd, text: &str) -> Vec<ShownResult> {
        self.front_end_sends(front_end, FromFrontEnd::Query { text: text.into() });
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
    fn results(&self) -> Option<Vec<ShownResult>> {
        self.drain()
            .into_iter()
            .rev()
            .find_map(|message| match message {
                ToFrontEnd::Results { results } => Some(results),
                _ => None,
            })
    }
}

fn titles(rows: &[ShownResult]) -> Vec<&str> {
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
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 2 });

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
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });

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

fn pinner() -> Found {
    manifest(
        "name = \"pinner\"\ncommand = [\"x\"]\naction_catalog = true\n\
         [[contributed_actions]]\nkind = \"project\"\nid = \"pin\"\ntitle = \"Pin a version here\"\nshortcut = \"ctrl+p\"",
    )
}

fn projects() -> Found {
    manifest(
        "name = \"projects\"\ncommand = [\"x\"]\n\
         [[contributed_actions]]\nkind = \"project\"\nid = \"own-contribution\"\ntitle = \"Never shown on own Items\"",
    )
}

fn runs(ext: &Ext) -> Vec<(u64, String, ItemRef)> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::RunAction {
                invocation,
                action,
                item,
                ..
            } => Some((invocation, action, item)),
            _ => None,
        })
        .collect()
}

fn run(h: &mut Harness, ui: &FrontEnd, owner: &str, id: &str, by: &str, action: &str) {
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

#[test]
fn contributed_actions_follow_the_owners_actions_and_are_never_primary() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();

    let row = &h.query(&ui, "kotodama")[0];
    assert_eq!(
        row.item.actions[0].id, "open",
        "the owner's Action stays primary"
    );
    let contributed: Vec<_> = row
        .contributed
        .iter()
        .map(|c| (c.extension.as_str(), c.action.id.as_str()))
        .collect();
    assert_eq!(contributed, [("pinner", "pin")]);
}

#[test]
fn when_shortcuts_clash_the_owners_binding_wins() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    h.up(0);
    let projects = h.up(1);
    let mut item = project("kotodama");
    item.actions[0].shortcut = Some("ctrl+p".into());
    h.add(&projects, vec![item]);
    let ui = h.front_end();

    let row = &h.query(&ui, "kotodama")[0];
    assert_eq!(row.item.actions[0].shortcut.as_deref(), Some("ctrl+p"));
    assert_eq!(row.contributed[0].action.shortcut, None);
}

#[test]
fn a_contributed_action_reaches_the_contributing_extension_with_the_item() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    let pinner = h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama")]);
    let ui = h.front_end();
    h.query(&ui, "kotodama");
    pinner.drain();
    projects.drain();

    run(&mut h, &ui, "projects", "kotodama", "pinner", "pin");
    let sent = runs(&pinner);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "pin");
    assert_eq!(sent[0].2, project("kotodama").to_ref());
    assert_eq!(runs(&projects), []);

    // The contributor's answer closes the menu that ran it.
    h.send(
        &pinner,
        FromExtension::ActionDone {
            invocation: sent[0].0,
            outcome: Outcome::Close,
        },
    );
    assert!(ui.drain().contains(&ToFrontEnd::Close));
}

#[test]
fn an_action_the_result_does_not_offer_is_refused() {
    let mut h = Harness::new(vec![pinner(), projects()]);
    let pinner = h.up(0);
    let projects = h.up(1);
    h.add(&projects, vec![project("kotodama"), app("Firefox")]);
    let ui = h.front_end();
    h.query(&ui, "f");
    pinner.drain();

    // `pin` is for projects, not apps.
    run(&mut h, &ui, "projects", "firefox", "pinner", "pin");
    assert_eq!(runs(&pinner), []);
    assert!(
        ui.drain()
            .iter()
            .any(|m| matches!(m, ToFrontEnd::ActionFailed { .. }))
    );
}

fn catalogs(ext: &Ext) -> Vec<Vec<CatalogEntry>> {
    ext.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToExtension::ActionCatalog { entries } => Some(entries),
            _ => None,
        })
        .collect()
}

#[test]
fn the_action_catalog_lists_actions_by_kind_without_items_and_follows_extensions() {
    let mut h = Harness::new(vec![plain("apps"), pinner(), projects()]);
    let apps = h.up(0);
    let pinner = h.up(1);
    h.add(&apps, vec![app("Firefox"), app("Files")]);
    h.send(&pinner, FromExtension::RequestActionCatalog);

    let summary = |entries: &[CatalogEntry]| {
        entries
            .iter()
            .map(|e| format!("{}:{}:{}", e.extension, e.kind, e.action.id))
            .collect::<Vec<_>>()
    };
    let first = catalogs(&pinner);
    assert_eq!(first.len(), 1);
    assert_eq!(summary(&first[0]), ["apps:app:open", "pinner:project:pin"]);

    let projects = h.up(2);
    let updated = catalogs(&pinner);
    assert_eq!(
        summary(updated.last().unwrap()),
        [
            "apps:app:open",
            "pinner:project:pin",
            "projects:project:own-contribution"
        ]
    );

    h.down(&projects);
    assert_eq!(
        summary(catalogs(&pinner).last().unwrap()),
        ["apps:app:open", "pinner:project:pin"]
    );
}

#[test]
fn an_extension_whose_manifest_does_not_ask_never_receives_the_catalog() {
    let mut h = Harness::new(vec![plain("apps"), pinner()]);
    let apps = h.up(0);
    let pinner = h.up(1);
    h.send(&apps, FromExtension::RequestActionCatalog);
    h.send(&pinner, FromExtension::RequestActionCatalog);
    h.add(&apps, vec![app("Firefox")]);

    assert_eq!(catalogs(&apps), Vec::<Vec<CatalogEntry>>::new());
    assert!(!catalogs(&pinner).is_empty());
}

fn statuses(ui: &FrontEnd) -> Vec<Vec<String>> {
    ui.drain()
        .into_iter()
        .filter_map(|message| match message {
            ToFrontEnd::Status { down } => Some(down),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stopped_extensions_results_disappear_and_a_marker_shows_it_is_down() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Firefox")]);
    let ui = h.front_end();
    h.query(&ui, "fire");

    h.down(&apps);
    let messages = ui.drain();
    assert!(messages.contains(&ToFrontEnd::Results { results: vec![] }));
    assert!(messages.contains(&ToFrontEnd::Status {
        down: vec!["apps".into()]
    }));
}

#[test]
fn a_stopped_extension_is_restarted_with_growing_backoff_and_marked_up_again() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();
    let launches = |h: &Harness| h.launches.0.borrow().len();

    let first = h.up(0);
    h.down(&first);
    h.advance(Duration::from_millis(99));
    assert_eq!(launches(&h), 1);
    h.advance(Duration::from_millis(1));
    assert_eq!(launches(&h), 2, "restarted after 100 ms");

    let second = h.up(0);
    h.down(&second);
    h.advance(Duration::from_millis(199));
    assert_eq!(launches(&h), 2);
    h.advance(Duration::from_millis(1));
    assert_eq!(launches(&h), 3, "then after 200 ms");

    statuses(&ui);
    h.up(0);
    // Back up: the marker goes once the handshake is done.
    let ui2 = h.front_end();
    h.query(&ui2, "x");
    assert_eq!(statuses(&ui), [Vec::<String>::new()]);
}

#[test]
fn messages_from_a_previous_run_of_an_extension_are_ignored() {
    let mut h = Harness::new(vec![plain("apps")]);
    let old = h.up(0);
    h.down(&old);
    h.advance(Duration::from_secs(1));
    let _new = h.up(0);
    let ui = h.front_end();

    h.add(&old, vec![app("Ghost")]);
    assert_eq!(h.query(&ui, "ghost"), []);
}

#[test]
fn a_config_change_reaches_only_the_extensions_whose_table_changed() {
    let table = |toml: &str| -> Table {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.toml"), toml).unwrap();
        kotodamad::config::load(&[dir.path().to_owned()]).unwrap()
    };
    let mut h = Harness::with_config(
        vec![plain("apps"), plain("web")],
        table("[extensions.apps]\nterminal = \"foot\""),
    );
    let apps = h.up(0);
    let web = h.up(1);
    let configs = |ext: &Ext| -> Vec<Table> {
        ext.drain()
            .into_iter()
            .filter_map(|m| match m {
                ToExtension::Config(table) => Some(table),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        configs(&apps),
        [BTreeMap::from([(
            "terminal".into(),
            Value::String("foot".into())
        )])],
        "the first message after the handshake"
    );
    assert_eq!(configs(&web), [Table::new()]);

    h.event(Event::Config(table(
        "[extensions.apps]\nterminal = \"kitty\"\n[other]\nx = 1",
    )));
    assert_eq!(
        configs(&apps),
        [BTreeMap::from([(
            "terminal".into(),
            Value::String("kitty".into())
        )])]
    );
    assert_eq!(configs(&web), []);
}

fn typer() -> Found {
    plain("typer")
}

fn type_text(h: &mut Harness, ui: &FrontEnd, typer: &Ext, text: &str) {
    h.add(typer, vec![app("Snippet")]);
    h.query(ui, "snippet");
    typer.drain();
    run(h, ui, "typer", "snippet", "typer", "open");
    let (invocation, ..) = runs(typer)[0].clone();
    h.send(
        typer,
        FromExtension::ActionDone {
            invocation,
            outcome: Outcome::Type { text: text.into() },
        },
    );
}

#[test]
fn typing_waits_until_every_front_end_has_closed() {
    let mut h = Harness::new(vec![typer()]);
    let typer = h.up(0);
    let ui = h.front_end();
    let other = h.front_end();

    type_text(&mut h, &ui, &typer, "hello");
    assert!(ui.drain().contains(&ToFrontEnd::Close));
    assert!(other.drain().contains(&ToFrontEnd::Close));
    assert_eq!(*h.outputs.0.borrow(), []);

    h.front_end_sends(&ui, FromFrontEnd::Closed);
    assert_eq!(*h.outputs.0.borrow(), []);
    h.event(Event::FrontEndDown {
        front_end: other.id,
    });
    assert_eq!(*h.outputs.0.borrow(), [Output::Type("hello".into())]);
}

#[test]
fn typing_goes_ahead_when_a_front_end_never_answers() {
    let mut h = Harness::new(vec![typer()]);
    let typer = h.up(0);
    let ui = h.front_end();

    type_text(&mut h, &ui, &typer, "hello");
    h.advance(Duration::from_millis(499));
    assert_eq!(*h.outputs.0.borrow(), []);
    h.advance(Duration::from_millis(1));
    assert_eq!(*h.outputs.0.borrow(), [Output::Type("hello".into())]);
}

#[test]
fn a_new_item_taking_a_removed_visible_items_place_in_the_index_is_still_late() {
    let mut h = Harness::new(vec![plain("apps")]);
    let apps = h.up(0);
    h.add(&apps, vec![app("Fire one"), app("Fire two")]);
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(titles(&h.query(&ui, "fire"))[0], "Fire one");

    h.send(
        &apps,
        FromExtension::RemoveItems {
            ids: vec!["fire-one".into()],
        },
    );
    h.add(&apps, vec![app("Fire newcomer")]);
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["Fire two", "Fire newcomer"]
    );
}

#[test]
fn a_bigger_viewport_protects_the_rows_it_shows_at_once() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(&apps, vec![app("g rust a"), app("g rust bb")]);
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(titles(&h.query(&ui, "g rust")), ["g rust a", "g rust bb"]);

    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 2 });
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
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust a", "g rust bb", "perfect"]
    );
}

#[test]
fn a_stopped_extensions_jobs_are_not_shown_to_new_front_ends() {
    let mut h = Harness::new(vec![plain("installer")]);
    let installer = h.up(0);
    h.send(
        &installer,
        FromExtension::JobProgress(Job {
            id: "install".into(),
            title: "Installing".into(),
            state: JobState::Running,
            progress: None,
            message: None,
        }),
    );
    h.down(&installer);

    let (tx, rx) = mpsc::channel();
    h.event(Event::FrontEndUp {
        front_end: FrontEndId(99),
        sender: tx,
    });
    let jobs: Vec<_> = rx
        .try_iter()
        .filter(|m| matches!(m, ToFrontEnd::JobProgress { .. }))
        .collect();
    assert_eq!(jobs, []);
}

fn stops(h: &Harness) -> Vec<(ExtensionId, u64)> {
    h.launches.1.borrow().clone()
}

#[test]
fn an_extension_that_never_finishes_its_handshake_is_stopped_and_restarted() {
    let mut h = Harness::new(vec![plain("apps")]);
    let ui = h.front_end();

    h.advance(HANDSHAKE_TIMEOUT - Duration::from_millis(1));
    assert_eq!(stops(&h), []);
    h.advance(Duration::from_millis(1));
    assert_eq!(stops(&h), [(ExtensionId(0), 1)]);

    // Stopping it ends the process, which reads as any other stop: a marker,
    // then a restart.
    h.event(Event::ExtensionDown {
        extension: ExtensionId(0),
        generation: 1,
    });
    assert!(ui.drain().contains(&ToFrontEnd::Status {
        down: vec!["apps".into()]
    }));
    h.advance(Duration::from_millis(100));
    assert_eq!(h.launches.0.borrow().len(), 2);
}

#[test]
fn an_extension_that_stops_reading_is_stopped_instead_of_queueing_without_end() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up_with_room(0, 4);
    let ui = h.front_end();

    // Config plus a Live query and a cancel per keystroke: it never reads them.
    for n in 0..3 {
        h.query(&ui, &n.to_string());
    }
    assert_eq!(stops(&h), [(ExtensionId(0), 1)]);
    drop(calc);
}

#[test]
fn a_restarted_extension_gets_the_live_query_already_typed() {
    let mut h = Harness::new(vec![calculator()]);
    let calc = h.up(0);
    let ui = h.front_end();
    h.query(&ui, "2+2");
    h.down(&calc);
    h.advance(Duration::from_millis(100));

    let calc = h.up(0);
    let sent = live_queries(&calc);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "2+2");
}

#[test]
fn rows_scrolled_past_are_protected_like_the_ones_on_screen() {
    let mut h = Harness::new(vec![plain("apps"), web()]);
    let apps = h.up(0);
    let web = h.up(1);
    h.add(
        &apps,
        vec![app("g rust a"), app("g rust bb"), app("g rust ccc")],
    );
    let ui = h.front_end();
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 0, rows: 1 });
    assert_eq!(
        titles(&h.query(&ui, "g rust")),
        ["g rust a", "g rust bb", "g rust ccc"]
    );

    // Scrolled so the second row is the one on screen.
    h.front_end_sends(&ui, FromFrontEnd::Viewport { top: 1, rows: 1 });
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
    assert_eq!(
        titles(&ui.results().unwrap()),
        ["g rust a", "g rust bb", "perfect", "g rust ccc"]
    );
}
