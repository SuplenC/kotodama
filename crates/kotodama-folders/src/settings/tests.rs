use std::collections::BTreeMap;

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
    let settings = Settings::from_config(&config, Some(Path::new("/home/me")));
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
    let settings = Settings::from_config(&config, None);
    assert_eq!(settings.open, Settings::default().open);
    assert_eq!(settings.terminal, ["foot"]);
}

#[test]
fn placeholders_are_filled_in_inside_words() {
    let command = words(&["tmux", "-s", "{session}", "--dir={path}"]);
    assert_eq!(
        fill(&command, Path::new("/p/my app"), Some("my app")),
        ["tmux", "-s", "my app", "--dir=/p/my app"]
    );
}
