use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;

use super::links_to;

#[test]
fn the_links_on_the_way_are_found_where_they_can_be_replaced() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    // ~/.nix-profile -> profiles/profile -> profile-2-link -> the
    // generation, whose `share` links into a store that never changes.
    let generation = root.join("store/generation");
    let package = root.join("store/package");
    fs::create_dir_all(&generation).unwrap();
    fs::create_dir_all(package.join("share/applications")).unwrap();
    symlink(package.join("share"), generation.join("share")).unwrap();
    fs::create_dir(root.join("profiles")).unwrap();
    symlink(&generation, root.join("profiles/profile-2-link")).unwrap();
    symlink("profile-2-link", root.join("profiles/profile")).unwrap();
    symlink("profiles/profile", root.join("nix-profile")).unwrap();
    fs::set_permissions(&generation, fs::Permissions::from_mode(0o555)).unwrap();

    let links = links_to(&root.join("nix-profile/share/applications"));

    fs::set_permissions(&generation, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        links,
        [
            root.join("nix-profile"),
            root.join("profiles/profile"),
            root.join("profiles/profile-2-link"),
        ]
    );
    // As far as the path exists.
    assert_eq!(links_to(&root.join("nix-profile/missing/deeper")).len(), 3);
    assert!(links_to(&root.join("store/package/share")).is_empty());
}

#[test]
fn a_loop_of_links_ends() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    symlink("two", root.join("one")).unwrap();
    symlink("one", root.join("two")).unwrap();
    assert_eq!(links_to(&root.join("one/dir")).len(), 40);
}
