//! Hyprland's events, read as lines from its event socket (`.socket2.sock`),
//! each `EVENT>>DATA`. Only the ones that change an Item are kept; Hyprland
//! sends many more, about focus, monitors and layers.
//!
//! # Owns
//!
//! - Which events change an Item, and what each one's data says.
//!
//! # Not here
//!
//! - Reading the lines from the socket: [`crate::hyprland::Events`].
//! - Applying a change to the windows: [`crate::windows::Windows::apply`].

/// An event that changes a window Item, or the binds and their keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A window was mapped. `address` is as `j/clients` writes it, `0x…`.
    Opened {
        address: String,
        workspace: String,
        class: String,
        title: String,
    },
    Closed {
        address: String,
    },
    Moved {
        address: String,
        workspace: String,
    },
    Retitled {
        address: String,
        title: String,
    },
    /// The config was read again, so the binds may have changed.
    ConfigReloaded,
    /// A keyboard's layout changed, or a keyboard got a keymap, so a key
    /// may have another name.
    LayoutChanged,
}

impl Change {
    /// The change one event line makes; `None` for the events that change no
    /// Item, and for a line that isn't an event.
    pub fn parse(line: &str) -> Option<Change> {
        let (event, data) = line.split_once(">>")?;
        Some(match event {
            "openwindow" => {
                // The title is last, and may hold commas.
                let mut fields = data.splitn(4, ',');
                let address = address(fields.next()?);
                let workspace = fields.next()?.to_owned();
                let class = fields.next()?.to_owned();
                let title = fields.next()?.to_owned();
                Change::Opened {
                    address,
                    workspace,
                    class,
                    title,
                }
            }
            "closewindow" => Change::Closed { address: address(data) },
            // `movewindow` and `windowtitle` come first and say less: the
            // first names the workspace without its id, the second only the
            // window.
            "movewindowv2" => {
                let mut fields = data.splitn(3, ',');
                let address = address(fields.next()?);
                let _id = fields.next()?;
                let workspace = fields.next()?.to_owned();
                Change::Moved { address, workspace }
            }
            "windowtitlev2" => {
                let (window, title) = data.split_once(',')?;
                Change::Retitled {
                    address: address(window),
                    title: title.to_owned(),
                }
            }
            "configreloaded" => Change::ConfigReloaded,
            "activelayout" => Change::LayoutChanged,
            _ => return None,
        })
    }
}

/// Events write a window's address in hex without the `0x` that `j/clients`
/// and dispatchers use.
fn address(hex: &str) -> String {
    format!("0x{hex}")
}
