use super::*;

fn installed(toml: &str) -> Found {
    Found::new(Manifest::from_toml(toml).unwrap(), PathBuf::from("/ext/calc")).unwrap()
}

fn configure_with(installed: &[Found], toml: &str) -> (Vec<Found>, Vec<String>) {
    let config = crate::config::parse(toml).unwrap();
    let mut report = Report::new();
    let configured = configure(installed, &config, &mut report);
    let abouts = report
        .into_problems()
        .into_iter()
        .map(|problem| problem.about)
        .collect();
    (configured, abouts)
}

fn calculator() -> Found {
    installed("name = \"calculator\"\ncommand = [\"bin/calc\"]\n[live_query]\ntriggers = ['^\\d']\ndebounce_ms = 60\n")
}

#[test]
fn a_bare_program_is_looked_up_on_path_only_when_the_config_names_it() {
    let shell = installed("name = \"calculator\"\ncommand = [\"sh\"]\n");
    assert_eq!(shell.program(), PathBuf::from("/ext/calc/sh"));

    let (configured, _) = configure_with(
        &[shell],
        "[extensions.calculator]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
    );

    let program = configured[0].program();
    assert!(program.is_absolute(), "{}", program.display());
    assert!(program.is_file(), "{}", program.display());
}

#[test]
fn an_extension_without_a_table_runs_as_installed() {
    let (configured, problems) = configure_with(&[calculator()], "");

    assert_eq!(configured[0].manifest, calculator().manifest);
    assert!(problems.is_empty());
}

#[test]
fn enable_false_switches_an_installed_extension_off() {
    let (configured, problems) = configure_with(&[calculator()], "[extensions.calculator]\nenable = false\n");

    assert!(configured.is_empty());
    assert!(problems.is_empty());
}

#[test]
fn command_env_and_live_queries_override_the_manifest() {
    let (configured, problems) = configure_with(
        &[calculator()],
        "[extensions.calculator]\ncommand = [\"/dev/calc\", \"-v\"]\nenv = { RUST_LOG = \"debug\" }\n\
             live_query = { debounce_ms = 0, prefixes = [\"= \"] }\n",
    );

    assert!(problems.is_empty(), "{problems:?}");
    let calc = &configured[0];
    assert_eq!(calc.manifest.command, ["/dev/calc", "-v"]);
    assert_eq!(calc.env, BTreeMap::from([("RUST_LOG".to_owned(), "debug".to_owned())]));
    let live = calc.manifest.live_query.as_ref().unwrap();
    assert_eq!(
        (live.debounce_ms, live.prefixes.as_slice()),
        (0, ["= ".to_owned()].as_slice())
    );
    assert_eq!(live.triggers, ["^\\d"]);
    assert_eq!(calc.dir, PathBuf::from("/ext/calc"));
    assert!(!calc.inline);
}

#[test]
fn a_bad_override_is_a_problem_and_the_manifest_stands() {
    let (configured, problems) = configure_with(
        &[calculator()],
        "[extensions.calculator]\ncommand = []\nlive_query = { triggers = [\"(\"] }\n",
    );

    assert_eq!(configured[0].manifest, calculator().manifest);
    assert_eq!(
        problems,
        ["extensions.calculator.command", "extensions.calculator.live_query"]
    );
}

#[test]
fn a_table_with_a_command_defines_an_extension_from_its_manifest_keys() {
    let (configured, problems) = configure_with(
        &[calculator()],
        "[extensions.notes]\ncommand = [\"notes\", \"--kotodama\"]\ntitle = \"Notes\"\nword = \"note\"\n\
             env = { NOTES_DIR = \"/tmp\" }\nlive_query = { prefixes = [\"note \"], debounce_ms = 100 }\n\
             folder = \"~/notes\"\n",
    );

    assert!(problems.is_empty(), "{problems:?}");
    let notes = &configured[1];
    assert!(notes.inline);
    assert_eq!(notes.manifest.name, "notes");
    assert_eq!(notes.manifest.command, ["notes", "--kotodama"]);
    assert_eq!(notes.manifest.title.as_deref(), Some("Notes"));
    assert_eq!(notes.manifest.word.as_deref(), Some("note"));
    assert_eq!(notes.manifest.live_query.as_ref().unwrap().prefixes, ["note "]);
    assert_eq!(notes.env["NOTES_DIR"], "/tmp");
}

#[test]
fn a_table_without_a_command_defines_nothing() {
    let (configured, problems) = configure_with(&[], "[extensions.emoji]\nword = \"e\"\n");

    assert!(configured.is_empty());
    assert!(problems.is_empty());
}

#[test]
fn an_extension_defined_badly_is_a_problem_and_isnt_run() {
    let (configured, problems) = configure_with(
        &[],
        "[extensions.notes]\ncommand = [\"bin/notes\"]\n[extensions.Bad]\ncommand = [\"x\"]\n",
    );

    assert!(configured.is_empty());
    assert_eq!(problems, ["extensions.Bad", "extensions.notes"]);
}
