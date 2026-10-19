//! The Results as the list model holds them, and how a new list replaces the
//! old one without resetting the model. Nothing here touches Qt, so it is
//! tested without it.

use std::fmt::Write;
use std::ops::Range;

use kotodama_protocol::{ActionRef, ResultRef, ShownResult};

/// One row of the list model.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub result: ResultRef,
    /// What Enter runs. Every Item has at least one Action, but a row is never
    /// trusted to.
    pub primary: Option<ActionRef>,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    /// Positions, in characters, of the title's matched characters, in order.
    pub highlights: Vec<u32>,
}

impl From<ShownResult> for Row {
    fn from(shown: ShownResult) -> Self {
        let mut item = shown.item;
        let primary = item.actions.first().map(|action| ActionRef {
            extension: shown.extension.clone(),
            id: action.id.clone(),
        });
        let mut highlights = shown.highlights;
        highlights.sort_unstable();
        highlights.dedup();
        Row {
            result: ResultRef {
                extension: shown.extension,
                id: std::mem::take(&mut item.id),
            },
            primary,
            title: item.title,
            subtitle: item.subtitle.unwrap_or_default(),
            icon: item.icon.unwrap_or_default(),
            highlights,
        }
    }
}

impl Row {
    /// The title as Qt's StyledText, with each run of matched characters set
    /// in `color` (any color QML accepts, such as `#89b4fa`).
    pub fn markup(&self, color: &str) -> String {
        let mut out = String::with_capacity(self.title.len() + 32 * self.highlights.len());
        let mut wanted = self.highlights.iter().copied().peekable();
        let mut open = false;
        for (n, ch) in self.title.chars().enumerate() {
            let matched = wanted.next_if_eq(&(n as u32)).is_some();
            if matched != open {
                if matched {
                    let _ = write!(out, "<font color=\"{}\">", Escaped(color));
                } else {
                    out.push_str("</font>");
                }
                open = matched;
            }
            let _ = write!(out, "{}", Escaped(ch.encode_utf8(&mut [0; 4])));
        }
        if open {
            out.push_str("</font>");
        }
        out
    }
}

/// Text made safe to put in StyledText.
struct Escaped<'a>(&'a str);

impl std::fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                _ => f.write_char(ch)?,
            }
        }
        Ok(())
    }
}

/// How the model goes from `old` rows to `new` ones: the rows both have are
/// changed in place, so the list keeps its delegates and its current row, and
/// only the difference in length is inserted or removed.
#[derive(Debug, PartialEq, Eq)]
pub struct Replace {
    pub changed: Range<usize>,
    pub inserted: Range<usize>,
    pub removed: Range<usize>,
}

impl Replace {
    pub fn new(old: usize, new: usize) -> Self {
        let kept = old.min(new);
        Replace {
            changed: 0..kept,
            inserted: kept..new.max(kept),
            removed: kept..old.max(kept),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, highlights: &[u32]) -> Row {
        Row {
            result: ResultRef {
                extension: "apps".into(),
                id: "x".into(),
            },
            primary: None,
            title: title.into(),
            subtitle: String::new(),
            icon: String::new(),
            highlights: highlights.to_vec(),
        }
    }

    #[test]
    fn runs_of_matched_characters_share_one_tag() {
        assert_eq!(
            row("Firefox", &[0, 1, 4]).markup("red"),
            "<font color=\"red\">Fi</font>re<font color=\"red\">f</font>ox"
        );
        assert_eq!(
            row("ab", &[0, 1]).markup("red"),
            "<font color=\"red\">ab</font>"
        );
        assert_eq!(row("ab", &[]).markup("red"), "ab");
    }

    #[test]
    fn highlights_count_characters_not_bytes() {
        assert_eq!(
            row("Żółw", &[1, 3]).markup("red"),
            "Ż<font color=\"red\">ó</font>ł<font color=\"red\">w</font>"
        );
    }

    #[test]
    fn the_title_and_the_color_are_escaped() {
        assert_eq!(
            row("<b>&", &[0]).markup("\"x"),
            "<font color=\"&quot;x\">&lt;</font>b&gt;&amp;"
        );
    }

    #[test]
    fn highlights_past_the_title_are_ignored() {
        assert_eq!(
            row("ab", &[1, 7]).markup("red"),
            "a<font color=\"red\">b</font>"
        );
    }

    #[test]
    fn a_new_list_changes_the_rows_both_have_and_inserts_or_removes_the_rest() {
        assert_eq!(
            Replace::new(3, 5),
            Replace {
                changed: 0..3,
                inserted: 3..5,
                removed: 3..3,
            }
        );
        assert_eq!(
            Replace::new(5, 2),
            Replace {
                changed: 0..2,
                inserted: 2..2,
                removed: 2..5,
            }
        );
        assert_eq!(
            Replace::new(0, 0),
            Replace {
                changed: 0..0,
                inserted: 0..0,
                removed: 0..0,
            }
        );
    }
}
