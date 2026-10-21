use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;

fn table(pairs: &[(&str, Value)]) -> Table {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>()
}

fn string(text: &str) -> Value {
    Value::String(text.into())
}

#[test]
fn folders_are_read_with_their_kind_and_depth_and_a_wrong_one_is_left_out() {
    let config = table(&[(
        "folders",
        Value::Array(vec![
            Value::Table(table(&[
                ("path", string("~/Projects")),
                ("kind", string("projects")),
                ("depth", Value::Integer(1)),
            ])),
            Value::Table(table(&[("path", string("/srv/code")), ("kind", string("projects"))])),
            Value::Table(table(&[("path", string("~/Scripts")), ("kind", string("scripts"))])),
            Value::Table(table(&[("path", string("relative")), ("kind", string("scripts"))])),
            Value::Table(table(&[("path", string("/x")), ("kind", string("music"))])),
        ]),
    )]);
    let settings = Settings::from_config(&config, Some(Path::new("/home/me")), None);
    assert_eq!(
        settings.folders.projects,
        [
            ProjectFolder {
                path: "/home/me/Projects".into(),
                depth: 1
            },
            ProjectFolder {
                path: "/srv/code".into(),
                depth: DEFAULT_DEPTH
            },
        ]
    );
    assert_eq!(settings.folders.scripts, [PathBuf::from("/home/me/Scripts")]);
}

#[test]
fn a_command_of_the_wrong_shape_keeps_its_default() {
    let config = table(&[
        ("open", string("nvim")),
        ("terminal", Value::Array(vec![string("foot")])),
    ]);
    let settings = Settings::from_config(&config, None, None);
    assert_eq!(settings.open, Settings::default().open);
    assert_eq!(settings.terminal, ["foot"]);
}

#[test]
fn the_defaults_open_the_preferred_terminal_through_xdg_terminal_exec() {
    let settings = Settings::from_config(&Table::new(), None, None);

    assert_eq!(settings.open, ["xdg-terminal-exec", "--dir={path}"]);
    assert_eq!(settings.shell, ["xdg-terminal-exec", "--dir={path}"]);
    assert_eq!(settings.terminal[..3], ["xdg-terminal-exec", "sh", "-c"]);
    assert_eq!(settings.file_manager, ["xdg-open", "{path}"]);
    assert_eq!(settings.copy_path, ["wl-copy", "{path}"]);
}

/// A folder for `PATH`, with an executable `lazygit` in it when `installed`.
fn path_with_lazygit(installed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    if installed {
        let lazygit = dir.path().join("lazygit");
        fs::write(&lazygit, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&lazygit, fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

#[test]
fn lazygit_is_offered_by_default_only_when_it_is_on_the_path() {
    let with = path_with_lazygit(true);
    let without = path_with_lazygit(false);

    let found = Settings::from_config(&Table::new(), None, Some(with.path().as_os_str()));
    let missing = Settings::from_config(&Table::new(), None, Some(without.path().as_os_str()));

    assert_eq!(
        found.lazygit.as_deref(),
        Some(&words(&["xdg-terminal-exec", "lazygit", "--path", "{path}"])[..])
    );
    assert_eq!(missing.lazygit, None);
    assert_eq!(Settings::from_config(&Table::new(), None, None).lazygit, None);
}

#[test]
fn a_lazygit_command_in_the_config_is_offered_without_lazygit_on_the_path() {
    let config = table(&[("lazygit", Value::Array(vec![string("gitui")]))]);

    let settings = Settings::from_config(&config, None, None);

    assert_eq!(settings.lazygit.as_deref(), Some(&words(&["gitui"])[..]));
}

#[test]
fn a_lazygit_that_isnt_executable_isnt_on_the_path() {
    let dir = path_with_lazygit(false);
    fs::write(dir.path().join("lazygit"), "not a program").unwrap();

    let settings = Settings::from_config(&Table::new(), None, Some(dir.path().as_os_str()));

    assert_eq!(settings.lazygit, None);
}

#[test]
fn placeholders_are_filled_in_inside_words() {
    let command = words(&["tmux", "-s", "{session}", "--dir={path}"]);
    assert_eq!(
        fill(&command, Path::new("/p/my app"), Some("my app")),
        ["tmux", "-s", "my app", "--dir=/p/my app"]
    );
}
