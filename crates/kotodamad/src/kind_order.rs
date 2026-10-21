//! Which kinds of Result come first when two match a query equally well: apps
//! before projects, projects before bookmarks, and bookmarks before files.

use kotodama_protocol::{ItemKind, KindName, SHARED_KINDS, Table, Value};

use crate::problem::Report;

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
/// A setting that doesn't parse is a problem, and keeps its default; a kind
/// in the list that isn't one is a problem, and left out.
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
    /// twice keeps its first place. Places are counted in a byte, so kinds
    /// past the 255th are left out.
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
    pub fn from_config(config: &Table, report: &mut Report) -> Self {
        let mut order = KindOrder::default();
        let table = match config.get("ranking") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                report.add("ranking", "should be a table");
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
                                Err(err) => report.add(format!("ranking.kinds[{n}]"), err),
                            }
                        }
                        let kept = usize::from(u8::MAX);
                        if names.len() > kept {
                            report.add(
                                "ranking.kinds",
                                format!(
                                    "names {} kinds, more than {kept}; the rest are left out",
                                    names.len()
                                ),
                            );
                        }
                        order = KindOrder::new(&names);
                        Ok(())
                    }
                    _ => Err("should be a list of Item kinds".to_owned()),
                },
                _ => Err("isn't a setting".to_owned()),
            };
            if let Err(err) = set {
                report.add(format!("ranking.{key}"), err);
            }
        }
        order
    }

    /// Where Results of the kind named `name` rank on an equal match, lowest
    /// first.
    pub fn place_named(&self, name: &str) -> u8 {
        match SHARED_KINDS.iter().position(|&shared| shared == name) {
            Some(n) => self.shared[n],
            None => self
                .namespaced
                .iter()
                .find(|(namespaced, _)| namespaced == name)
                .map_or(self.others, |&(_, place)| place),
        }
    }

    /// Where Results of `kind` rank on an equal match, lowest first.
    pub fn place(&self, kind: &ItemKind) -> u8 {
        match kind.shared_index() {
            Some(n) => self.shared[n],
            None => self
                .namespaced
                .iter()
                .find(|(name, _)| name == kind.name())
                .map_or(self.others, |&(_, place)| place),
        }
    }
}

/// The place of each kind every Segment of the Index names, by Segment and
/// kind (see [`Segment::kinds`](crate::segment::Segment::kinds)). Worked out
/// per search from the few kinds there are. Ranking then looks up each
/// Result's place, and doesn't compare kind names.
#[derive(Default)]
pub struct KindPlaces {
    places: Vec<Vec<u8>>,
    others: u8,
}

impl KindPlaces {
    /// The places of `segments`' kinds, each Segment's kind names in Segment
    /// order, in `order`.
    pub fn update<'a>(&mut self, order: &KindOrder, segments: impl Iterator<Item = &'a [String]>) {
        self.others = order.others;
        let mut n = 0;
        for kinds in segments {
            if n == self.places.len() {
                self.places.push(Vec::new());
            }
            let places = &mut self.places[n];
            places.clear();
            places.extend(kinds.iter().map(|kind| order.place_named(kind)));
            n += 1;
        }
        self.places.truncate(n);
    }

    /// Where Results of `kind` in `segment` rank on an equal match, lowest
    /// first.
    #[inline]
    pub fn place(&self, segment: u32, kind: u16) -> u8 {
        self.places
            .get(segment as usize)
            .and_then(|places| places.get(usize::from(kind)))
            .copied()
            .unwrap_or(self.others)
    }
}

fn kind_name(value: &Value) -> Result<&str, String> {
    match value {
        Value::String(kind) if kind == OTHERS => Ok(kind),
        Value::String(kind) => KindName::new(kind.as_str()).map(|_| kind.as_str()),
        _ => Err("should be the name of an Item kind, or `*`".to_owned()),
    }
}
