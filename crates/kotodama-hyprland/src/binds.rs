//! Hyprland's binds, read from `j/binds` (what `hyprctl binds -j` prints):
//! the ones with a description as Indexed items, and every one in the
//! Shortcuts View, grouped by modifier.
//!
//! # Owns
//!
//! - Parsing `j/binds`, and a bind's keys and dispatch in words.
//! - The binds' Items, their ids, and the Shortcuts Item and View.
//!
//! # Not here
//!
//! - Asking Hyprland for the binds and running a dispatch: [`crate::hyprland`].
//! - When the binds are read again and sent: `main.rs`.

use std::collections::HashMap;

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;
use serde::Deserialize;

/// The Action that runs a bind's dispatcher.
pub const RUN: &str = "run";
/// The Shortcuts View's id.
pub const VIEW: &str = "shortcuts";
/// The id of the Item that opens the Shortcuts View: `--view
/// hyprland/shortcuts`. The View has the same id, but they are two things.
pub const SHORTCUTS: &str = "shortcuts";
/// The Shortcuts Item's one Action, which pushes the View.
pub const OPEN: &str = "open";
/// A bind's Item kind.
pub const KIND: &str = "hyprland.shortcut";
const SHORTCUTS_KIND: &str = "hyprland.shortcuts";
const ICON: &str = "input-keyboard";

/// Hyprland's modifier bits, in the order a bind's keys are written.
const MODIFIERS: [(u32, &str); 8] = [
    (64, "SUPER"),
    (4, "CTRL"),
    (8, "ALT"),
    (1, "SHIFT"),
    (2, "CAPS"),
    (16, "MOD2"),
    (32, "MOD3"),
    (128, "MOD5"),
];

/// One bind, with only the fields Kotodama shows or runs.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Bind {
    #[serde(default)]
    pub modmask: u32,
    #[serde(default)]
    pub submap: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub keycode: u32,
    #[serde(default)]
    pub catch_all: bool,
    #[serde(default)]
    pub mouse: bool,
    #[serde(default)]
    pub description: String,
    pub dispatcher: String,
    #[serde(default)]
    pub arg: String,
}

impl Bind {
    /// What `dispatch` runs for this bind: its dispatcher and argument.
    pub fn dispatch(&self) -> String {
        if self.arg.is_empty() {
            self.dispatcher.clone()
        } else {
            format!("{} {}", self.dispatcher, self.arg)
        }
    }

