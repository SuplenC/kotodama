//! Hyprland's binds, read from `j/binds` (what `hyprctl binds -j` prints):
//! the ones with a description as Indexed items, and every one in the
//! Shortcuts View, grouped by modifier.
//!
//! Under Hyprland's Lua config every bind runs a Lua function: its dispatcher
//! is `__lua`, and its argument the function's reference in the Lua
//! registry. A bind on a keycode (`code:12`) has no key name in `j/binds`;
//! its keycode is there under hyprlang, and under Lua only in the plain
//! `binds` listing.
//!
//! # Owns
//!
//! - Parsing `j/binds`, and a bind's keys and dispatch in words.
//! - A bind's keycode from the plain `binds` listing, and the name a keycode bind's key is shown
//!   by.
//! - The binds' Items, their ids, and the Shortcuts Item and View.
//!
//! # Not here
//!
//! - Asking Hyprland for the binds and running a dispatch: [`crate::hyprland`].
//! - What a keycode's key is called: [`crate::keymap`].
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
/// The dispatcher of every bind in Hyprland's Lua config.
const LUA: &str = "__lua";
/// How a key is written on a keycode, in a config and in the plain listing.
const CODE: &str = "code:";
/// What the plain `binds` listing puts before a bind's key.
const LISTED_KEY: &str = "\tkey: ";

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
    /// For a bind on a keycode, its key's name under the current layout, as
    /// [`name_keys`] found it; `j/binds` has none.
    #[serde(skip)]
    pub layout_key: Option<String>,
}

impl Bind {
    /// What `dispatch` runs for this bind: its dispatcher and argument, or
    /// under Lua the bind's function, from the Lua registry. Hyprland runs
    /// that as `hl.dispatch(debug.getregistry()[N])`, which calls it as a
    /// keypress would. `None` for a Lua bind without a reference, as Hyprland
    /// leaves one it unbound.
    pub fn dispatch(&self) -> Option<String> {
        if self.is_lua() {
            let reference = self.arg.parse::<u32>().ok().filter(|&reference| reference > 0)?;
            Some(format!("debug.getregistry()[{reference}]"))
        } else if self.arg.is_empty() {
            Some(self.dispatcher.clone())
        } else {
            Some(format!("{} {}", self.dispatcher, self.arg))
        }
    }

    /// Whether the bind runs a function of Hyprland's Lua config.
    pub fn is_lua(&self) -> bool {
        self.dispatcher == LUA
    }

    /// Whether the bind is on a keycode, which [`name_keys`] names.
    pub fn on_keycode(&self) -> bool {
        !self.catch_all && self.key.is_empty() && self.keycode != 0
    }

    /// Whether `j/binds` lost the bind's keycode, as it does for a bind on a
    /// keycode under Lua: the plain listing has it
    /// ([`keycodes_from_listing`]).
    pub fn lacks_keycode(&self) -> bool {
        !self.catch_all && self.key.is_empty() && self.keycode == 0
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

    /// The key, as the config names it; a single letter is upper case. A
    /// keycode is `code:12` here whatever the layout, so an id made of it
    /// stays the same when the layout changes.
    fn written_key(&self) -> String {
        if self.catch_all {
            "catchall".to_owned()
        } else if self.key.is_empty() {
            format!("{CODE}{}", self.keycode)
        } else {
            upper_letter(&self.key)
        }
    }

    /// The key as it is shown: a keycode's by its name under the current
    /// layout, where one was found.
    fn key_name(&self) -> String {
        match &self.layout_key {
            Some(name) if self.on_keycode() => upper_letter(name),
            _ => self.written_key(),
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
    /// A Lua function's reference says nothing, so it is just a Lua function.
    fn summary(&self) -> String {
        if !self.description.is_empty() {
            self.description.clone()
        } else if self.is_lua() {
            "Lua function".to_owned()
        } else {
            self.dispatch().unwrap_or_default()
        }
    }

    /// The words besides its keys a bind is found by: its dispatcher and
    /// argument, which under Lua are a reference that says nothing.
    fn dispatch_words(&self) -> [&str; 2] {
        if self.is_lua() {
            ["", ""]
        } else {
            [&self.dispatcher, &self.arg]
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

/// A bind's Item, which runs it. Binds stay in the Hyprland app, out of the
/// search, where hundreds of them would crowd out what is looked for.
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
        app_only: true,
        weak_keywords: Vec::new(),
        actions: vec![Action::new(RUN, "Run")],
    }
}

/// The Item that opens the Shortcuts View, which `kotodama open --view
/// hyprland/shortcuts` names: the app's command, listed first in it.
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
        app_only: true,
        weak_keywords: Vec::new(),
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

/// `key`, upper case if it is a single letter, as `Q`.
fn upper_letter(key: &str) -> String {
    if key.chars().count() == 1 {
        key.to_uppercase()
    } else {
        key.to_owned()
    }
}

/// Gives each bind that [lacks its keycode](Bind::lacks_keycode) the one
/// the plain `binds` listing (`hyprctl binds`) shows for it. That listing
/// writes a bind without a key name by the keys its config gave it, as
/// `SUPER + SHIFT + code:12`, in the same order as `j/binds`. A listing with
/// another number of binds, read after the binds changed, is left alone.
pub fn keycodes_from_listing(binds: &mut [Bind], listing: &str) {
    let keys: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.strip_prefix(LISTED_KEY))
        .collect();
    if keys.len() != binds.len() {
        return;
    }
    for (bind, keys) in binds.iter_mut().zip(keys) {
        if !bind.lacks_keycode() {
            continue;
        }
        let last = keys.rsplit('+').next().unwrap_or_default().trim();
        if let Some(code) = last.strip_prefix(CODE).and_then(|code| code.parse().ok()) {
            bind.keycode = code;
        }
    }
}

/// Names the key of each bind [on a keycode](Bind::on_keycode) by
/// `name(keycode)`, the keysym it types under the current layout; one it
/// gives no name stays `code:N`.
pub fn name_keys(binds: &mut [Bind], name: impl Fn(u32) -> Option<String>) {
    for bind in binds.iter_mut().filter(|bind| bind.on_keycode()) {
        bind.layout_key = name(bind.keycode);
    }
}

/// Each bind's id: its submap, modifiers and key, so it stays the same however
/// often the binds are read, and a keycode's whatever the layout. A second
/// bind on the same keys gets `#2`.
fn ids(binds: &[Bind]) -> Vec<String> {
    let mut seen = HashMap::new();
    binds
        .iter()
        .map(|bind| {
            let id = format!("{}/{}/{}", bind.submap, bind.modmask, bind.written_key());
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
            let [dispatcher, _] = bind.dispatch_words();
            let keywords = (!dispatcher.is_empty()).then(|| dispatcher.to_owned());
            bind_item(
                id,
                bind.description.clone(),
                bind.in_submap(bind.keys()),
                keywords.into_iter().collect(),
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
            let [dispatcher, arg] = bind.dispatch_words();
            let keywords = [bind.description.as_str(), dispatcher, arg]
                .into_iter()
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
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
