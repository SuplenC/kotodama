//! Picking the Item a confirmed Proposal runs on, when several Indexed items
//! offer its Action: a list View of the Daemon's own that lists them.
//!
//! # Owns
//! - [`Pick`], what runs once the user picks, and [`view`], the list.
//!
//! # Not here
//! - Showing, filtering and running a pick: `daemon/picks.rs:Daemon::offer_pick` and the
//!   `FromFrontEnd` arms that call `Daemon::run_pick`.

use std::collections::BTreeMap;

use kotodama_protocol::Action;
use kotodama_protocol::ActionRef;
use kotodama_protocol::Item;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

use crate::daemon::ExtensionId;

/// The id of the one Action on each Item of a pick's list: it runs the
/// proposed Action on that Item.
pub const PICK: &str = "pick";

/// A proposed Action waiting for the user to pick the Item it runs on.
pub struct Pick {
    /// The id of the list View that asks.
    pub view: String,
    /// The Extension whose Items the list holds, which the Action runs on.
    pub target: ExtensionId,
    pub action: ActionRef,
    /// The Arguments the Proposal filled in.
    pub arguments: BTreeMap<String, String>,
}

/// The list View asking which of `offering` the Action titled `title` runs
/// on. Each Item keeps what the search shows of it, with [`PICK`] as its one
/// Action, so the list offers nothing else.
pub fn view(id: String, title: &str, offering: Vec<Item>) -> View {
    let items = offering
        .into_iter()
        .map(|item| Item {
            keyword: None,
            hidden: false,
            app_only: false,
            actions: vec![Action::new(PICK, title)],
            ..item
        })
        .collect();
    View {
        id,
        title: format!("Pick where to run “{title}”"),
        content: ViewContent::List {
            items,
            filters_itself: false,
        },
    }
}
