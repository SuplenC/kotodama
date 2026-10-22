//! Writing Lua for Hyprland's Lua config, which runs a `dispatch` request as
//! `hl.dispatch(…)` around its argument.
//!
//! # Owns
//!
//! - Quoting text as a Lua string literal, so text from the user or from Hyprland can't end the
//!   literal early.
//!
//! # Not here
//!
//! - Which dispatch to write: [`crate::windows::dispatch`] and [`crate::binds::Bind::dispatch`].

use std::fmt::Write;

/// `text` as a Lua string literal, in double quotes. A quote and a backslash
/// are escaped, and so is every control character, as a three-digit decimal
/// escape: a shorter one followed by a digit would read that digit too.
pub fn string(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('"');
    for c in text.chars() {
        match c {
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            c if c.is_ascii_control() => {
                write!(literal, "\\{:03}", u32::from(c)).expect("writing to a String can't fail");
            }
            c => literal.push(c),
        }
    }
    literal.push('"');
    literal
}

#[cfg(test)]
mod tests;
