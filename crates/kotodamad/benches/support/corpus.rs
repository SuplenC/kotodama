//! 10k made-up Indexed items, the same on every run: about what an apps,
//! files and projects index holds on a busy desktop.

use kotodama_protocol::Action;
use kotodama_protocol::Item;
use kotodama_protocol::ItemKind;

pub const ITEMS: usize = 10_000;

const SYLLABLES: [&str; 24] = [
    "fi", "re", "fox", "ter", "mi", "nal", "code", "ka", "to", "da", "ma", "set", "tings", "vol", "ume", "blue",
    "tooth", "net", "work", "wall", "pa", "per", "dis", "play",
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
                    let mut word: String = (0..=next() % 3).map(|_| SYLLABLES[next() % SYLLABLES.len()]).collect();
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
                app_only: false,
                actions: vec![Action::new("open", "Open")],
                title,
            }
        })
        .collect()
}

/// `n` made-up clipboard entries, `clip-0` on, whose titles are `chars`
/// characters of words: what the clipboard Extension would send if it didn't
/// cut its searchable text short. `seed` varies the words.
pub fn long_titles(n: usize, chars: usize, seed: u32) -> Vec<Item> {
    let mut seed = seed | 1;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    (0..n)
        .map(|n| {
            let mut title = String::new();
            while title.len() < chars {
                if !title.is_empty() {
                    title.push(' ');
                }
                for _ in 0..=next() % 3 {
                    title.push_str(SYLLABLES[next() % SYLLABLES.len()]);
                }
            }
            title.truncate(chars);
            Item {
                id: format!("clip-{n}"),
                kind: ItemKind::Url {
                    url: format!("https://example.org/{n}"),
                },
                subtitle: Some("Copied 3 minutes ago".into()),
                icon: None,
                keyword: None,
                identity: None,
                keywords: vec![],
                hidden: false,
                app_only: false,
                actions: vec![Action::new("paste", "Paste")],
                title,
            }
        })
        .collect()
}
