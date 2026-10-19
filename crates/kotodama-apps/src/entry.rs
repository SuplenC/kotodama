//! Reading one `.desktop` file, as the Desktop Entry Specification describes it:
//! the `[Desktop Entry]` group and its `[Desktop Action <id>]` groups, with
//! localized values picked for the user's locale.

/// What a `.desktop` file says, before anything is decided about showing it.
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
            Some(key) => self
                .0
                .iter()
                .position(|ours| ours == key)
                .map(|at| self.0.len() - at),
        }
    }
}

/// A value and how well its locale matched.
#[derive(Default)]
struct Localized(Option<(usize, String)>);

impl Localized {
    fn offer(&mut self, rank: usize, value: &str) {
        if self.0.as_ref().is_none_or(|(best, _)| rank >= *best) {
            self.0 = Some((rank, value.to_owned()));
        }
    }

    fn get(self) -> Option<String> {
        self.0.map(|(_, value)| value)
    }
}

#[derive(Default)]
struct ActionGroup {
    id: String,
    name: Localized,
    icon: Option<String>,
    exec: Option<String>,
}

enum Group {
    Entry,
    Action(usize),
    Other,
}

/// Reads a `.desktop` file. `None` if it has no `[Desktop Entry]` group or no
/// `Name`, which makes it no desktop entry at all.
pub fn parse(text: &str, locale: &Locale) -> Option<Entry> {
    let mut entry = Entry::default();
    let mut seen_entry = false;
    let mut name = Localized::default();
    let mut generic_name = Localized::default();
    let mut comment = Localized::default();
    let mut keywords = Localized::default();
    let mut action_ids = Vec::new();
    let mut groups: Vec<ActionGroup> = Vec::new();
    let mut group = Group::Other;

    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end().strip_suffix(']').unwrap_or(header);
            group = if header == "Desktop Entry" {
                // The first group must be this one; a second copy is ignored.
                if seen_entry {
                    Group::Other
                } else {
                    seen_entry = true;
                    Group::Entry
                }
            } else if let Some(id) = header.strip_prefix("Desktop Action ") {
                groups.push(ActionGroup {
                    id: id.to_owned(),
                    ..ActionGroup::default()
                });
                Group::Action(groups.len() - 1)
            } else {
                Group::Other
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim_end(), value.trim_start());
        let (key, lang) = match key.split_once('[') {
            Some((key, lang)) => (key, Some(lang.strip_suffix(']').unwrap_or(lang))),
            None => (key, None),
        };
        let Some(rank) = locale.rank(lang) else {
            continue;
        };
        match group {
            Group::Entry => match key {
                "Name" => name.offer(rank, &unescape(value)),
                "GenericName" => generic_name.offer(rank, &unescape(value)),
                "Comment" => comment.offer(rank, &unescape(value)),
                "Keywords" => keywords.offer(rank, value),
                _ if lang.is_some() => {}
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
                "Actions" => action_ids = list(value),
                _ => {}
            },
            Group::Action(n) => {
                let action = &mut groups[n];
                match key {
                    "Name" => action.name.offer(rank, &unescape(value)),
                    _ if lang.is_some() => {}
                    "Icon" => action.icon = Some(unescape(value)),
                    "Exec" => action.exec = Some(unescape(value)),
                    _ => {}
                }
            }
            Group::Other => {}
        }
    }
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
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
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
mod tests {
    use super::*;

    fn polish() -> Locale {
        Locale::new("pl_PL.UTF-8")
    }

    #[test]
    fn values_are_read_from_the_desktop_entry_group_only() {
        let entry = parse(
            "# A comment\n[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus\n\
             [Something Else]\nName=Not this\n",
            &Locale::default(),
        )
        .unwrap();
        assert_eq!(entry.name, "Files");
        assert_eq!(entry.kind, "Application");
        assert_eq!(entry.exec.as_deref(), Some("nautilus"));
    }

    #[test]
    fn a_file_without_a_desktop_entry_group_or_a_name_is_no_entry() {
        assert_eq!(parse("[Other]\nName=X\n", &Locale::default()), None);
        assert_eq!(parse("[Desktop Entry]\nExec=x\n", &Locale::default()), None);
    }

    #[test]
    fn localized_values_follow_the_locale_most_specific_first() {
        let text = "[Desktop Entry]\nName=Files\nName[pl]=Pliki\nName[pl_PL]=Pliki PL\n\
                    Name[de]=Dateien\nGenericName=File Manager\nGenericName[pl]=Menedżer plików\n\
                    Keywords=folder;\nKeywords[pl]=katalog;teczka;\n";
        let entry = parse(text, &polish()).unwrap();
        assert_eq!(entry.name, "Pliki PL");
        assert_eq!(entry.generic_name.as_deref(), Some("Menedżer plików"));
        assert_eq!(entry.keywords, ["katalog", "teczka"]);

        let entry = parse(text, &Locale::new("en_US.UTF-8")).unwrap();
        assert_eq!(entry.name, "Files");
        assert_eq!(entry.keywords, ["folder"]);
    }

    #[test]
    fn a_locale_with_a_modifier_prefers_the_modified_translation() {
        let text = "[Desktop Entry]\nName=Name\nName[sr]=Ime\nName[sr@latin]=Ime latinicom\n";
        assert_eq!(
            parse(text, &Locale::new("sr_RS.UTF-8@latin")).unwrap().name,
            "Ime latinicom"
        );
        assert_eq!(parse(text, &Locale::new("C")).unwrap().name, "Name");
    }

    #[test]
    fn strings_and_lists_are_unescaped() {
        let entry = parse(
            "[Desktop Entry]\nName=A\\sB\nComment=line\\none\nKeywords=semi\\;colon;back\\\\slash;;\n\
             OnlyShowIn=GNOME;KDE;\n",
            &Locale::default(),
        )
        .unwrap();
        assert_eq!(entry.name, "A B");
        assert_eq!(entry.comment.as_deref(), Some("line\none"));
        assert_eq!(entry.keywords, ["semi;colon", "back\\slash"]);
        assert_eq!(entry.only_show_in, ["GNOME", "KDE"]);
    }

    #[test]
    fn flags_are_true_only_when_they_say_true() {
        let entry = parse(
            "[Desktop Entry]\nName=A\nTerminal=true\nNoDisplay=false\nHidden=true\n",
            &Locale::default(),
        )
        .unwrap();
        assert!(entry.terminal);
        assert!(!entry.no_display);
        assert!(entry.hidden);
    }

    #[test]
    fn desktop_actions_come_in_the_order_actions_lists_them() {
        let entry = parse(
            "[Desktop Entry]\nName=Firefox\nExec=firefox %u\nActions=private;new-window;missing;\n\
             [Desktop Action new-window]\nName=New Window\nName[pl]=Nowe okno\nExec=firefox --new-window %u\n\
             [Desktop Action private]\nName=New Private Window\nExec=firefox --private-window %u\nIcon=private\n\
             [Desktop Action unlisted]\nName=Unlisted\nExec=firefox\n",
            &polish(),
        )
        .unwrap();
        assert_eq!(
            entry.actions,
            [
                DesktopAction {
                    id: "private".into(),
                    name: "New Private Window".into(),
                    icon: Some("private".into()),
                    exec: "firefox --private-window %u".into(),
                },
                DesktopAction {
                    id: "new-window".into(),
                    name: "Nowe okno".into(),
                    icon: None,
                    exec: "firefox --new-window %u".into(),
                },
            ]
        );
    }
}
