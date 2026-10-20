//! Which kinds of Result come first when two match a query equally well: apps
//! before projects, projects before bookmarks, and bookmarks before files.

use kotodama_protocol::{ItemKind, KindName, SHARED_KINDS, Table, Value};

/// Stands for every kind the order doesn't name.
const OTHERS: &str = "*";

/// The order of Item kinds, from the config's `[ranking]` table:
///
/// ```toml
/// [ranking]
/// # On an equal match, kinds earlier in the list rank first. `*` stands for
/// # every kind not in it; without it, those rank last.
/// kinds = ["app", "project", "url", "*", "file"]
/// ```
///
/// A setting that doesn't parse is logged and keeps its default; a kind in the
/// list that isn't one is logged and left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindOrder {
    /// The place of each shared kind, in [`SHARED_KINDS`] order, lowest first.
    shared: [u8; SHARED_KINDS.len()],
    /// The namespaced kinds the order names, with their places.
    namespaced: Vec<(String, u8)>,
    /// The place of every other kind.
    others: u8,
}

impl Default for KindOrder {
    fn default() -> Self {
        KindOrder::new(&["app", "project", "url", OTHERS, "file"])
    }
}

impl KindOrder {
    /// The order of `kinds`, each a kind's name or [`OTHERS`]. A kind named
    /// twice keeps its first place.
    fn new(kinds: &[&str]) -> Self {
        let kinds = &kinds[..kinds.len().min(usize::from(u8::MAX))];
        let others = kinds
            .iter()
            .position(|&kind| kind == OTHERS)
            .unwrap_or(kinds.len()) as u8;
        let mut order = KindOrder {
            shared: [others; SHARED_KINDS.len()],
            namespaced: Vec::new(),
            others,
        };
        let mut named: Vec<&str> = Vec::new();
        for (place, &kind) in kinds.iter().enumerate() {
            if kind == OTHERS || named.contains(&kind) {
                continue;
            }
            named.push(kind);
            match SHARED_KINDS.iter().position(|&shared| shared == kind) {
                Some(n) => order.shared[n] = place as u8,
                None => order.namespaced.push((kind.to_owned(), place as u8)),
            }
        }
        order
    }

    /// The `[ranking]` table of the merged config.
    pub fn from_config(config: &Table) -> Self {
        let mut order = KindOrder::default();
        let table = match config.get("ranking") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                log::error!("`ranking` should be a table");
                return order;
            }
            None => return order,
        };
        for (key, value) in table {
            let set = match key.as_str() {
                "kinds" => match value {
                    Value::Array(kinds) => {
                        let mut names = Vec::new();
                        for (n, kind) in kinds.iter().enumerate() {
                            match kind_name(kind) {
                                Ok(name) => names.push(name),
                                Err(err) => log::error!("ranking.kinds[{n}]: {err}"),
                            }
                        }
                        order = KindOrder::new(&names);
                        Ok(())
                    }
                    _ => Err("should be a list of Item kinds".to_owned()),
                },
                _ => Err("isn't a setting".to_owned()),
            };
            if let Err(err) = set {
                log::error!("ranking.{key}: {err}");
            }
        }
        order
    }

    /// Where Results of `kind` rank on an equal match, lowest first.
    pub fn place(&self, kind: &ItemKind) -> u8 {
        match kind {
            ItemKind::Namespaced(kind) => self
                .namespaced
                .iter()
                .find(|(name, _)| name == kind.as_str())
                .map_or(self.others, |&(_, place)| place),
            shared => self.shared[shared_index(shared)],
        }
    }
}

/// A shared kind's index in [`SHARED_KINDS`], without comparing names.
fn shared_index(kind: &ItemKind) -> usize {
    match kind {
        ItemKind::App { .. } => 0,
        ItemKind::File { .. } => 1,
        ItemKind::Folder { .. } => 2,
        ItemKind::Project { .. } => 3,
        ItemKind::Window { .. } => 4,
        ItemKind::Url { .. } => 5,
        ItemKind::Namespaced(_) => unreachable!("a namespaced kind isn't shared"),
    }
}

fn kind_name(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(kind) if kind == OTHERS => Ok(kind),
        Value::String(kind) => KindName::new(kind.as_str()).map(|_| kind.as_str()),
        _ => Err("should be the name of an Item kind, or `*`".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_kinds_index_is_its_place_among_the_shared_kinds() {
        let kinds = [
            ItemKind::App {
                desktop_id: "x".into(),
            },
            ItemKind::File { path: "/x".into() },
            ItemKind::Folder { path: "/x".into() },
            ItemKind::Project { path: "/x".into() },
            ItemKind::Window {
                address: "x".into(),
            },
            ItemKind::Url { url: "x".into() },
        ];
        assert_eq!(kinds.len(), SHARED_KINDS.len());
        for kind in &kinds {
            assert_eq!(SHARED_KINDS[shared_index(kind)], kind.name());
        }
    }
}
