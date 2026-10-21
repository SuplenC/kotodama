//! The Empty query: the Results most used at this time of day, then the Fixed
//! menu, or without one the apps.

use kotodama_protocol::{Table, Value};

use crate::history::ResultKey;
use crate::keywords::string;

/// What the Empty query shows, from the config's `[empty_query]` table:
///
/// ```toml
/// [empty_query]
/// most_used = 3  # how many of the Results most used at this time of day come first
/// # Then these, in this order, each unless it is shown already. Without
/// # `menu`, the apps' entry Items; `menu = []` shows nothing more.
/// menu = [
///     { extension = "apps", item = "firefox" },
///     { extension = "kotodama", item = "clear-history" },
/// ]
/// ```
///
/// A setting that doesn't parse is logged and keeps its default; an Item of
/// the Fixed menu that doesn't is logged and left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyQuery {
    /// How many of the Results most used at this time of day come first.
    pub most_used: usize,
    /// The Fixed menu: the Items shown after them, by the name of the Extension that owns each
    /// and its id. Unset, the apps' entry Items are shown instead, so a first run, with
    /// nothing used yet, shows what there is to enter.
    pub menu: Option<Vec<ResultKey>>,
}

impl Default for EmptyQuery {
    fn default() -> Self {
        EmptyQuery {
            most_used: 3,
            menu: None,
        }
    }
}

impl EmptyQuery {
    /// The `[empty_query]` table of the merged config.
    pub fn from_config(config: &Table) -> Self {
        let mut empty = EmptyQuery::default();
        let table = match config.get("empty_query") {
            Some(Value::Table(table)) => table,
            Some(_) => {
                log::error!("`empty_query` should be a table");
                return empty;
            }
            None => return empty,
        };
        for (key, value) in table {
            let set = match key.as_str() {
                "most_used" => match value {
                    Value::Integer(n) if (0..=50).contains(n) => {
                        empty.most_used = *n as usize;
                        Ok(())
                    }
                    _ => Err("should be a number from 0 to 50".to_owned()),
                },
                "menu" => match value {
                    Value::Array(items) => {
                        let menu = empty.menu.insert(Vec::new());
                        for (n, item) in items.iter().enumerate() {
                            match menu_item(item) {
                                Ok(item) => menu.push(item),
                                Err(err) => log::error!("empty_query.menu[{n}]: {err}"),
                            }
                        }
                        Ok(())
                    }
                    _ => Err("should be a list".to_owned()),
                },
                _ => Err("isn't a setting".to_owned()),
            };
            if let Err(err) = set {
                log::error!("empty_query.{key}: {err}");
            }
        }
        empty
    }
}

fn menu_item(value: &Value) -> Result<ResultKey, String> {
    let Value::Table(table) = value else {
        return Err("should be a table with `extension` and `item`".into());
    };
    Ok((
        string(table, "extension")?.ok_or("needs `extension`")?,
        string(table, "item")?.ok_or("needs `item`")?,
    ))
}
