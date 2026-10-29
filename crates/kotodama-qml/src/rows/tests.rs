use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::OfferedAction;

use super::*;

fn row(title: &str, highlights: &[u32]) -> Row {
    Row {
        result: ResultRef {
            extension: "apps".into(),
            id: "x".into(),
        },
        own: Vec::new(),
        contributed: Arc::new([]),
        title: title.into(),
        subtitle: String::new(),
        icon: String::new(),
        highlights: highlights.to_vec(),
        alias: None,
        unavailable: None,
        enters: false,
    }
}

#[test]
fn the_hints_name_the_primary_action_and_the_keys_the_row_takes() {
    let mut entry = row("Emoji", &[]);
    entry.own = vec![Action::new("open", "Open")];
    entry.enters = true;
    let mut project = row("kotodama", &[]);
    project.own = vec![Action::new("open-editor", "Open in Editor")];

    assert_eq!(entry.hints(), "↵ Open · Tab Enter app · ⌃K Actions");
    assert_eq!(project.hints(), "↵ Open in Editor · ⌃K Actions");
}

#[test]
fn a_row_that_runs_nothing_hints_nothing() {
    let mut unavailable = row("Firefox", &[]);
    unavailable.own = vec![Action::new("open", "Open")];
    unavailable.unavailable = Some("apps isn't running".into());

    assert_eq!(unavailable.hints(), "");
    assert_eq!(row("Nothing to do", &[]).hints(), "");
}

#[test]
fn an_action_that_asks_first_names_itself_and_the_keys_that_answer() {
    let mut clipboard = row("Clipboard History", &[]);
    clipboard.own = vec![
        Action::new("open", "Show Clipboard History"),
        Action::new("clear", "Clear Clipboard History").confirm(),
    ];
    clipboard.contributed = Arc::new([OfferedAction {
        extension: "notes".into(),
        action: Action::new("forget", "Forget").confirm(),
    }]);

    assert_eq!(clipboard.confirmation(0), None);
    assert_eq!(
        clipboard.confirmation(1).as_deref(),
        Some("Clear Clipboard History? ↵ Confirm · Esc Cancel")
    );
    assert_eq!(
        clipboard.confirmation(2).as_deref(),
        Some("Forget? ↵ Confirm · Esc Cancel")
    );
    assert_eq!(clipboard.confirmation(3), None);
}

fn offered(extension: &str, id: &str, title: &str) -> OfferedAction {
    OfferedAction {
        extension: extension.into(),
        action: Action::new(id, title),
    }
}

#[test]
fn the_action_panel_lists_the_items_own_actions_then_contributed_ones_under_their_extension() {
    let shown = ShownResult {
        extension: "projects".into(),
        item: Item {
            id: "kotodama".into(),
            kind: ItemKind::Project {
                path: "/src/kotodama".into(),
            },
            title: "kotodama".into(),
            subtitle: None,
            icon: None,
            keyword: None,
            keywords: Vec::new(),
            hidden: false,
            app_only: false,
            weak_keywords: Vec::new(),
            identity: None,
            actions: vec![Action::new("open", "Open"), Action::new("term", "Open in terminal")],
        },
        contributed: vec![
            offered("pinner", "pin", "Pin a version here"),
            offered("pinner", "unpin", "Unpin"),
            offered("git", "log", "Show log"),
        ],
        highlights: Vec::new(),
        alias: None,
        unavailable: None,
    };
    let row = Row::from(shown);
    let panel: Vec<_> = row
        .actions()
        .enumerate()
        .map(|(n, entry)| {
            let runs = row.action(n).unwrap();
            let runs = format!("{}/{}", runs.extension, runs.id);
            (entry.group, entry.action.title.as_str(), runs)
        })
        .collect();
    assert_eq!(
        panel,
        [
            (None, "Open", "projects/open".to_owned()),
            (None, "Open in terminal", "projects/term".to_owned()),
            (Some("pinner"), "Pin a version here", "pinner/pin".to_owned()),
            (Some("pinner"), "Unpin", "pinner/unpin".to_owned()),
            (Some("git"), "Show log", "git/log".to_owned()),
        ]
    );
    assert_eq!(row.action(5), None);
}

