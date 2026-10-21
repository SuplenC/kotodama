use super::*;

fn overrides(toml: &str) -> (Overrides, Vec<String>) {
    let config = crate::config::parse(toml).unwrap();
    let mut report = Report::new();
    let overrides = Overrides::from_config(&config, &mut report);
    let abouts = report
        .into_problems()
        .into_iter()
        .map(|problem| problem.about)
        .collect();
    (overrides, abouts)
}

#[test]
fn paths_reads_directories_and_refuses_relative_ones() {
    let (overrides, problems) = overrides(
        "[paths]\nextension_dirs = [\"/opt/ext\", \"ext\"]\ndefault_extension_dirs = false\n\
             state_dir = \"/var/lib/kotodama\"\ncache_dir = \"cache\"\nsocket = \"/run/k.sock\"\n",
    );

    assert_eq!(
        overrides,
        Overrides {
            extension_dirs: vec![PathBuf::from("/opt/ext")],
            default_extension_dirs: false,
            state_dir: Some(PathBuf::from("/var/lib/kotodama")),
            cache_dir: None,
            socket: Some(PathBuf::from("/run/k.sock")),
        }
    );
    assert_eq!(problems, ["paths.cache_dir", "paths.extension_dirs[1]"]);
}

#[test]
fn extension_dirs_from_the_config_come_after_the_default_ones_unless_those_are_off() {
    let config_dirs = [
        PathBuf::from("/etc/xdg/kotodama"),
        PathBuf::from("/home/me/.config/kotodama"),
    ];
    let mut overrides = Overrides {
        extension_dirs: vec![PathBuf::from("/opt/ext")],
        ..Overrides::default()
    };

    let dirs = extension_dirs(&config_dirs, &overrides);
    assert_eq!(
        dirs[dirs.len() - 2..],
        [
            PathBuf::from("/home/me/.config/kotodama/extensions"),
            PathBuf::from("/opt/ext")
        ]
    );

    overrides.default_extension_dirs = false;
    assert_eq!(extension_dirs(&config_dirs, &overrides), [PathBuf::from("/opt/ext")]);
}

#[test]
fn a_named_config_dir_is_the_whole_config() {
    assert_eq!(
        config_dirs(Some(PathBuf::from("/tmp/conf"))).unwrap(),
        [PathBuf::from("/tmp/conf")]
    );
}
