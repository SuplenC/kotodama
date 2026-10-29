//! The Daemon's real messages, as the format benchmarks and the protobuf
//! interoperability test send them: a query, a list of Results, and a batch
//! of Items.

use kotodama_protocol::Action;
use kotodama_protocol::Argument;
use kotodama_protocol::FromExtension;
use kotodama_protocol::FromFrontEnd;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::OfferedAction;
use kotodama_protocol::Scope;
use kotodama_protocol::ShownResult;
use kotodama_protocol::ToFrontEnd;

/// As many Results as the Daemon sends for one query.
pub const RESULTS: usize = 50;

/// A batch of Items, as an Extension sends while it indexes.
pub const BATCH: usize = 1_000;

pub fn query() -> FromFrontEnd {
    FromFrontEnd::Query { text: "firefox".into() }
}

pub fn item(n: usize) -> Item {
    Item {
        id: format!("org.example.App{n}.desktop"),
        kind: ItemKind::App {
            desktop_id: format!("org.example.App{n}.desktop"),
        },
        identity: Some(format!("app:org.example.App{n}.desktop")),
        title: format!("Example App {n}"),
        subtitle: Some("A typical application with a one-line description".into()),
        icon: Some(format!("org.example.App{n}")),
        keyword: None,
        keywords: vec!["example".into(), "utility".into(), format!("app{n}")],
        actions: vec![
            Action::new("open", "Open"),
            Action::new("new-window", "Open a new window").shortcut("ctrl+n"),
        ],
        hidden: false,
        app_only: false,
        weak_keywords: Vec::new(),
    }
}

pub fn results() -> ToFrontEnd {
    ToFrontEnd::Results {
        version: 7,
        results: (0..RESULTS)
            .map(|n| ShownResult {
                extension: "apps".into(),
                item: item(n),
                contributed: vec![
                    OfferedAction {
                        extension: "folders".into(),
                        action: Action::new("pin", "Pin to the menu")
                            .arguments(vec![Argument::new("where").placeholder("top").optional()]),
                    },
                    OfferedAction {
                        extension: "kotodama".into(),
                        action: Action::new("forget", "Forget").shortcut("ctrl+shift+delete"),
                    },
                ],
                highlights: vec![0, 1, 2, 3, 8],
                alias: None,
                unavailable: None,
            })
            .collect(),
        scope: Some(Scope {
            word: "apps".into(),
            title: "Apps".into(),
            icon: None,
            intro: None,
        }),
    }
}

pub fn batch() -> FromExtension {
    FromExtension::AddItems {
        items: (0..BATCH).map(item).collect(),
    }
}