#[test]
fn the_action_panel_lists_every_destructive_action_last_but_the_primary() {
    let destructive = |id: &str, title: &str| Action::new(id, title).role(ActionRole::Destructive);
    let mut chrome = row("Google Chrome", &[]);
    chrome.own = vec![
        destructive("reset", "Reset profile"),
        destructive("clear", "Clear data"),
        Action::new("open", "Open"),
    ];
    chrome.contributed = Arc::new([
        OfferedAction {
            extension: "packages".into(),
            action: destructive("remove", "Remove"),
        },
        offered("pinner", "pin", "Pin"),
    ]);

    let panel: Vec<_> = chrome
        .actions()
        .enumerate()
        .map(|(n, entry)| {
            let runs = chrome.action(n).unwrap();
            assert_eq!(runs.id, entry.action.id, "the panel runs what it lists");
            (entry.group, entry.action.title.as_str())
        })
        .collect();
    assert_eq!(
        panel,
        [
            (None, "Reset profile"),
            (None, "Open"),
            (Some("pinner"), "Pin"),
            (None, "Clear data"),
            (Some("packages"), "Remove"),
        ]
    );
    assert_eq!(chrome.action(5), None);
    assert_eq!(chrome.confirmation(4), None);
}

#[test]
fn an_unavailable_row_keeps_its_alias_badge_and_runs_nothing() {
    let shown = ShownResult {
        extension: "apps".into(),
        item: Item {
            id: "firefox".into(),
            kind: ItemKind::App {
                desktop_id: "firefox.desktop".into(),
            },
            title: "Firefox".into(),
            subtitle: None,
            icon: None,
            keyword: None,
            keywords: Vec::new(),
            hidden: false,
            app_only: false,
            weak_keywords: Vec::new(),
            identity: None,
            actions: vec![Action::new("open", "Open")],
        },
        contributed: Vec::new(),
        highlights: Vec::new(),
        alias: Some("ff".into()),
        unavailable: Some("apps isn't running".into()),
    };

    let row = Row::from(shown);

    assert_eq!(row.alias.as_deref(), Some("ff"));
    assert_eq!(row.unavailable.as_deref(), Some("apps isn't running"));
    assert_eq!(row.action(0), None);
}

#[test]
fn a_contributed_shortcut_an_items_own_action_binds_is_left_to_the_own_one() {
    let open = Action::new("open", "Open").shortcut("ctrl+o");
    let item = Item {
        id: "https://example.com".into(),
        kind: ItemKind::Url {
            url: "https://example.com".into(),
        },
        title: "Example".into(),
        subtitle: None,
        icon: None,
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        identity: None,
        actions: vec![open],
    };
    let mut copy = offered("links", "copy", "Copy link");
    copy.action.shortcut = Some("ctrl+o".into());
    let mut share = offered("links", "share", "Share");
    share.action.shortcut = Some("ctrl+s".into());
    let row = Row::listed("bookmarks", &item, Arc::from([copy, share]), Vec::new());
    let shortcuts: Vec<_> = row
        .actions()
        .map(|entry| (entry.action.id.as_str(), entry.shortcut))
        .collect();
    assert_eq!(
        shortcuts,
        [("open", Some("ctrl+o")), ("copy", None), ("share", Some("ctrl+s"))]
    );
    assert_eq!(row.result.extension, "bookmarks");
}

#[test]
fn runs_of_matched_characters_share_one_tag() {
    assert_eq!(
        row("Firefox", &[0, 1, 4]).markup("red"),
        "<font color=\"red\">Fi</font>re<font color=\"red\">f</font>ox"
    );
    assert_eq!(row("ab", &[0, 1]).markup("red"), "<font color=\"red\">ab</font>");
    assert_eq!(row("ab", &[]).markup("red"), "ab");
}

#[test]
fn highlights_count_characters_not_bytes() {
    assert_eq!(
        row("Żółw", &[1, 3]).markup("red"),
        "Ż<font color=\"red\">ó</font>ł<font color=\"red\">w</font>"
    );
}

#[test]
fn the_title_and_the_color_are_escaped() {
    assert_eq!(
        row("<b>&", &[0]).markup("\"x"),
        "<font color=\"&quot;x\">&lt;</font>b&gt;&amp;"
    );
}

#[test]
fn highlights_past_the_title_are_ignored() {
    assert_eq!(row("ab", &[1, 7]).markup("red"), "a<font color=\"red\">b</font>");
}

/// What a view is told.
#[derive(Debug, PartialEq)]
enum Told {
    Removed(Range<usize>),
    Inserted(Range<usize>),
    Moved { from: usize, before: usize },
    Changed(Range<usize>),
}

/// A model that records what views are told, and keeps a view's copy of
/// the rows' ids from that alone, to check it against the rows.
struct Recorder {
    rows: Rows,
    told: Vec<Told>,
    seen: Vec<String>,
}

impl Recorder {
    fn new(ids: &[&str]) -> Self {
        Recorder {
            rows: ids.iter().map(|id| listed(id, &[])).collect::<Vec<_>>().into(),
            told: Vec::new(),
            seen: ids.iter().map(ToString::to_string).collect(),
        }
    }

    fn ids(&self) -> Vec<&str> {
        self.rows.iter().map(|row| row.result.id.as_str()).collect()
    }

