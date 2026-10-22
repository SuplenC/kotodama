use super::Hyprland;
use super::Language;

#[test]
fn the_language_is_the_config_provider_status_names() {
    let lua = include_str!("../../tests/fixtures/status-lua.json");
    let hyprlang = include_str!("../../tests/fixtures/status-hyprlang.json");

    assert_eq!(Language::from_status(lua), Language::Lua);
    assert_eq!(Language::from_status(hyprlang), Language::Hyprlang);
}

#[test]
fn a_hyprland_without_status_is_hyprlang() {
    // What a Hyprland older than 0.56 answers a request it doesn't know.
    assert_eq!(Language::from_status("unknown request"), Language::Hyprlang);
    assert_eq!(Language::from_status(""), Language::Hyprlang);
}

#[test]
fn the_wayland_socket_is_the_one_the_lock_file_names() {
    let runtime = tempfile::tempdir().unwrap();
    let dir = runtime.path().join("hypr").join("sig_1729000000_1");
    std::fs::create_dir_all(&dir).unwrap();
    let hyprland = Hyprland { dir: dir.clone() };
    assert_eq!(hyprland.wayland_socket(), None);

    std::fs::write(dir.join("hyprland.lock"), "1234\nwayland-1\n").unwrap();

    assert_eq!(hyprland.wayland_socket(), Some(runtime.path().join("wayland-1")));
}
