use super::*;

#[test]
fn every_scaffold_is_filled_in_with_the_name_and_runs_its_program() {
    for (language, command) in [
        (Language::Shell, r#"command = ["./extension.sh"]"#),
        (Language::Python, r#"command = ["./extension.py"]"#),
        (Language::Rust, r#"command = ["bin/kotodama-notes"]"#),
    ] {
        let files = files("notes", language);
        for file in &files {
            assert!(!file.contents.contains("{{"), "{} has a blank left", file.path);
        }
        let manifest = &files[0];
        assert_eq!(manifest.path, "manifest.toml");
        assert!(
            manifest.contents.contains(command),
            "{language:?}: {}",
            manifest.contents
        );
        assert!(manifest.contents.contains(r#"name = "notes""#));
    }
}

#[test]
fn a_scripts_program_is_executable() {
    for language in [Language::Shell, Language::Python] {
        let files = files("notes", language);
        assert!(
            files
                .iter()
                .any(|file| file.executable && file.contents.starts_with("#!"))
        );
    }
}

#[test]
fn a_name_the_daemon_refuses_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (name, refused) in [
        ("Notes", "InvalidName"),
        ("my notes", "InvalidName"),
        ("kotodama", "ReservedName"),
    ] {
        let mut options = Options::new(name);
        options.parent = dir.path().to_owned();
        let err = write(&options).unwrap_err();
        assert!(format!("{err:?}").starts_with(refused), "{name}: {err:?}");
    }
}

#[test]
fn it_writes_a_new_directory_and_never_over_an_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = Options::new("notes");
    options.language = Language::Shell;
    options.parent = dir.path().join("extensions");

    let written = write(&options).unwrap();
    assert_eq!(written, dir.path().join("extensions/notes"));
    let script = fs::metadata(written.join("extension.sh")).unwrap();
    assert_eq!(script.permissions().mode() & 0o111, 0o111);

    assert!(matches!(write(&options), Err(ScaffoldError::Exists(_))));
}