    fn check(&self) {
        assert_eq!(self.seen, self.ids(), "the view's rows are the model's");
    }
}

impl Model for Recorder {
    fn rows(&mut self) -> &mut Rows {
        &mut self.rows
    }

    fn begin_remove(&mut self, rows: Range<usize>) {
        self.check();
        self.told.push(Told::Removed(rows));
    }

    fn end_remove(&mut self) {
        let Some(Told::Removed(rows)) = self.told.last() else {
            panic!("no begin_remove");
        };
        self.seen.drain(rows.clone());
        self.check();
    }

    fn begin_insert(&mut self, rows: Range<usize>) {
        self.check();
        self.told.push(Told::Inserted(rows));
    }

    fn end_insert(&mut self) {
        let Some(Told::Inserted(rows)) = self.told.last() else {
            panic!("no begin_insert");
        };
        let ids: Vec<String> = rows
            .clone()
            .map(|at| self.rows.get(at).unwrap().result.id.clone())
            .collect();
        self.seen.splice(rows.start..rows.start, ids);
        self.check();
    }

    fn begin_move(&mut self, from: usize, before: usize) {
        self.check();
        assert!(before != from && before != from + 1, "Qt refuses it");
        self.told.push(Told::Moved { from, before });
    }

    fn end_move(&mut self) {
        let Some(&Told::Moved { from, before }) = self.told.last() else {
            panic!("no begin_move");
        };
        let id = self.seen.remove(from);
        let to = if from < before { before - 1 } else { before };
        self.seen.insert(to, id);
        self.check();
    }

    fn changed(&mut self, rows: Range<usize>) {
        // A view reads the rows changed in place again.
        for at in rows.clone() {
            self.seen[at].clone_from(&self.rows.get(at).unwrap().result.id);
        }
        self.check();
        self.told.push(Told::Changed(rows));
    }
}

fn listed(id: &str, highlights: &[u32]) -> Row {
    let mut row = row(id, highlights);
    row.result = ResultRef {
        extension: "clipboard".into(),
        id: id.into(),
    };
    row
}

/// A list View's new rows by id, each with its highlights, counting the
/// rows it makes.
struct New {
    rows: Vec<(String, Vec<u32>)>,
    made: std::cell::Cell<usize>,
}

fn new(ids: &[&str]) -> New {
    New {
        rows: ids.iter().map(|id| (id.to_string(), Vec::new())).collect(),
        made: std::cell::Cell::default(),
    }
}

impl Fresh for New {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn key(&self, n: usize) -> (&str, &str) {
        ("clipboard", &self.rows[n].0)
    }

    fn row(&self, n: usize) -> Row {
        self.made.set(self.made.get() + 1);
        let (id, highlights) = &self.rows[n];
        listed(id, highlights)
    }

    fn refresh(&self, n: usize, old: &mut Row) -> bool {
        let highlights = &self.rows[n].1;
        let changed = old.highlights != *highlights;
        old.highlights.clone_from(highlights);
        changed
    }
}

/// The model with `old` rows after a diff to `to`, which it must show.
fn diffed(old: &[&str], to: &New) -> Recorder {
    let mut model = Recorder::new(old);
    diff(&mut model, to);
    let ids: Vec<&str> = to.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(model.ids(), ids);
    model
}

#[test]
fn a_filter_that_narrows_removes_the_rows_that_no_longer_match_and_leaves_the_rest() {
    let to = new(&["b", "d", "e"]);
    let model = diffed(&["a", "b", "c", "d", "e", "f"], &to);
    assert_eq!(
        model.told,
        [Told::Removed(5..6), Told::Removed(2..3), Told::Removed(0..1)]
    );
    assert_eq!(to.made.get(), 0, "no row is made again");
}

#[test]
fn a_filter_that_widens_inserts_the_rows_that_match_again_in_runs() {
    let to = new(&["a", "b", "c", "d", "e", "f"]);
    let model = diffed(&["b", "e"], &to);
    assert_eq!(
        model.told,
        [Told::Inserted(0..1), Told::Inserted(2..4), Told::Inserted(5..6)]
    );
    assert_eq!(to.made.get(), 4, "only the new rows are made");
}

#[test]
fn a_filter_that_reorders_moves_the_fewest_rows() {
    let model = diffed(&["a", "b", "c", "d"], &new(&["d", "a", "b", "c"]));
    assert_eq!(model.told, [Told::Moved { from: 3, before: 0 }]);

    let model = diffed(&["a", "b", "c", "d"], &new(&["b", "c", "d", "a"]));
    assert_eq!(model.told, [Told::Moved { from: 0, before: 4 }]);

    let model = diffed(&["a", "b", "c", "d", "e"], &new(&["e", "c", "a", "b", "d"]));
    assert_eq!(model.told.len(), 2, "a, b and d stay: {:?}", model.told);
}

