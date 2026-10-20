//! 10k made-up Indexed items, the same on every run: about what an apps,
//! files and projects index holds on a busy desktop.

use kotodama_protocol::{Action, Item, ItemKind};

pub const ITEMS: usize = 10_000;

const SYLLABLES: [&str; 24] = [
    "fi", "re", "fox", "ter", "mi", "nal", "code", "ka", "to", "da", "ma", "set", "tings", "vol",
    "ume", "blue", "tooth", "net", "work", "wall", "pa", "per", "dis", "play",
];

pub fn items() -> Vec<Item> {
    let mut seed: u32 = 0x9e37_79b9;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    (0..ITEMS)
        .map(|n| {
            let words = 1 + next() % 3;
            let title = (0..words)
                .map(|_| {
                    let mut word: String = (0..1 + next() % 3)
                        .map(|_| SYLLABLES[next() % SYLLABLES.len()])
                        .collect();
                    word[..1].make_ascii_uppercase();
                    word
                })
                .collect::<Vec<_>>()
                .join(" ");
            Item {
                id: format!("item-{n}"),
                kind: ItemKind::App {
                    desktop_id: format!("org.example.App{n}.desktop"),
                },
                subtitle: Some("A typical application with a one-line description".into()),
                icon: Some(format!("org.example.App{n}")),
                keyword: None,
                identity: None,
                keywords: vec![format!("app{n}")],
                hidden: false,
                actions: vec![Action {
                    id: "open".into(),
                    title: "Open".into(),
                    shortcut: None,
                    arguments: vec![],
                }],
                title,
            }
        })
        .collect()
}
