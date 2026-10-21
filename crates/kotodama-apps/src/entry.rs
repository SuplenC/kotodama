//! Reading one `.desktop` file, as the Desktop Entry Specification describes it:
//! the `[Desktop Entry]` group and its `[Desktop Action <id>]` groups, with
//! localized values picked for the user's locale.
//!
//! # Owns
//!
//! - The `.desktop` file format: groups, keys, localized keys, string escapes and lists.
//!
//! # Not here
//!
//! - Whether an entry is shown, and the app it becomes: `scan.rs`.
//! - Splitting `Exec` into arguments: `exec.rs`.

/// A desktop entry, in the specification's words: what a `.desktop` file says,
/// before anything is decided about showing it. It isn't an Item; the scan
/// turns the entries to show into apps, and those become Items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// `Type`; only `Application` entries are apps.
    pub kind: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub keywords: Vec<String>,
    /// `Exec`, with the string escapes undone but still quoted.
    pub exec: Option<String>,
    pub try_exec: Option<String>,
    /// `Path`: the directory to start the app in.
    pub path: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    /// `Hidden=true` means the entry is deleted, along with any of the same id
    /// in a less important directory.
    pub hidden: bool,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
    /// The desktop actions named by `Actions`, in that order, that have a
    /// group with a name and an `Exec`.
    pub actions: Vec<DesktopAction>,
}

/// A `[Desktop Action <id>]` group, such as Firefox's "New Private Window".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopAction {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub exec: String,
}

/// The locale keys a localized value is looked up by, best first: for
/// `pl_PL.UTF-8@euro`, `pl_PL@euro`, `pl_PL`, `pl@euro` and `pl`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Locale(Vec<String>);

impl Locale {
    /// From a POSIX locale name such as `LC_MESSAGES` holds. `C` and `POSIX`
    /// have no translations.
    #[must_use]
    pub fn new(name: &str) -> Self {
        if name.is_empty() || name == "C" || name == "POSIX" || name.starts_with("C.") {
            return Locale::default();
        }
        let (rest, modifier) = match name.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (name, None),
        };
        let rest = rest.split_once('.').map_or(rest, |(rest, _encoding)| rest);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        let mut keys = Vec::new();
        if let (Some(country), Some(modifier)) = (country, modifier) {
            keys.push(format!("{lang}_{country}@{modifier}"));
        }
        if let Some(country) = country {
            keys.push(format!("{lang}_{country}"));
        }
        if let Some(modifier) = modifier {
            keys.push(format!("{lang}@{modifier}"));
        }
        keys.push(lang.to_owned());
        Locale(keys)
    }

    /// How good a match `key` is: 0 for the unlocalized value, higher for
    /// better ones, `None` for another locale's.
    fn rank(&self, key: Option<&str>) -> Option<usize> {
        match key {
            None => Some(0),
            Some(key) => self.0.iter().position(|ours| ours == key).map(|at| self.0.len() - at),
        }
    }
}

/// A value and how well its locale matched.
#[derive(Default)]
struct Localized(Option<(usize, String)>);

impl Localized {
    /// Keeps `value` unless a better match is already kept. Of two equal ones,
    /// the later line wins.
    fn offer(&mut self, rank: usize, value: &str) {
        if self.0.as_ref().is_none_or(|(best, _)| rank >= *best) {
            self.0 = Some((rank, value.to_owned()));
        }
    }

    fn get(self) -> Option<String> {
        self.0.map(|(_, value)| value)
    }
}

/// A `[Desktop Action <id>]` group as it is read, before `Actions` decides
/// whether it is one of the entry's.
#[derive(Default)]
struct ActionGroup {
    id: String,
    name: Localized,
    icon: Option<String>,
    exec: Option<String>,
}

impl ActionGroup {
    /// Takes one `key=value` line of the group. A localized key other than
    /// `Name` is not the specification's, so it is skipped.
    fn set(&mut self, key: &str, localized: bool, rank: usize, value: &str) {
        match key {
            "Name" => self.name.offer(rank, &unescape(value)),
            _ if localized => {}
            "Icon" => self.icon = Some(unescape(value)),
            "Exec" => self.exec = Some(unescape(value)),
            _ => {}
        }
    }
}

/// The group the lines being read belong to.
#[derive(Clone, Copy, Default)]
enum Group {
    Entry,
    /// The action group at this index in [`Parser::groups`].
    Action(usize),
    #[default]
    Other,
}

/// What [`parse`] has read so far.
#[derive(Default)]
struct Parser {
    entry: Entry,
    seen_entry: bool,
    name: Localized,
    generic_name: Localized,
    comment: Localized,
    keywords: Localized,
    /// The ids `Actions` lists, in its order.
    action_ids: Vec<String>,
    /// Every action group, in the file's order, listed or not.
    groups: Vec<ActionGroup>,
    group: Group,
}

