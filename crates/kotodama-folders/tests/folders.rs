//! Drives the Folder source the way the Daemon does: over its stdio, with
//! its home folder in a temporary directory. Commands the Actions run are
//! small scripts that write down how they were started. Needs `git`, which
//! the dev environment provides.

use std::collections::BTreeMap;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;
use tempfile::TempDir;

/// Long enough for a change on disk to be noticed and settle.
const WAIT: Duration = Duration::from_secs(5);

/// The Extension is declared first, so it is killed before its home goes.
struct Folders {
    extension: Spawned,
    home: TempDir,
}

impl Deref for Folders {
    type Target = Spawned;
    fn deref(&self) -> &Spawned {
        &self.extension
    }
}

impl DerefMut for Folders {
    fn deref_mut(&mut self) -> &mut Spawned {
        &mut self.extension
    }
}

impl Folders {
    /// Starts the Extension with `home` as its home folder.
    fn start(home: TempDir) -> Self {
        let extension = Spawned::start(
            Command::new(env!("CARGO_BIN_EXE_kotodama-folders"))
                .env("HOME", home.path())
                .env("KOTODAMA_LAUNCH_PREFIX", ""),
            WAIT,
        );
        Folders { extension, home }
    }

    /// Starts the Extension and sends it `config`.
    fn configured(home: TempDir, config: &str) -> Self {
        let mut folders = Folders::start(home);
        folders.configure(config);
        folders
    }

    /// Sends `config`, TOML for the `[extensions.folders]` table.
    fn configure(&mut self, config: &str) {
        let table: toml::Table = toml::from_str(config).unwrap();
        self.send(ToExtension::Config(table_from(table)));
    }

    fn path(&self) -> &Path {
        self.home.path()
    }

    /// The Items of the next message, which must be `add_items`, by title.
    fn added(&self) -> BTreeMap<String, Item> {
        self.extension
            .added()
            .into_iter()
            .map(|item| (item.title.clone(), item))
            .collect()
    }

    /// Runs `action` on `item` with `arguments`, and returns the outcome.
    fn run_with(&mut self, item: &Item, action: &str, arguments: &[(&str, &str)]) -> Outcome {
        let arguments = arguments
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect();
        self.start_run(7, action, item.to_ref(), arguments);
        self.outcome(7, |message| {
            matches!(message, FromExtension::JobProgress(_))
        })
    }

    fn run(&mut self, item: &Item, action: &str) -> Outcome {
        self.run_with(item, action, &[])
    }

    fn job(&self) -> Job {
        match self.recv() {
            FromExtension::JobProgress(job) => job,
            other => panic!("expected job_progress, got {other:?}"),
        }
    }
}

fn table_from(table: toml::Table) -> Table {
    table
        .into_iter()
        .map(|(key, value)| (key, value_from(value)))
        .collect()
}

fn value_from(value: toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Integer(i),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Array(values) => Value::Array(values.into_iter().map(value_from).collect()),
        toml::Value::Table(table) => Value::Table(table_from(table)),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
    }
}

/// A home folder, canonical so paths compare equal.
fn home() -> TempDir {
    tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

/// A repository with one commit at `dir`.
fn repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success());
    }
}

fn executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A command that writes its working directory and arguments, one per line,
/// to `out`.
fn recorder(dir: &Path, out: &Path) -> PathBuf {
    let path = dir.join("bin/recorder");
    executable(
        &path,
        &format!(
            "#!/bin/sh\n{{ pwd; printf '%s\\n' \"$@\"; }} > '{}.tmp'\nmv '{0}.tmp' '{0}'\n",
            out.display()
        ),
    );
    path
}

/// What the recorder wrote, once it has.
fn recorded(out: &Path) -> Vec<String> {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(out) {
            return text.lines().map(str::to_owned).collect();
        }
        assert!(start.elapsed() < WAIT, "nothing ran");
        thread::sleep(Duration::from_millis(20));
    }
}

