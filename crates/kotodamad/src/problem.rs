//! Problems: what went wrong that the user can fix, gathered where it is
//! found rather than left in the journal. The Daemon sends the list to every
//! Front end in `ToFrontEnd::Status`, offers it as one of its own Items, and
//! `kotodama check` prints it.
//!
//! Every problem is found when the config, the Manifests or an Extension's
//! state changes, never on a keystroke.
//!
//! # Owns
//!
//! - [`Report`]: collects the problems found while reading something.
//! - [`Problems`]: the Daemon's problems, by where they were found.
//! - [`item`] and [`view`]: the Daemon's own Item that lists them, and the View it opens.
//!
//! # Not here
//!
//! - The problems of Extensions that are down, and when the list is sent: `daemon.rs`.
//! - Reading the settings that report them: each setting's owner (`engine.rs`, `paths.rs`,
//!   `history.rs`, `empty_query.rs`, `kind_order.rs`, `keywords.rs`, `configured.rs`, `apps.rs`).

use std::fmt::Write as _;

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::Problem;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

/// The id of the Daemon's own Item that opens the list, and of the View it
/// opens: what `Problem::list` names, for Front ends.
pub const PROBLEMS: &str = "problems";
/// The id of that Item's one Action: what `Problem::show` names.
pub const SHOW: &str = "show";

/// The problems found while reading the config or the Manifests. Each one is
/// also logged as it is found, for the journal.
#[derive(Debug, Default)]
pub struct Report {
    problems: Vec<Problem>,
    /// Logs nothing: what it finds is reported again where it is told.
    silent: bool,
}

impl Report {
    pub fn new() -> Self {
        Report::default()
    }

    /// A report that logs nothing, for reading settings before the Daemon
    /// that reports them is there.
    pub fn silent() -> Self {
        Report {
            silent: true,
            ..Report::default()
        }
    }

    /// Notes that something is wrong with `about`, such as a config key.
    pub fn add(&mut self, about: impl Into<String>, message: impl Into<String>) {
        let problem = Problem {
            about: about.into(),
            message: message.into(),
        };
        if !self.silent {
            log::warn!("{}: {}", problem.about, problem.message);
        }
        self.problems.push(problem);
    }

    pub fn into_problems(self) -> Vec<Problem> {
        self.problems
    }
}

/// The Daemon's problems, by where they were found: each group is replaced
/// whole when its source is read again.
#[derive(Debug, Default)]
pub struct Problems {
    /// The config file that can't be read, while the last good config is
    /// kept.
    pub unreadable: Option<Problem>,
    /// The settings, read from the config.
    pub config: Vec<Problem>,
    /// The Manifests refused or out of date.
    pub manifests: Vec<Problem>,
    /// The Extensions' tables in the config: what they switch off, change
    /// or define.
    pub extensions: Vec<Problem>,
    /// The apps' settings, read from the config and the Manifests.
    pub apps: Vec<Problem>,
    /// The keywords two things ask for.
    pub conflicts: Vec<Problem>,
    /// Whether a group changed since the list was last put together.
    pub dirty: bool,
    /// The whole list as last put together.
    pub sent: Vec<Problem>,
    /// Whether the Front ends have yet to hear of a change to it.
    pub unsent: bool,
}

impl Problems {
    /// Every problem but those of Extensions that are down, which follow.
    pub fn list(&self, down: impl IntoIterator<Item = Problem>) -> Vec<Problem> {
        (self.unreadable.iter().cloned())
            .chain(self.config.iter().cloned())
            .chain(self.manifests.iter().cloned())
            .chain(self.extensions.iter().cloned())
            .chain(self.apps.iter().cloned())
            .chain(self.conflicts.iter().cloned())
            .chain(down)
            .collect()
    }
}

/// Replaces `group` with `problems`, and returns whether that changed it.
pub fn replace(group: &mut Vec<Problem>, problems: Vec<Problem>) -> bool {
    let changed = *group != problems;
    if changed {
        *group = problems;
    }
    changed
}

/// The Daemon's own Item that opens the list of `count` problems: found by
/// "problems", and shown only while there are any.
///
/// # Panics
///
/// Never: `kotodama.command` is a valid kind.
pub fn item(count: usize) -> Item {
    let title = match count {
        1 => "1 problem".to_owned(),
        n => format!("{n} problems"),
    };
    Item {
        id: PROBLEMS.into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("kotodama.command").expect("a valid kind")),
        title,
        subtitle: Some("What went wrong in the config and the Extensions".into()),
        icon: Some("dialog-warning".into()),
        keyword: None,
        identity: None,
        keywords: vec!["problems".into(), "check".into()],
        hidden: false,
        app_only: false,
        actions: vec![Action::new(SHOW, "Show")],
    }
}

/// The View that lists `problems`, one per line, by what each is about.
pub fn view(problems: &[Problem]) -> View {
    let markdown = if problems.is_empty() {
        "Nothing is wrong.".to_owned()
    } else {
        problems.iter().fold(String::new(), |mut markdown, problem| {
            let _ = writeln!(
                markdown,
                "- **{}**: {}",
                escape(&problem.about),
                escape(&problem.message)
            );
            markdown
        })
    };
    View {
        id: PROBLEMS.into(),
        title: "Problems".into(),
        content: ViewContent::Detail {
            markdown,
            metadata: Vec::new(),
        },
    }
}

/// `text` as Markdown that shows it as it is, but for its code spans in
/// backticks, which stay code: a key such as `half_life_days` or a path
/// would otherwise turn into emphasis or a link.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut code = false;
    for c in text.chars() {
        if c == '`' {
            code = !code;
        } else if !code && matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '#') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_shows_the_text_as_it_is_but_keeps_code_spans() {
        assert_eq!(
            escape("history.half_life_days in ~/a_b.toml: `word_x` *x*"),
            "history.half\\_life\\_days in ~/a\\_b.toml: `word_x` \\*x\\*"
        );
    }

    #[test]
    fn front_ends_name_the_item_and_its_action_as_the_daemon_does() {
        assert_eq!(Problem::list().id, PROBLEMS);
        assert_eq!(Problem::show().id, SHOW);
        assert_eq!(Problem::list().extension, crate::daemon::OWN);
    }

    #[test]
    fn the_item_counts_the_problems() {
        assert_eq!(item(1).title, "1 problem");
        assert_eq!(item(3).title, "3 problems");
    }
}
