//! The Results as the list model holds them, and how a new list replaces the
//! old one without resetting the model. Nothing here touches Qt, so it is
//! tested without it.

use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;

use kotodama_protocol::{
    APP_ENTRY_KIND, Action, ActionRef, Item, ItemKind, OfferedAction, ResultRef, ShownResult,
};

/// One row of the list model.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub result: ResultRef,
    /// The Item's own Actions, the primary one (what Enter runs) first.
    own: Vec<Action>,
    /// Other Extensions' Actions for the Item, grouped by Extension. The
    /// Items of a list View share those of their kind.
    contributed: Arc<[OfferedAction]>,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    /// Positions, in characters, of the title's matched characters, in order.
    pub highlights: Vec<u32>,
    /// The Alias the query named the Result by, shown as a badge.
    pub alias: Option<Box<str>>,
    /// Why the Result can't run, shown greyed out in place of the subtitle.
    pub unavailable: Option<Box<str>>,
    /// Whether it is an app's entry Item, which Tab enters.
    pub enters: bool,
}

/// One entry of the Action panel.
#[derive(Debug, PartialEq)]
pub struct PanelEntry<'a> {
    pub action: &'a Action,
    /// The Extension that offers a Contributed action; `None` for the Item's
    /// own Actions.
    pub group: Option<&'a str>,
    /// What runs it from the keyboard: the Action's shortcut, unless it is a
    /// Contributed action's and one of the Item's own Actions binds it too.
    pub shortcut: Option<&'a str>,
}

impl From<ShownResult> for Row {
    fn from(shown: ShownResult) -> Self {
        let item = shown.item;
        let mut highlights = shown.highlights;
        highlights.sort_unstable();
        highlights.dedup();
        let enters =
            matches!(&item.kind, ItemKind::Namespaced(kind) if kind.as_str() == APP_ENTRY_KIND);
        Row {
            result: ResultRef {
                extension: shown.extension,
                id: item.id,
            },
            own: item.actions,
            contributed: shown.contributed.into(),
            title: item.title,
            subtitle: item.subtitle.unwrap_or_default(),
            icon: item.icon.unwrap_or_default(),
            highlights,
            alias: shown.alias.map(String::into_boxed_str),
            unavailable: shown.unavailable.map(String::into_boxed_str),
            enters,
        }
    }
}

impl Row {
    /// A list View's Item, with the Actions contributed to its kind and the
    /// filter's `highlights`, in order.
    pub fn listed(
        extension: &str,
        item: &Item,
        contributed: Arc<[OfferedAction]>,
        highlights: Vec<u32>,
    ) -> Self {
        Row {
            result: ResultRef {
                extension: extension.to_owned(),
                id: item.id.clone(),
            },
            own: item.actions.clone(),
            contributed,
            title: item.title.clone(),
            subtitle: item.subtitle.clone().unwrap_or_default(),
            icon: item.icon.clone().unwrap_or_default(),
            highlights,
            alias: None,
            unavailable: None,
            enters: false,
        }
    }

    /// Brings a row [`Row::listed`] made for the same Item up to date: whether
    /// it changed. Nothing is copied unless something did, so a filter that
    /// only moves the highlights costs no more than them.
    pub fn relist(
        &mut self,
        extension: &str,
        item: &Item,
        contributed: &Arc<[OfferedAction]>,
        highlights: &[u32],
    ) -> bool {
        let same = self.title == item.title
            && self.subtitle == item.subtitle.as_deref().unwrap_or_default()
            && self.icon == item.icon.as_deref().unwrap_or_default()
            && self.own == item.actions
            && (Arc::ptr_eq(&self.contributed, contributed) || self.contributed == *contributed);
        if !same {
            *self = Row::listed(extension, item, contributed.clone(), highlights.to_vec());
            return true;
        }
        if self.highlights == highlights {
            return false;
        }
        self.highlights.clear();
        self.highlights.extend_from_slice(highlights);
        true
    }

