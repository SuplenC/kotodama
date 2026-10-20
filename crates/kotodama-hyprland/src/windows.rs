//! Hyprland's open windows, read once from `j/clients` (what `hyprctl
//! clients -j` prints) and then kept up to date from its events, as Items of
//! the shared kind `window`, by address.

use std::collections::BTreeMap;

use kotodama_protocol::{Action, Argument, Item, ItemKind};
use serde::Deserialize;

use crate::events::Change;

pub const FOCUS: &str = "focus";
pub const CLOSE: &str = "close";
pub const MOVE: &str = "move";
pub const FLOAT: &str = "float";
/// The Argument [`MOVE`] asks for.
pub const WORKSPACE: &str = "Workspace";

#[derive(Debug, PartialEq, Eq)]
struct Window {
    class: String,
    title: String,
    workspace: String,
}

/// The open windows, by address.
#[derive(Debug, Default)]
pub struct Windows(BTreeMap<String, Window>);

#[derive(Deserialize)]
struct Client {
    address: String,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    workspace: ClientWorkspace,
}

#[derive(Deserialize)]
struct ClientWorkspace {
    name: String,
}

impl Windows {
    /// Reads `j/clients`. A client not mapped yet isn't a window to show:
    /// its `openwindow` event is still to come.
    pub fn from_clients(json: &str) -> Result<Self, String> {
        let clients: Vec<Client> = serde_json::from_str(json)
            .map_err(|err| format!("couldn't read Hyprland's windows: {err}"))?;
        Ok(Windows(
            clients
                .into_iter()
                .filter(|client| client.mapped)
                .map(|client| {
                    let window = Window {
                        class: client.class,
                        title: client.title,
                        workspace: client.workspace.name,
                    };
                    (client.address, window)
                })
                .collect(),
        ))
    }

    /// Applies one event; says whether a window changed.
    pub fn apply(&mut self, change: &Change) -> bool {
        match change {
            Change::Opened {
                address,
                workspace,
                class,
                title,
            } => {
                let window = Window {
                    class: class.clone(),
                    title: title.clone(),
                    workspace: workspace.clone(),
                };
                let old = self.0.insert(address.clone(), window);
                old.as_ref() != self.0.get(address)
            }
            Change::Closed { address } => self.0.remove(address).is_some(),
            Change::Moved { address, workspace } => set(
                self.0.get_mut(address).map(|window| &mut window.workspace),
                workspace,
            ),
            Change::Retitled { address, title } => set(
                self.0.get_mut(address).map(|window| &mut window.title),
                title,
            ),
            Change::ConfigReloaded => false,
        }
    }

    /// Every window as an Item, by address.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.0.iter().map(|(address, window)| item(address, window))
    }
}

/// Sets `field`, if the window is there, to `value`; says whether it changed.
fn set(field: Option<&mut String>, value: &str) -> bool {
    match field {
        Some(field) if field != value => {
            value.clone_into(field);
            true
        }
        _ => false,
    }
}

fn item(address: &str, window: &Window) -> Item {
    let title = if window.title.is_empty() {
        window.class.clone()
    } else {
        window.title.clone()
    };
    Item {
        id: address.to_owned(),
        kind: ItemKind::Window {
            address: address.to_owned(),
        },
        identity: None,
        title,
        subtitle: Some(format!("{} · workspace {}", window.class, window.workspace)),
        // An app's class is most often its desktop id, so its icon's name.
        icon: (!window.class.is_empty()).then(|| window.class.clone()),
        keyword: None,
        keywords: vec![window.class.clone()],
        hidden: false,
        actions: vec![
            Action::new(FOCUS, "Focus Window"),
            Action::new(CLOSE, "Close Window"),
            Action::new(MOVE, "Move to Workspace").arguments(vec![Argument {
                name: WORKSPACE.to_owned(),
                placeholder: Some("1, 2 or a name".to_owned()),
                optional: false,
            }]),
            Action::new(FLOAT, "Toggle Floating"),
        ],
    }
}

/// What `dispatch` runs for `action` on the window at `address`; `workspace`
/// is the [`MOVE`] Action's Argument.
pub fn dispatch(action: &str, address: &str, workspace: Option<&str>) -> Result<String, String> {
    Ok(match action {
        FOCUS => format!("focuswindow address:{address}"),
        CLOSE => format!("closewindow address:{address}"),
        FLOAT => format!("togglefloating address:{address}"),
        MOVE => {
            let workspace = workspace
                .map(str::trim)
                .filter(|workspace| !workspace.is_empty())
                .ok_or("Name a workspace to move the window to")?;
            format!("movetoworkspacesilent {workspace},address:{address}")
        }
        _ => return Err(format!("A window has no Action `{action}`")),
    })
}
