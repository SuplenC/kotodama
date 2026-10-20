//! Drives the emoji Extension the way the Daemon does, over its stdio.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;

const WAIT: Duration = Duration::from_secs(10);

struct Emoji(Spawned);

impl Emoji {
    /// Starts the Extension and sends it `config`, as the Daemon does.
    fn start(config: Table) -> Self {
        let mut extension = Spawned::start(&mut Command::new(env!("CARGO_BIN_EXE_kotodama-emoji")), WAIT);
        extension.send(ToExtension::Config(config));
        Emoji(extension)
    }

    /// The Items the Extension indexes, which it sends once, all at once.
    fn items(&self) -> Vec<Item> {
        self.0.added()
    }

    fn run(&mut self, id: &str, action: &str) -> Outcome {
        let item = ItemRef {
            id: id.into(),
            kind: ItemKind::Namespaced(NamespacedKind::new("emoji.character").expect("a valid kind")),
        };
        self.0.start_run(3, action, item, BTreeMap::new());
        self.0
            .outcome(3, |message| matches!(message, FromExtension::AddItems { .. }))
    }
}

fn typed(text: &str) -> Outcome {
    Outcome::Type { text: text.into() }
}

fn tone(name: &str) -> Table {
    [("skin_tone".to_owned(), Value::String(name.into()))]
        .into_iter()
        .collect()
}

fn action_ids(item: &Item) -> Vec<&str> {
    item.actions.iter().map(|action| action.id.as_str()).collect()
}

#[test]
fn every_emoji_and_symbol_is_one_indexed_item() {
    let emoji = Emoji::start(Table::new());
    let items = emoji.items();
    let find = |id: &str| {
        items
            .iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("no Item {id}"))
    };

    let thumbs = find("👍");
    assert_eq!(thumbs.title, "👍 Thumbs up");
    assert_eq!(thumbs.kind.name(), "emoji.character");
    assert!(thumbs.keywords.iter().any(|word| word == "like"));
    // Skin tones are Actions of the one Item, not Items of their own.
    assert!(!items.iter().any(|item| item.id == "👍🏽"));
    assert_eq!(
        action_ids(thumbs),
        [
            "type",
            "copy",
            "type-light",
            "type-medium-light",
            "type-medium",
            "type-medium-dark",
            "type-dark"
        ]
    );
    assert_eq!(thumbs.actions[4].title, "Type 👍🏽");

    let arrow = find("→");
    assert_eq!(arrow.title, "→ Right-pointing arrow");
    assert_eq!(action_ids(arrow), ["type", "copy"]);
    assert_eq!(find("⌘").title, "⌘ Command key");
    assert!(find("⌥").keywords.iter().any(|word| word == "alt"));
    // CLDR names ⇧ for its shape; the key's name is a keyword.
    assert_eq!(find("⇧").title, "⇧ Upwards hollow arrow");
    assert!(find("⇧").keywords.iter().any(|word| word == "shift"));
    assert_eq!(find("🇵🇱").title, "🇵🇱 Flag: Poland");

    assert!(items.len() > 2_000, "{} Items", items.len());
}

#[test]
fn the_primary_action_types_the_character_and_the_second_copies_it() {
    let mut emoji = Emoji::start(Table::new());
    emoji.items();
    assert_eq!(emoji.run("👍", "type"), typed("👍"));
    assert_eq!(emoji.run("→", "copy"), Outcome::Copy(Clipboard::Text("→".into())));
}

#[test]
fn a_skin_tone_action_types_the_toned_emoji() {
    let mut emoji = Emoji::start(Table::new());
    emoji.items();
    assert_eq!(emoji.run("👍", "type-dark"), typed("👍🏿"));
    assert_eq!(emoji.run("🤝", "type-medium-light"), typed("🤝🏼"));
    // A multi-person emoji's tone is the whole sequence's.
    assert_eq!(emoji.run("💏", "type-light"), typed("💏🏻"));
}

#[test]
fn the_configured_skin_tone_is_what_the_primary_and_copy_actions_use() {
    let mut emoji = Emoji::start(tone("medium"));
    emoji.items();
    assert_eq!(emoji.run("👍", "type"), typed("👍🏽"));
    assert_eq!(emoji.run("👍", "copy"), Outcome::Copy(Clipboard::Text("👍🏽".into())));
    // Without skin tones, the emoji is itself.
    assert_eq!(emoji.run("🔥", "type"), typed("🔥"));
    // An explicit tone still wins.
    assert_eq!(emoji.run("👍", "type-light"), typed("👍🏻"));
}

#[test]
fn an_unknown_skin_tone_setting_leaves_emoji_untoned() {
    let mut emoji = Emoji::start(tone("purple"));
    emoji.items();
    assert_eq!(emoji.run("👍", "type"), typed("👍"));
}

#[test]
fn an_action_on_something_unknown_fails() {
    let mut emoji = Emoji::start(Table::new());
    emoji.items();
    assert!(matches!(emoji.run("nope", "type"), Outcome::Failed { .. }));
    assert!(matches!(emoji.run("🔥", "type-dark"), Outcome::Failed { .. }));
    assert!(matches!(emoji.run("👍", "dance"), Outcome::Failed { .. }));
}