fn action_ids(item: &Item) -> Vec<&str> {
    item.actions
        .iter()
        .map(|action| action.id.as_str())
        .collect()
}

#[test]
fn nothing_is_sent_before_the_config_and_folders_come_from_it() {
    let home = home();
    repo(&home.path().join("Projects/kotodama"));
    let mut folders = Folders::start(home);
    folders.quiet();

    folders.configure(r#"folders = [{ path = "~/Projects", kind = "projects" }]"#);
    let items = folders.added();
    let project = &items["kotodama"];
    let path = folders.path().join("Projects/kotodama");
    assert_eq!(project.id, path.to_string_lossy());
    assert_eq!(project.kind, ItemKind::Project { path: path.clone() });
    assert_eq!(
        project.identity.as_deref(),
        Some(format!("folder:{}", path.display()).as_str())
    );
    assert_eq!(project.subtitle.as_deref(), Some("~/Projects/kotodama"));
    assert_eq!(
        action_ids(project),
        ["open", "shell", "lazygit", "file-manager", "copy-path"]
    );
}

#[test]
fn opening_a_project_runs_the_command_with_its_tms_session_in_its_folder() {
    let home = home();
    let out = home.path().join("out");
    let recorder = recorder(home.path(), &out);
    repo(&home.path().join("Projects/kotodama.nvim"));
    let mut folders = Folders::configured(
        home,
        &format!(
            r#"
            folders = [{{ path = "~/Projects", kind = "projects" }}]
            open = ["{}", "-s", "{{session}}", "-c", "{{path}}"]
            "#,
            recorder.display()
        ),
    );
    let project = folders.added()["kotodama.nvim"].clone();

    assert_eq!(folders.run(&project, "open"), Outcome::Close);
    let path = folders.path().join("Projects/kotodama.nvim");
    assert_eq!(
        recorded(&out),
        [
            path.to_str().unwrap(),
            "-s",
            "kotodama_nvim",
            "-c",
            path.to_str().unwrap()
        ]
    );
}

#[test]
fn each_project_action_runs_its_own_command() {
    let home = home();
    repo(&home.path().join("Projects/app"));
    let mut config = String::from(r#"folders = [{ path = "~/Projects", kind = "projects" }]"#);
    for key in ["shell", "lazygit", "file_manager", "copy_path"] {
        let recorder = recorder(
            &home.path().join(key),
            &home.path().join(format!("{key}.out")),
        );
        config.push_str(&format!(
            "\n{key} = [\"{}\", \"{key}\", \"{{path}}\"]",
            recorder.display()
        ));
    }
    let mut folders = Folders::configured(home, &config);
    let project = folders.added()["app"].clone();
    let path = folders.path().join("Projects/app");

    for (action, key) in [
        ("shell", "shell"),
        ("lazygit", "lazygit"),
        ("file-manager", "file_manager"),
        ("copy-path", "copy_path"),
    ] {
        assert_eq!(folders.run(&project, action), Outcome::Close);
        assert_eq!(
            recorded(&folders.path().join(format!("{key}.out"))),
            [path.to_str().unwrap(), key, path.to_str().unwrap()]
        );
    }
}

#[test]
fn a_command_that_isnt_installed_fails_with_a_message() {
    let home = home();
    repo(&home.path().join("Projects/app"));
    let mut folders = Folders::configured(
        home,
        r#"
        folders = [{ path = "~/Projects", kind = "projects" }]
        open = ["kotodama-no-such-terminal"]
        "#,
    );
    let project = folders.added()["app"].clone();
    match folders.run(&project, "open") {
        Outcome::Failed { message } => assert!(message.contains("kotodama-no-such-terminal")),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn executable_files_are_scripts_titled_by_their_subfolders() {
    let home = home();
    let scripts = home.path().join("Scripts");
    executable(&scripts.join("backup.sh"), "#!/bin/sh\n");
    executable(&scripts.join("work/deploy.sh"), "#!/bin/sh\n");
    executable(&scripts.join("work/db/vacuum"), "#!/bin/sh\n");
    executable(&scripts.join(".hidden.sh"), "#!/bin/sh\n");
    executable(&scripts.join(".git/hooks/pre-commit"), "#!/bin/sh\n");
    fs::write(scripts.join("notes.txt"), "not executable").unwrap();
    executable(
        &scripts.join("named.sh"),
        "#!/bin/sh\n# kotodama: name=Deploy staging\n",
    );
    executable(&scripts.join("app.desktop"), "[Desktop Entry]\n");

    let folders = Folders::configured(
        home,
        r#"folders = [{ path = "~/Scripts", kind = "scripts" }]"#,
    );
    let items = folders.added();
    assert_eq!(
        items.keys().collect::<Vec<_>>(),
        [
            "Deploy staging",
            "app",
            "backup",
            "work › db › vacuum",
            "work › deploy"
        ]
    );
    let backup = &items["backup"];
    let path = folders.path().join("Scripts/backup.sh");
    assert_eq!(backup.kind, ItemKind::File { path: path.clone() });
    assert_eq!(
        backup.identity.as_deref(),
        Some(format!("file:{}", path.display()).as_str())
    );
    assert_eq!(backup.subtitle.as_deref(), Some("~/Scripts/backup.sh"));
    assert_eq!(action_ids(backup), ["run", "copy-path"]);
    assert!(backup.actions[0].arguments.is_empty());
    // Found by its file's name too.
    assert_eq!(items["Deploy staging"].keywords, ["named"]);
    // A desktop entry is a file like any other, not the apps Extension's app.
    assert_eq!(
        items["app"].identity.as_deref(),
        Some(
            format!(
                "file:{}",
                folders.path().join("Scripts/app.desktop").display()
            )
            .as_str()
        )
    );
}

#[test]
fn a_script_runs_in_the_terminal_in_its_folder() {
    let home = home();
    let out = home.path().join("out");
    let recorder = recorder(home.path(), &out);
    let script = home.path().join("Scripts/work/backup.sh");
    executable(&script, "#!/bin/sh\n");
    let mut folders = Folders::configured(
        home,
        &format!(
            r#"
            folders = [{{ path = "~/Scripts", kind = "scripts" }}]
            terminal = ["{}", "--hold", "-e"]
            "#,
            recorder.display()
        ),
    );
    let item = folders.added()["work › backup"].clone();

    assert_eq!(folders.run(&item, "run"), Outcome::Close);
    assert_eq!(
        recorded(&out),
        [
            folders.path().join("Scripts/work").to_str().unwrap(),
            "--hold",
            "-e",
            script.to_str().unwrap()
        ]
    );
}

#[test]
fn a_script_that_asks_for_arguments_gets_them_split_as_a_shell_would() {
    let home = home();
    let out = home.path().join("out");
    executable(
        &home.path().join("Scripts/greet"),
        &format!(
            "#!/bin/sh\n# kotodama: args=prompt\nfor arg in \"$@\"; do echo \"$arg\"; done > '{}'\n",
            out.display()
        ),
    );
    // `env` runs the script itself, as a terminal would.
    let mut folders = Folders::configured(
        home,
        r#"
        folders = [{ path = "~/Scripts", kind = "scripts" }]
        terminal = ["env"]
        "#,
    );
    let item = folders.added()["greet"].clone();
    let run = &item.actions[0];
    assert_eq!(run.arguments.len(), 1);
    assert!(
        !run.arguments[0].optional,
        "a required Argument opens a form"
    );

    let argument = run.arguments[0].name.clone();
    assert_eq!(
        folders.run_with(&item, "run", &[(&argument, r#"one "two words" 'three'"#)]),
        Outcome::Close
    );
    assert_eq!(recorded(&out), ["one", "two words", "three"]);

    match folders.run_with(&item, "run", &[(&argument, "\"open")]) {
        Outcome::Failed { .. } => {}
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_background_script_is_a_job_that_ends_with_its_last_line() {
    let home = home();
    executable(
        &home.path().join("Scripts/ok.sh"),
        "#!/bin/sh\n# kotodama: terminal=false name=Sync\necho working\necho synced 3 files\n",
    );
    executable(
        &home.path().join("Scripts/fail.sh"),
        "#!/bin/sh\n# kotodama: terminal=false\necho oops >&2\nexit 3\n",
    );
    let mut folders = Folders::configured(
        home,
        r#"
        folders = [{ path = "~/Scripts", kind = "scripts" }]
        terminal = ["kotodama-no-such-terminal"]
        "#,
    );
    let items = folders.added();
    assert_eq!(items["Sync"].actions[0].title, "Run in Background");

    folders.send(ToExtension::RunAction {
        invocation: 1,
        action: "run".into(),
        item: items["Sync"].to_ref(),
        arguments: BTreeMap::new(),
    });
    let started = folders.job();
    assert_eq!(
        (started.title.as_str(), started.state),
        ("Sync", JobState::Running)
    );
    assert!(matches!(
        folders.recv(),
        FromExtension::ActionDone {
            invocation: 1,
            outcome: Outcome::Close
        }
    ));
    let done = folders.job();
    assert_eq!(done.id, started.id);
    assert_eq!(done.state, JobState::Succeeded);
    assert_eq!(done.message.as_deref(), Some("synced 3 files"));

    assert_eq!(folders.run(&items["fail"], "run"), Outcome::Close);
    let failed = folders.job();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(
        failed.message.as_deref(),
        Some("Exited with status 3: oops")
    );
}

#[test]
fn new_projects_and_scripts_appear_and_removed_ones_go_without_a_restart() {
    let home = home();
    fs::create_dir_all(home.path().join("Projects/work")).unwrap();
    fs::create_dir_all(home.path().join("Scripts")).unwrap();
    let folders = Folders::configured(
        home,
        r#"
        folders = [
          { path = "~/Projects", kind = "projects", depth = 2 },
          { path = "~/Scripts", kind = "scripts" },
          { path = "~/Later", kind = "scripts" },
        ]
        "#,
    );
    folders.quiet();

    repo(&folders.path().join("Projects/work/api"));
    assert_eq!(folders.added().keys().collect::<Vec<_>>(), ["api"]);

    executable(
        &folders.path().join("Scripts/tools/backup.sh"),
        "#!/bin/sh\n",
    );
    assert_eq!(
        folders.added().keys().collect::<Vec<_>>(),
        ["tools › backup"]
    );

    // A header edit renames it.
    executable(
        &folders.path().join("Scripts/tools/backup.sh"),
        "#!/bin/sh\n# kotodama: name=Back up\n",
    );
    assert_eq!(
        folders.added().keys().collect::<Vec<_>>(),
        ["tools › Back up"]
    );

    // A folder that didn't exist yet.
    executable(&folders.path().join("Later/hello"), "#!/bin/sh\n");
    assert_eq!(folders.added().keys().collect::<Vec<_>>(), ["hello"]);

    fs::remove_dir_all(folders.path().join("Projects/work/api")).unwrap();
    assert_eq!(
        folders.removed(),
        [folders.path().join("Projects/work/api").to_string_lossy()]
    );
}

#[test]
fn a_config_without_folders_takes_every_item_away() {
    let home = home();
    repo(&home.path().join("Projects/app"));
    let mut folders = Folders::configured(
        home,
        r#"folders = [{ path = "~/Projects", kind = "projects" }]"#,
    );
    assert_eq!(folders.added().len(), 1);

    folders.configure("");
    assert_eq!(
        folders.removed(),
        [folders.path().join("Projects/app").to_string_lossy()]
    );
}