/// Reads a `.desktop` file. `None` if it has no `[Desktop Entry]` group or no
/// `Name`, which makes it no desktop entry at all.
pub fn parse(text: &str, locale: &Locale) -> Option<Entry> {
    let mut parser = Parser::default();
    for line in text.lines() {
        parser.line(line, locale);
    }
    parser.finish()
}

impl Parser {
    fn line(&mut self, line: &str, locale: &Locale) {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        if let Some(header) = line.strip_prefix('[') {
            self.group = self.header(header);
            return;
        }
        let Some((key, value)) = line.split_once('=') else {
            return;
        };
        let (key, value) = (key.trim_end(), value.trim_start());
        let (key, lang) = match key.split_once('[') {
            Some((key, lang)) => (key, Some(lang.strip_suffix(']').unwrap_or(lang))),
            None => (key, None),
        };
        let Some(rank) = locale.rank(lang) else {
            return;
        };
        match self.group {
            Group::Entry => self.set(key, lang.is_some(), rank, value),
            Group::Action(n) => self.groups[n].set(key, lang.is_some(), rank, value),
            Group::Other => {}
        }
    }

    /// The group a `[header]` line opens, given the line after its `[`.
    fn header(&mut self, header: &str) -> Group {
        let header = header.trim_end().strip_suffix(']').unwrap_or(header);
        if header == "Desktop Entry" {
            // The first group must be this one; a second copy is ignored.
            if self.seen_entry {
                return Group::Other;
            }
            self.seen_entry = true;
            return Group::Entry;
        }
        let Some(id) = header.strip_prefix("Desktop Action ") else {
            return Group::Other;
        };
        self.groups.push(ActionGroup {
            id: id.to_owned(),
            ..ActionGroup::default()
        });
        Group::Action(self.groups.len() - 1)
    }

    /// Takes one `key=value` line of the `[Desktop Entry]` group. A localized
    /// key other than the four localestrings is not the specification's, so
    /// it is skipped.
    fn set(&mut self, key: &str, localized: bool, rank: usize, value: &str) {
        let entry = &mut self.entry;
        match key {
            "Name" => self.name.offer(rank, &unescape(value)),
            "GenericName" => self.generic_name.offer(rank, &unescape(value)),
            "Comment" => self.comment.offer(rank, &unescape(value)),
            "Keywords" => self.keywords.offer(rank, value),
            _ if localized => {}
            "Type" => entry.kind = unescape(value),
            "Icon" => entry.icon = Some(unescape(value)),
            "Exec" => entry.exec = Some(unescape(value)),
            "TryExec" => entry.try_exec = Some(unescape(value)),
            "Path" => entry.path = Some(unescape(value)),
            "Terminal" => entry.terminal = value == "true",
            "NoDisplay" => entry.no_display = value == "true",
            "Hidden" => entry.hidden = value == "true",
            "OnlyShowIn" => entry.only_show_in = list(value),
            "NotShowIn" => entry.not_show_in = list(value),
            "Actions" => self.action_ids = list(value),
            _ => {}
        }
    }

    /// The entry, with the best translation of each localized value and the
    /// listed actions that have a name and an `Exec`.
    fn finish(self) -> Option<Entry> {
        let Parser {
            mut entry,
            seen_entry,
            name,
            generic_name,
            comment,
            keywords,
            action_ids,
            mut groups,
            group: _,
        } = self;
        if !seen_entry {
            return None;
        }
        entry.name = name.get().filter(|name| !name.is_empty())?;
        entry.generic_name = generic_name.get().filter(|text| !text.is_empty());
        entry.comment = comment.get().filter(|text| !text.is_empty());
        entry.keywords = keywords.get().as_deref().map(list).unwrap_or_default();
        entry.icon = entry.icon.filter(|icon| !icon.is_empty());
        for id in action_ids {
            let Some(n) = groups.iter().position(|group| group.id == id) else {
                continue;
            };
            let group = std::mem::take(&mut groups[n]);
            if let (Some(name), Some(exec)) = (group.name.get(), group.exec) {
                entry.actions.push(DesktopAction {
                    id,
                    name,
                    icon: group.icon.filter(|icon| !icon.is_empty()),
                    exec,
                });
            }
        }
        Some(entry)
    }
}

/// Undoes the string escapes: `\s`, `\n`, `\t`, `\r` and `\\`. Any other
/// backslash is kept as it is.
fn unescape(value: &str) -> String {
    if !value.contains('\\') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// A list value: separated by `;`, with `\;` for a semicolon inside an
/// element. Empty elements are dropped.
fn list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(';') => item.push(';'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
                None => item.push('\\'),
            },
            ';' => items.push(unescape(&std::mem::take(&mut item))),
            c => item.push(c),
        }
    }
    items.push(unescape(&item));
    items.retain(|item| !item.trim().is_empty());
    items
}

#[cfg(test)]
mod tests;