#[test]
fn a_filter_that_removes_inserts_and_reorders_at_once_ends_with_the_new_rows() {
    let model = diffed(&["a", "b", "c", "d", "e"], &new(&["x", "d", "b", "y", "a"]));
    assert_eq!(
        model.told,
        [
            Told::Removed(4..5),
            Told::Removed(2..3),
            Told::Moved { from: 2, before: 0 },
            Told::Moved { from: 2, before: 1 },
            Told::Inserted(0..1),
            Told::Inserted(3..4),
        ]
    );
}

#[test]
fn only_the_rows_whose_highlights_moved_change_in_place() {
    let mut to = new(&["a", "b", "c", "d", "e"]);
    to.rows[1].1 = vec![0];
    to.rows[2].1 = vec![0];
    to.rows[4].1 = vec![1];
    let model = diffed(&["a", "b", "c", "d", "e"], &to);
    assert_eq!(model.told, [Told::Changed(1..3), Told::Changed(4..5)]);
    assert_eq!(model.rows.get(4).unwrap().highlights, [1]);
}

#[test]
fn rows_far_out_of_order_are_removed_and_inserted_rather_than_moved_one_by_one() {
    let old: Vec<String> = (0..200).map(|n| n.to_string()).collect();
    let old: Vec<&str> = old.iter().map(String::as_str).collect();
    let reversed: Vec<&str> = old.iter().rev().copied().collect();
    let model = diffed(&old, &new(&reversed));
    assert!(
        model.told.iter().all(|told| !matches!(told, Told::Moved { .. })),
        "{:?}",
        model.told
    );
}

#[test]
fn any_new_order_is_reached_and_every_view_keeps_up() {
    // Lists and shuffles from a fixed linear congruential sequence.
    let mut seed = 7_u64;
    let mut next = |below: usize| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) as usize % below
    };
    let ids: Vec<String> = (0..40).map(|n| n.to_string()).collect();
    for _ in 0..200 {
        let mut old: Vec<&str> = Vec::new();
        let mut fresh: Vec<&str> = Vec::new();
        for id in &ids {
            if next(3) > 0 {
                old.push(id);
            }
            if next(3) > 0 {
                fresh.push(id);
            }
        }
        for n in (1..old.len()).rev() {
            old.swap(n, next(n + 1));
        }
        for _ in 0..next(6) {
            let (a, b) = (next(fresh.len()), next(fresh.len()));
            fresh.swap(a, b);
        }
        diffed(&old, &new(&fresh));
    }
}

#[test]
fn rows_with_the_same_key_are_kept_once() {
    let model = diffed(&["a", "a", "b"], &new(&["b", "a", "a"]));
    assert_eq!(
        model.told,
        [
            Told::Removed(1..2),
            Told::Moved { from: 1, before: 0 },
            Told::Inserted(2..3)
        ]
    );
}

#[test]
fn the_results_change_in_place_by_position_and_the_rest_are_inserted_or_removed() {
    let mut model = Recorder::new(&["a", "b", "c"]);
    let rows = ["c", "b", "a", "d", "e"].map(|id| listed(id, &[]));
    replace(&mut model, rows.to_vec());
    assert_eq!(model.told, [Told::Changed(0..3), Told::Inserted(3..5)]);
    assert_eq!(model.ids(), ["c", "b", "a", "d", "e"]);

    let mut model = Recorder::new(&["a", "b", "c", "d", "e"]);
    replace(&mut model, vec![listed("x", &[]), listed("y", &[])]);
    assert_eq!(model.told, [Told::Removed(2..5), Told::Changed(0..2)]);
    assert_eq!(model.ids(), ["x", "y"]);

    let mut model = Recorder::new(&[]);
    replace(&mut model, Vec::new());
    assert_eq!(model.told, []);
}

#[test]
fn a_listed_row_is_made_again_only_when_its_item_changed() {
    let mut item = Item {
        id: "a".into(),
        kind: ItemKind::Url {
            url: "https://a".into(),
        },
        title: "Alpha".into(),
        subtitle: None,
        icon: None,
        keyword: None,
        keywords: Vec::new(),
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
        identity: None,
        actions: vec![Action::new("open", "Open")],
    };
    let none: Arc<[OfferedAction]> = Arc::new([]);
    let mut row = Row::listed("bookmarks", &item, Arc::clone(&none), vec![0]);
    assert!(!row.relist("bookmarks", &item, &none, &[0]));
    assert!(row.relist("bookmarks", &item, &none, &[1, 2]));
    assert_eq!(row.highlights, [1, 2]);
    item.title = "Alps".into();
    assert!(row.relist("bookmarks", &item, &none, &[1, 2]));
    assert_eq!(row.title, "Alps");
}