    /// The Action panel: the Item's own Actions, then the Contributed ones.
    /// Built when asked for, since most rows are never opened.
    pub fn actions(&self) -> impl Iterator<Item = PanelEntry<'_>> {
        let own = self.own.iter().map(|action| PanelEntry {
            action,
            group: None,
            shortcut: action.shortcut.as_deref(),
        });
        let contributed = self.contributed.iter().map(|offered| PanelEntry {
            action: &offered.action,
            group: Some(offered.extension.as_str()),
            // The Daemon settles clashes among contributors, and with a
            // Result's own Actions, but a View's Items share theirs by kind.
            shortcut: offered.action.shortcut.as_deref().filter(|shortcut| {
                !self
                    .own
                    .iter()
                    .any(|own| own.shortcut.as_deref() == Some(*shortcut))
            }),
        });
        own.chain(contributed)
    }

    /// The keys the row takes, for a footer that teaches them: what Enter
    /// runs, Tab on an entry Item, and Ctrl+K. Empty when nothing runs.
    pub fn hints(&self) -> String {
        let Some(primary) = self.own.first().filter(|_| self.unavailable.is_none()) else {
            return String::new();
        };
        let mut hints = format!("↵ {}", primary.title);
        if self.enters {
            hints.push_str(" · Tab Enter app");
        }
        hints.push_str(" · ⌃K Actions");
        hints
    }

    /// What the footer asks before the `n`th entry of the Action panel
    /// runs, such as `Power Off? ↵ Confirm · Esc Cancel`; `None` for an
    /// Action that runs at once.
    pub fn confirmation(&self, n: usize) -> Option<String> {
        let entry = self.actions().nth(n)?;
        entry
            .action
            .confirm
            .then(|| format!("{}? ↵ Confirm · Esc Cancel", entry.action.title))
    }

    /// What runs the `n`th entry of the Action panel. Nothing runs on an
    /// unavailable row.
    pub fn action(&self, n: usize) -> Option<ActionRef> {
        if self.unavailable.is_some() {
            return None;
        }
        match self.own.get(n) {
            Some(own) => Some(ActionRef {
                extension: self.result.extension.clone(),
                id: own.id.clone(),
            }),
            None => self
                .contributed
                .get(n - self.own.len())
                .map(|offered| ActionRef {
                    extension: offered.extension.clone(),
                    id: offered.action.id.clone(),
                }),
        }
    }

    /// The title as Qt's StyledText, with each run of matched characters set
    /// in `color` (any color QML accepts, such as `#89b4fa`).
    pub fn markup(&self, color: &str) -> String {
        let mut out = String::with_capacity(self.title.len() + 32 * self.highlights.len());
        let mut wanted = self.highlights.iter().copied().peekable();
        let mut open = false;
        for (n, ch) in self.title.chars().enumerate() {
            let matched = wanted.next_if_eq(&(n as u32)).is_some();
            if matched != open {
                if matched {
                    let _ = write!(out, "<font color=\"{}\">", Escaped(color));
                } else {
                    out.push_str("</font>");
                }
                open = matched;
            }
            let _ = write!(out, "{}", Escaped(ch.encode_utf8(&mut [0; 4])));
        }
        if open {
            out.push_str("</font>");
        }
        out
    }
}

/// Text made safe to put in StyledText.
struct Escaped<'a>(&'a str);

impl std::fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                _ => f.write_char(ch)?,
            }
        }
        Ok(())
    }
}

/// A list model's rows, with a gap where they last changed. A change moves
/// the gap to it, which costs the rows between the two, so changes made in
/// order through the list, as [`diff`] makes them, cost each row once
/// however many there are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rows {
    /// The rows before the gap.
    head: Vec<Row>,
    /// The rows after the gap, last first.
    tail: Vec<Row>,
}

impl From<Vec<Row>> for Rows {
    fn from(head: Vec<Row>) -> Self {
        Rows {
            head,
            tail: Vec::new(),
        }
    }
}

