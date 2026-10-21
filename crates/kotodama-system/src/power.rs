//! Power and session: an Item for each of Power Off, Reboot, Suspend,
//! Hibernate, Lock Screen and Log Out, and the form View that asks before the
//! three that end the session.
//!
//! # Owns
//!
//! - The power Items, their keywords, and which of them ask first.
//! - The confirmation View, and finding the Item it asks about from its id.
//!
//! # Not here
//!
//! - Calling logind: [`crate::logind`].
//! - When the Items are sent and what answers a run or a submitted form: `main.rs`.

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;
use kotodama_protocol::NamespacedKind;
use kotodama_protocol::View;
use kotodama_protocol::ViewContent;

/// The Item kind of every power Item.
pub const KIND: &str = "system.power";
/// The primary Action of every power Item: runs it, or asks first.
pub const RUN: &str = "run";
/// What the id of a confirmation View starts with; the Item's id follows.
const CONFIRM: &str = "confirm-";

/// One thing logind does to the machine or the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    PowerOff,
    Reboot,
    Suspend,
    Hibernate,
    Lock,
    LogOut,
}

impl Power {
    /// Every one, in the order the Items are sent.
    pub const ALL: [Power; 6] = [
        Power::PowerOff,
        Power::Reboot,
        Power::Suspend,
        Power::Hibernate,
        Power::Lock,
        Power::LogOut,
    ];

    /// Its Item's id.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Power::PowerOff => "power-off",
            Power::Reboot => "reboot",
            Power::Suspend => "suspend",
            Power::Hibernate => "hibernate",
            Power::Lock => "lock",
            Power::LogOut => "log-out",
        }
    }

    /// The one whose Item has the id `id`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Power> {
        Power::ALL.into_iter().find(|power| power.id() == id)
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Power::PowerOff => "Power Off",
            Power::Reboot => "Reboot",
            Power::Suspend => "Suspend",
            Power::Hibernate => "Hibernate",
            Power::Lock => "Lock Screen",
            Power::LogOut => "Log Out",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Power::PowerOff => "Shut the computer down",
            Power::Reboot => "Restart the computer",
            Power::Suspend => "Sleep, keeping the session in memory",
            Power::Hibernate => "Sleep, saving the session to disk",
            Power::Lock => "Lock the session; the screen locker asks for the password",
            Power::LogOut => "End the session, closing every app in it",
        }
    }

    /// The freedesktop icon names icon themes ship for these.
    fn icon(self) -> &'static str {
        match self {
            Power::PowerOff => "system-shutdown",
            Power::Reboot => "system-reboot",
            Power::Suspend => "system-suspend",
            Power::Hibernate => "system-suspend-hibernate",
            Power::Lock => "system-lock-screen",
            Power::LogOut => "system-log-out",
        }
    }

    /// The other words people type for it.
    fn keywords(self) -> &'static [&'static str] {
        match self {
            Power::PowerOff => &["shutdown", "shut down", "poweroff", "turn off", "halt"],
            Power::Reboot => &["restart", "reboot"],
            Power::Suspend => &["sleep", "standby"],
            Power::Hibernate => &["sleep", "suspend to disk"],
            Power::Lock => &["lock", "lock session"],
            Power::LogOut => &["logout", "log off", "sign out", "end session", "exit"],
        }
    }

    /// Whether it asks before it runs: the three that close every app, so a
    /// stray Enter can't. Suspend and Lock lose nothing.
    #[must_use]
    pub fn asks_first(self) -> bool {
        matches!(self, Power::PowerOff | Power::Reboot | Power::LogOut)
    }

    /// The form View that asks before it runs: no fields, and its title on
    /// the submit button, so a second Enter runs it and Esc doesn't.
    #[must_use]
    pub fn confirmation(self) -> View {
        let question = match self {
            Power::PowerOff => "Power off the computer?",
            Power::Reboot => "Reboot the computer?",
            Power::LogOut => "Log out? Apps that are open close.",
            Power::Suspend | Power::Hibernate | Power::Lock => self.title(),
        };
        View {
            id: format!("{CONFIRM}{}", self.id()),
            title: question.to_owned(),
            content: ViewContent::Form {
                fields: Vec::new(),
                submit: self.title().to_owned(),
            },
        }
    }

    /// The one the confirmation View `view` asks about.
    #[must_use]
    pub fn from_confirmation(view: &str) -> Option<Power> {
        view.strip_prefix(CONFIRM)
            .and_then(Power::from_id)
            .filter(|power| power.asks_first())
    }

    /// Its Item.
    #[must_use]
    pub fn item(self) -> Item {
        Item {
            id: self.id().to_owned(),
            kind: kind(),
            identity: None,
            title: self.title().to_owned(),
            subtitle: Some(self.subtitle().to_owned()),
            icon: Some(self.icon().to_owned()),
            keyword: None,
            keywords: self.keywords().iter().map(|&keyword| keyword.to_owned()).collect(),
            hidden: false,
            app_only: false,
            actions: vec![Action::new(RUN, self.title())],
        }
    }
}

fn kind() -> ItemKind {
    ItemKind::Namespaced(NamespacedKind::new(KIND).expect("the Extension's kind is valid"))
}

/// The power Items: Hibernate only when `hibernate`, as logind can't
/// hibernate a machine without swap to resume from.
pub fn items(hibernate: bool) -> impl Iterator<Item = Item> {
    Power::ALL
        .into_iter()
        .filter(move |&power| hibernate || power != Power::Hibernate)
        .map(Power::item)
}

#[cfg(test)]
mod tests;
