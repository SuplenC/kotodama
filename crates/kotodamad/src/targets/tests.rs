use super::*;

const PACKAGES: ExtensionId = ExtensionId(0);
const FLATPAK: ExtensionId = ExtensionId(1);

fn targets() -> Targets {
    let mut targets = Targets::default();
    targets.set_offered([
        (PACKAGES, Action::new("remove", "Remove")),
        (PACKAGES, Action::new("update", "Update")),
        (FLATPAK, Action::new("remove", "Uninstall")),
    ]);
    targets
}

fn on<'a>(targets: &'a Targets, identity: &str) -> Vec<(ExtensionId, &'a str)> {
    targets
        .on(identity)
        .map(|(extension, action)| (extension, action.title.as_str()))
        .collect()
}

fn names(identities: &[&str]) -> Vec<String> {
    identities.iter().map(|&identity| identity.to_owned()).collect()
}

#[test]
fn an_identity_is_offered_the_actions_that_name_it_in_extension_order() {
    let mut targets = targets();

    targets.target(FLATPAK, "remove", names(&["app:gimp"])).unwrap();
    targets
        .target(PACKAGES, "update", names(&["app:gimp", "app:vlc"]))
        .unwrap();
    targets.target(PACKAGES, "remove", names(&["app:gimp"])).unwrap();

    assert_eq!(
        on(&targets, "app:gimp"),
        [(PACKAGES, "Remove"), (PACKAGES, "Update"), (FLATPAK, "Uninstall")]
    );
    assert_eq!(on(&targets, "app:vlc"), [(PACKAGES, "Update")]);
    assert_eq!(on(&targets, "app:firefox"), []);
}

#[test]
fn naming_identities_again_replaces_those_named_before_and_returns_both() {
    let mut targets = targets();
    targets
        .target(PACKAGES, "remove", names(&["app:gimp", "app:vlc"]))
        .unwrap();

    let mut changed = targets
        .target(PACKAGES, "remove", names(&["app:vlc", "app:mpv"]))
        .unwrap();

    changed.sort_unstable();
    assert_eq!(changed, [Arc::from("app:gimp"), Arc::from("app:mpv")]);
    assert_eq!(on(&targets, "app:gimp"), []);
    assert_eq!(on(&targets, "app:mpv"), [(PACKAGES, "Remove")]);

    targets.target(PACKAGES, "remove", Vec::new()).unwrap();
    assert!(targets.is_empty());
}

#[test]
fn an_action_not_declared_targeted_names_nothing() {
    let mut targets = targets();

    assert_eq!(
        targets.target(FLATPAK, "update", names(&["app:gimp"])),
        Err(NotTargeted::action("update"))
    );
    assert!(targets.is_empty());
}

#[test]
fn the_identities_an_extension_named_go_when_its_run_ends_and_stay_while_others_start() {
    let mut targets = targets();
    targets.target(PACKAGES, "remove", names(&["app:gimp"])).unwrap();
    targets.target(FLATPAK, "remove", names(&["app:gimp"])).unwrap();

    targets.set_offered([
        (PACKAGES, Action::new("remove", "Remove")),
        (FLATPAK, Action::new("remove", "Uninstall")),
        (ExtensionId(2), Action::new("pin", "Pin")),
    ]);
    assert_eq!(on(&targets, "app:gimp"), [(PACKAGES, "Remove"), (FLATPAK, "Uninstall")]);

    targets.forget(PACKAGES);
    assert_eq!(on(&targets, "app:gimp"), [(FLATPAK, "Uninstall")]);
}