impl Rows {
    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn get(&self, n: usize) -> Option<&Row> {
        match n.checked_sub(self.head.len()) {
            None => self.head.get(n),
            Some(after) if after < self.tail.len() => Some(&self.tail[self.tail.len() - 1 - after]),
            Some(_) => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.head.iter().chain(self.tail.iter().rev())
    }

    fn gap_at(&mut self, at: usize) {
        let head = self.head.len();
        if at < head {
            self.tail.extend(self.head.drain(at..).rev());
        } else if at > head {
            let from = self.tail.len() - (at - head);
            self.head.extend(self.tail.drain(from..).rev());
        }
    }

    fn remove(&mut self, rows: Range<usize>) {
        self.gap_at(rows.end);
        self.head.truncate(rows.start);
    }

    fn insert(&mut self, at: usize, rows: impl Iterator<Item = Row>) {
        self.gap_at(at);
        self.head.extend(rows);
    }

    /// Moves row `from` to be row `to`.
    fn move_row(&mut self, from: usize, to: usize) {
        self.gap_at(from + 1);
        let row = self.head.pop().expect("a row before the gap");
        self.gap_at(to);
        self.head.push(row);
    }

    /// Puts `rows` in place of as many first rows.
    fn put_front(&mut self, rows: Vec<Row>) {
        self.gap_at(rows.len());
        self.head = rows;
    }
}

/// A list model's rows, and the (Qt) views it tells how they change. Rows are
/// inserted, removed or moved between a `begin_*` and its `end_*`, and rows
/// changed in place before `changed` names them, so a view that reads them
/// on any signal finds them as it was told.
pub trait Model {
    fn rows(&mut self) -> &mut Rows;
    fn begin_remove(&mut self, rows: Range<usize>);
    fn end_remove(&mut self);
    fn begin_insert(&mut self, rows: Range<usize>);
    fn end_insert(&mut self);
    /// Row `from` moves to just before row `before`, both as numbered before
    /// the move.
    fn begin_move(&mut self, from: usize, before: usize);
    fn end_move(&mut self);
    /// The rows in `rows` changed in place, in all roles.
    fn changed(&mut self, rows: Range<usize>);
}

fn remove(model: &mut impl Model, rows: Range<usize>) {
    model.begin_remove(rows.clone());
    model.rows().remove(rows);
    model.end_remove();
}

fn insert(model: &mut impl Model, at: usize, rows: impl ExactSizeIterator<Item = Row>) {
    model.begin_insert(at..at + rows.len());
    model.rows().insert(at, rows);
    model.end_insert();
}

/// Puts `new` in place of the model's rows by position: the rows both lists
/// have change in place, so the list keeps its delegates and its current
/// row, and only the difference in length is inserted or removed. For the
/// search's Results, whose top row stays the current one as they change.
pub fn replace(model: &mut impl Model, mut new: Vec<Row>) {
    let old = model.rows().len();
    let kept = old.min(new.len());
    if old > kept {
        remove(model, kept..old);
    }
    let tail = new.split_off(kept);
    model.rows().put_front(new);
    if kept > 0 {
        model.changed(0..kept);
    }
    if !tail.is_empty() {
        insert(model, kept, tail.into_iter());
    }
}

/// A new list of rows for [`diff`], each with a key: the Extension and the
/// id of its Item.
pub trait Fresh {
    fn len(&self) -> usize;
    fn key(&self, n: usize) -> (&str, &str);
    /// The `n`th row, which the old list doesn't have.
    fn row(&self, n: usize) -> Row;
    /// Brings `old`, the row with the `n`th row's key, up to date: whether it
    /// changed.
    fn refresh(&self, n: usize, old: &mut Row) -> bool;
}

/// Above this many rows out of place, [`diff`] removes and inserts them
/// rather than moving each: a move shifts every row between its two places,
/// here and in each view, so many of them cost more than the few delegates
/// on screen that removing and inserting makes again.
const MOST_MOVES: usize = 64;

/// Puts `new` in place of the model's rows by key: a row only the old list
/// has is removed, one only the new list has is inserted, and one both have
/// is kept, with its delegate, moved if it is out of order, and changed only
/// if it differs. The views' current row stays on its Item while it is
/// listed. For a list View, whose Items stay put as its filter changes.
pub fn diff(model: &mut impl Model, new: &impl Fresh) {
    // Each new row's old place, if the old list has its key.
    let mut old_place: Vec<Option<usize>> = {
        let old = model.rows();
        let mut place: HashMap<(&str, &str), usize> = HashMap::with_capacity(old.len());
        for (n, row) in old.iter().enumerate() {
            // The first of rows with the same key is the one kept.
            place.entry(key(row)).or_insert(n);
        }
        (0..new.len()).map(|n| place.remove(&new.key(n))).collect()
    };
    // The kept rows in order already, which stay where they are; the rest
    // move, unless that's too many moves.
    let in_order = increasing(&old_place);
    if old_place.iter().flatten().count() - in_order.len() > MOST_MOVES {
        for (n, old) in old_place.iter_mut().enumerate() {
            if old.is_some() && in_order.binary_search(&n).is_err() {
                *old = None;
            }
        }
    }

    // Removed, last first so the places of the rows before stay as they were.
    let old = model.rows().len();
    let mut kept = vec![false; old];
    for &at in old_place.iter().flatten() {
        kept[at] = true;
    }
    let mut end = old;
    while end > 0 {
        if kept[end - 1] {
            end -= 1;
            continue;
        }
        let start = kept[..end]
            .iter()
            .rposition(|&kept| kept)
            .map_or(0, |at| at + 1);
        remove(model, start..end);
        end = start;
    }

    // Moved: what remains is the kept rows in their old order, numbered here
    // by their new place. Each row out of order goes just after the row
    // before it in the new list; the rows in order never move, so the list
    // ends in the new order.
    let mut order: Vec<usize> = Vec::with_capacity(old_place.len());
    let mut by_old: Vec<Option<usize>> = vec![None; old];
    for (n, &at) in old_place.iter().enumerate() {
        if let Some(at) = at {
            by_old[at] = Some(n);
        }
    }
    order.extend(by_old.into_iter().flatten());
    let mut out_of_order = old_place
        .iter()
        .enumerate()
        .filter(|(n, at)| at.is_some() && in_order.binary_search(n).is_err())
        .map(|(n, _)| n)
        .peekable();
    if out_of_order.peek().is_some() {
        let placed: Vec<usize> = old_place
            .iter()
            .enumerate()
            .filter_map(|(n, at)| at.map(|_| n))
            .collect();
        for n in out_of_order {
            let before = placed[..placed.binary_search(&n).unwrap_or_default()]
                .last()
                .map_or(0, |&previous| {
                    order
                        .iter()
                        .position(|&at| at == previous)
                        .unwrap_or_default()
                        + 1
                });
            let from = order.iter().position(|&at| at == n).unwrap_or_default();
            if from == before {
                continue;
            }
            let to = if from < before { before - 1 } else { before };
            model.begin_move(from, before);
            model.rows().move_row(from, to);
            order.remove(from);
            order.insert(to, n);
            model.end_move();
        }
    }

    // Inserted, in runs, first first: each run's place is its new one, since
    // the rows before it are the new list's already.
    let mut n = 0;
    while n < old_place.len() {
        if old_place[n].is_some() {
            n += 1;
            continue;
        }
        let end = old_place[n..]
            .iter()
            .position(Option::is_some)
            .map_or(old_place.len(), |run| n + run);
        insert(model, n, (n..end).map(|at| new.row(at)));
        n = end;
    }

    // Changed, in runs.
    let mut changed: Vec<Range<usize>> = Vec::new();
    let rows = model.rows();
    rows.gap_at(rows.len());
    for (n, at) in old_place.iter().enumerate() {
        if at.is_some() && new.refresh(n, &mut rows.head[n]) {
            match changed.last_mut() {
                Some(run) if run.end == n => run.end += 1,
                _ => changed.push(n..n + 1),
            }
        }
    }
    for run in changed {
        model.changed(run);
    }
}

fn key(row: &Row) -> (&str, &str) {
    (&row.result.extension, &row.result.id)
}

/// The new places, in order, of a longest run of kept rows whose old places
/// increase too: the most rows that can stay where they are.
fn increasing(old_place: &[Option<usize>]) -> Vec<usize> {
    // tails[k]: the new place of the row ending the best run of length k + 1
    // found so far, the one with the lowest old place.
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; old_place.len()];
    for (n, at) in old_place.iter().enumerate() {
        let Some(at) = *at else { continue };
        let length = tails.partition_point(|&tail| old_place[tail].unwrap_or_default() < at);
        previous[n] = length.checked_sub(1).map(|k| tails[k]);
        if length == tails.len() {
            tails.push(n);
        } else {
            tails[length] = n;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(n) = at {
        run.push(n);
        at = previous[n];
    }
    run.reverse();
    run
}

#[cfg(test)]
mod tests {
    use kotodama_protocol::{Item, ItemKind, OfferedAction};

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
                identity: None,
                actions: vec![
                    Action::new("open", "Open"),
                    Action::new("term", "Open in terminal"),
                ],
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
                (
                    Some("pinner"),
                    "Pin a version here",
                    "pinner/pin".to_owned()
                ),
                (Some("pinner"), "Unpin", "pinner/unpin".to_owned()),
                (Some("git"), "Show log", "git/log".to_owned()),
            ]
        );
        assert_eq!(row.action(5), None);
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
            [
                ("open", Some("ctrl+o")),
                ("copy", None),
                ("share", Some("ctrl+s"))
            ]
        );
        assert_eq!(row.result.extension, "bookmarks");
    }

    #[test]
    fn runs_of_matched_characters_share_one_tag() {
        assert_eq!(
            row("Firefox", &[0, 1, 4]).markup("red"),
            "<font color=\"red\">Fi</font>re<font color=\"red\">f</font>ox"
        );
        assert_eq!(
            row("ab", &[0, 1]).markup("red"),
            "<font color=\"red\">ab</font>"
        );
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
        assert_eq!(
            row("ab", &[1, 7]).markup("red"),
            "a<font color=\"red\">b</font>"
        );
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
                rows: ids
                    .iter()
                    .map(|id| listed(id, &[]))
                    .collect::<Vec<_>>()
                    .into(),
                told: Vec::new(),
                seen: ids.iter().map(|id| id.to_string()).collect(),
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
            made: Default::default(),
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
            [
                Told::Removed(5..6),
                Told::Removed(2..3),
                Told::Removed(0..1)
            ]
        );
        assert_eq!(to.made.get(), 0, "no row is made again");
    }

    #[test]
    fn a_filter_that_widens_inserts_the_rows_that_match_again_in_runs() {
        let to = new(&["a", "b", "c", "d", "e", "f"]);
        let model = diffed(&["b", "e"], &to);
        assert_eq!(
            model.told,
            [
                Told::Inserted(0..1),
                Told::Inserted(2..4),
                Told::Inserted(5..6)
            ]
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
            model
                .told
                .iter()
                .all(|told| !matches!(told, Told::Moved { .. })),
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
            identity: None,
            actions: vec![Action::new("open", "Open")],
        };
        let none: Arc<[OfferedAction]> = Arc::new([]);
        let mut row = Row::listed("bookmarks", &item, none.clone(), vec![0]);
        assert!(!row.relist("bookmarks", &item, &none, &[0]));
        assert!(row.relist("bookmarks", &item, &none, &[1, 2]));
        assert_eq!(row.highlights, [1, 2]);
        item.title = "Alps".into();
        assert!(row.relist("bookmarks", &item, &none, &[1, 2]));
        assert_eq!(row.title, "Alps");
    }
}