    /// The modifiers, as `SUPER + SHIFT`; empty without any.
    pub fn modifiers(&self) -> String {
        MODIFIERS
            .iter()
            .filter(|(bit, _)| self.modmask & bit != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// The key, as the config names it; a single letter is upper case.
    fn key_name(&self) -> String {
        if self.catch_all {
            "catchall".to_owned()
        } else if self.key.is_empty() {
            format!("code:{}", self.keycode)
        } else if self.key.chars().count() == 1 {
            self.key.to_uppercase()
        } else {
            self.key.clone()
        }
    }

    /// The whole combination, as `SUPER + SHIFT + S`.
    pub fn keys(&self) -> String {
        let modifiers = self.modifiers();
        if modifiers.is_empty() {
            self.key_name()
        } else {
            format!("{modifiers} + {}", self.key_name())
        }
    }

    /// What the bind does, in words: its description, or else its dispatch.
    fn summary(&self) -> String {
        if self.description.is_empty() {
            self.dispatch()
        } else {
            self.description.clone()
        }
    }

    /// `text`, followed by the bind's submap when it is in one.
    fn in_submap(&self, text: String) -> String {
        if self.submap.is_empty() {
            text
        } else {
            format!("{text} · submap {}", self.submap)
        }
    }
}

/// The error [`parse`] returns.
#[derive(Debug, thiserror::Error)]
#[error("couldn't read Hyprland's binds")]
pub struct ParseError(#[source] serde_json::Error);

impl ParseError {
    fn json(err: serde_json::Error) -> Self {
        ParseError(err)
    }
}

fn kind(name: &str) -> ItemKind {
    ItemKind::Namespaced(NamespacedKind::new(name).expect("the Extension's kinds are valid"))
}

/// A bind's Item, which runs it.
fn bind_item(id: String, title: String, subtitle: String, keywords: Vec<String>) -> Item {
    Item {
        id,
        kind: kind(KIND),
        identity: None,
        title,
        subtitle: Some(subtitle),
        icon: Some(ICON.to_owned()),
        keyword: None,
        keywords,
        hidden: false,
        actions: vec![Action::new(RUN, "Run")],
    }
}

/// The Item that opens the Shortcuts View, which `kotodama open --view
/// hyprland/shortcuts` names. The search finds it too.
pub fn shortcuts_item() -> Item {
    Item {
        id: SHORTCUTS.to_owned(),
        kind: kind(SHORTCUTS_KIND),
        identity: None,
        title: "Keyboard Shortcuts".to_owned(),
        subtitle: Some("Hyprland's binds, by modifier".to_owned()),
        icon: Some(ICON.to_owned()),
        keyword: None,
        keywords: ["keybinds", "hotkeys", "hyprland"].map(str::to_owned).to_vec(),
        hidden: false,
        actions: vec![Action::new(OPEN, "Show Keyboard Shortcuts")],
    }
}

/// Reads `j/binds`.
///
/// # Errors
///
/// [`ParseError`] when `json` is not a list of binds.
pub fn parse(json: &str) -> Result<Vec<Bind>, ParseError> {
    serde_json::from_str(json).map_err(ParseError::json)
}

/// Each bind's id: its submap, modifiers and key, so it stays the same however
/// often the binds are read. A second bind on the same keys gets `#2`.
fn ids(binds: &[Bind]) -> Vec<String> {
    let mut seen = HashMap::new();
    binds
        .iter()
        .map(|bind| {
            let id = format!("{}/{}/{}", bind.submap, bind.modmask, bind.key_name());
            let count = seen.entry(id.clone()).or_insert(0);
            *count += 1;
            if *count == 1 { id } else { format!("{id}#{count}") }
        })
        .collect()
}

/// The bind an Item stands for, by the Item's id.
pub fn find<'a>(binds: &'a [Bind], id: &str) -> Option<&'a Bind> {
    ids(binds).iter().position(|bind| bind == id).map(|at| &binds[at])
}

/// Every bind with a description, as an Indexed item titled by it, with its
/// keys beside. Mouse binds are left out: the menu can't run them.
pub fn items(binds: &[Bind]) -> Vec<Item> {
    binds
        .iter()
        .zip(ids(binds))
        .filter(|(bind, _)| !bind.description.is_empty() && !bind.mouse)
        .map(|(bind, id)| {
            bind_item(
                id,
                bind.description.clone(),
                bind.in_submap(bind.keys()),
                vec![bind.dispatcher.clone()],
            )
        })
        .collect()
}

/// The Shortcuts View: every bind, titled by its keys. Grouped by modifier:
/// binds outside a submap first, then by how many modifiers they need and
/// their names, so binds without one lead; within a group, the config's
/// order.
pub fn view(binds: &[Bind]) -> View {
    let mut rows: Vec<(&Bind, String)> = binds.iter().zip(ids(binds)).collect();
    rows.sort_by_cached_key(|(bind, _)| {
        (
            !bind.submap.is_empty(),
            bind.submap.clone(),
            bind.modmask.count_ones(),
            bind.modifiers(),
        )
    });
    let items = rows
        .into_iter()
        .map(|(bind, id)| {
            let keywords = [&bind.description, &bind.dispatcher, &bind.arg]
                .into_iter()
                .filter(|word| !word.is_empty())
                .cloned()
                .collect();
            bind_item(id, bind.keys(), bind.in_submap(bind.summary()), keywords)
        })
        .collect();
    View {
        id: VIEW.to_owned(),
        title: "Keyboard Shortcuts".to_owned(),
        content: ViewContent::List {
            items,
            filters_itself: false,
        },
    }
}
