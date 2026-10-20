//! Which character separates a number's decimals, from the user's locale.
//!
//! # Owns
//!
//! - [`DecimalSeparator`] and reading it off a POSIX locale name.
//!
//! # Not here
//!
//! - Reading numbers with it and writing answers in it: `math.rs`.
//! - The `decimal_separator` setting that overrides the locale: `main.rs`.

use std::env;

/// The character between a number's whole part and its decimals. The other
/// of `.` and `,` groups its thousands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DecimalSeparator {
    /// `3.5`, with `1,000` a thousand: English, Chinese, Japanese and others.
    #[default]
    Dot,
    /// `3,5`, with `1.000` a thousand: most of Europe, South America and
    /// others.
    Comma,
}

/// Languages whose locales write decimals with a comma, sorted for a binary
/// search. [`DOT_TERRITORIES`] and [`COMMA_TERRITORIES`] name the exceptions.
/// From glibc's `LC_NUMERIC` data.
const COMMA_LANGUAGES: &[&str] = &[
    "af", "an", "ast", "az", "be", "bg", "br", "bs", "ca", "ce", "crh", "cs", "csb", "cv", "da",
    "de", "el", "eo", "es", "et", "eu", "fi", "fo", "fr", "fur", "fy", "gl", "hr", "hsb", "hu",
    "hy", "id", "is", "it", "ka", "kk", "ky", "lb", "li", "lij", "lt", "lv", "mk", "mn", "nb",
    "nds", "nl", "nn", "no", "oc", "os", "pl", "pt", "ro", "ru", "sc", "se", "sk", "sl", "sq",
    "sr", "sv", "tr", "uk", "uz", "vi",
];

/// Locales of a [`COMMA_LANGUAGES`] language that write decimals with a dot.
const DOT_TERRITORIES: &[&str] = &[
    "de_CH", "de_LI", "es_DO", "es_GT", "es_HN", "es_MX", "es_NI", "es_PA", "es_PR", "es_SV",
    "es_US", "it_CH",
];

/// Locales of another language that write decimals with a comma.
const COMMA_TERRITORIES: &[&str] = &["en_DK"];

impl DecimalSeparator {
    /// The separator a POSIX locale name such as `de_DE.UTF-8` or
    /// `pt_BR@euro` writes decimals with. `C`, `POSIX` and anything unknown
    /// write them with a dot.
    #[must_use]
    pub fn of_locale(locale: &str) -> Self {
        let name = locale.split(['.', '@']).next().unwrap_or_default();
        let language = name.split('_').next().unwrap_or_default();
        let comma = COMMA_TERRITORIES.contains(&name)
            || (COMMA_LANGUAGES.binary_search(&language).is_ok()
                && !DOT_TERRITORIES.contains(&name));
        if comma {
            DecimalSeparator::Comma
        } else {
            DecimalSeparator::Dot
        }
    }

    /// The separator of the locale numbers are written in: `LC_ALL`, else
    /// `LC_NUMERIC`, else `LANG`, the first one set.
    #[must_use]
    pub fn from_env() -> Self {
        let locale = ["LC_ALL", "LC_NUMERIC", "LANG"]
            .into_iter()
            .filter_map(|name| env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        Self::of_locale(&locale)
    }

    /// The setting's value, `"dot"` or `"comma"`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "dot" => Some(DecimalSeparator::Dot),
            "comma" => Some(DecimalSeparator::Comma),
            _ => None,
        }
    }

    pub(crate) fn decimal(self) -> u8 {
        match self {
            DecimalSeparator::Dot => b'.',
            DecimalSeparator::Comma => b',',
        }
    }

    pub(crate) fn thousands(self) -> u8 {
        match self {
            DecimalSeparator::Dot => b',',
            DecimalSeparator::Comma => b'.',
        }
    }

    pub(crate) fn fend(self) -> fend_core::DecimalSeparatorStyle {
        match self {
            DecimalSeparator::Dot => fend_core::DecimalSeparatorStyle::Dot,
            DecimalSeparator::Comma => fend_core::DecimalSeparatorStyle::Comma,
        }
    }
}

#[cfg(test)]
mod tests;
