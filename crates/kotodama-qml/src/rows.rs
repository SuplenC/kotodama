//! The Results as the list model holds them, and how a new list replaces the
//! old one without resetting the model. Nothing here touches Qt, so it is
//! tested without it.

use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;

use kotodama_protocol::{Action, ActionRef, Item, OfferedAction, ResultRef, ShownResult};

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
        }
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

    /// What runs the `n`th entry of the Action panel.
    pub fn action(&self, n: usize) -> Option<ActionRef> {
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

/// How the model goes from `old` rows to `new` ones: the rows both have are
/// changed in place, so the list keeps its delegates and its current row, and
/// only the difference in length is inserted or removed.
#[derive(Debug, PartialEq, Eq)]
pub struct Replace {
    pub changed: Range<usize>,
    pub inserted: Range<usize>,
    pub removed: Range<usize>,
}

impl Replace {
    pub fn new(old: usize, new: usize) -> Self {
        let kept = old.min(new);
        Replace {
            changed: 0..kept,
            inserted: kept..new.max(kept),
            removed: kept..old.max(kept),
        }
    }
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
        }
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

    #[test]
    fn a_new_list_changes_the_rows_both_have_and_inserts_or_removes_the_rest() {
        assert_eq!(
            Replace::new(3, 5),
            Replace {
                changed: 0..3,
                inserted: 3..5,
                removed: 3..3,
            }
        );
        assert_eq!(
            Replace::new(5, 2),
            Replace {
                changed: 0..2,
                inserted: 2..2,
                removed: 2..5,
            }
        );
        assert_eq!(
            Replace::new(0, 0),
            Replace {
                changed: 0..0,
                inserted: 0..0,
                removed: 0..0,
            }
        );
    }
}
