//! The Daemon's real messages, as the format benchmarks and the protobuf
//! interoperability test send them: a query, a list of Results, and a batch
//! of Items.

use kotodama_protocol::{
    Action, Argument, FromExtension, FromFrontEnd, Item, ItemKind, OfferedAction, Scope,
    ShownResult, ToFrontEnd,
};

/// As many Results as the Daemon sends for one query.
pub const RESULTS: usize = 50;

/// A batch of Items, as an Extension sends while it indexes.
pub const BATCH: usize = 1_000;

pub fn query() -> FromFrontEnd {
    FromFrontEnd::Query {
        text: "firefox".into(),
    }
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
                        action: Action::new("pin", "Pin to the menu").arguments(vec![Argument {
                            name: "where".into(),
                            placeholder: Some("top".into()),
                            optional: true,
                        }]),
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
            prefix: "apps".into(),
            title: "Apps".into(),
            icon: None,
        }),
    }
}

pub fn batch() -> FromExtension {
    FromExtension::AddItems {
        items: (0..BATCH).map(item).collect(),
    }
}
