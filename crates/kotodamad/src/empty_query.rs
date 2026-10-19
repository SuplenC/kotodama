//! What the menu shows before anything is typed: the Results most used at this
//! time of day, then a fixed menu, from the config's `[empty_query]` table:
//!
//! ```toml
//! [empty_query]
//! most_used = 3  # how many of the Results most used at this time of day come first
//! # Then these, in this order, each unless it is shown already.
//! menu = [
//!     { extension = "apps", item = "firefox" },
//!     { extension = "kotodama", item = "clear-history" },
//! ]
//! ```
//!
//! A setting that doesn't parse is logged and keeps its default; a menu entry
//! that doesn't is logged and skipped.

use kotodama_protocol::{Table, Value};

use crate::keywords::string;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyQuery {
    /// How many of the Results most used at this time of day come first.
    pub most_used: usize,
    /// The Items shown after them, by the name of the Extension that owns each
    /// and its id.
    pub menu: Vec<(String, String)>,
}

impl Default for EmptyQuery {
    fn default() -> Self {
        EmptyQuery {
            most_used: 3,
            menu: Vec::new(),
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
            match (key.as_str(), value) {
                ("most_used", Value::Integer(n)) if (0..=50).contains(n) => {
                    empty.most_used = *n as usize;
                }
                ("most_used", _) => {
                    log::error!("empty_query.most_used: should be a number from 0 to 50");
                }
                ("menu", Value::Array(entries)) => {
                    for (n, entry) in entries.iter().enumerate() {
                        match menu_entry(entry) {
                            Ok(entry) => empty.menu.push(entry),
                            Err(err) => log::error!("empty_query.menu[{n}]: {err}"),
                        }
                    }
                }
                ("menu", _) => log::error!("empty_query.menu: should be a list"),
                _ => log::error!("empty_query.{key}: isn't a setting"),
            }
        }
        empty
    }
}

fn menu_entry(value: &Value) -> Result<(String, String), String> {
    let Value::Table(table) = value else {
        return Err("should be a table with `extension` and `item`".into());
    };
    Ok((
        string(table, "extension")?.ok_or("needs `extension`")?,
        string(table, "item")?.ok_or("needs `item`")?,
    ))
}
