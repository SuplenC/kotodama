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
