//! The Folder source under the real Daemon, as a Front end sees it: another
//! Extension's Contributed action on its projects, and Frecency ranking its
//! Items. The other Extension is the shell example (`examples/shell-extension`),
//! which needs bash 4+ and jq, as the dev environment provides; `tee` records
//! what the Daemon sends it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::testing;
use kotodama_protocol::*;
use kotodamad::paths::Paths;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(10);

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/shell-extension")
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

/// Starts the Daemon in this process on a home in `root`, with the Folder
/// source on `root/Projects` and the shell example installed.
fn start(root: &Path) -> Client {
    let extensions = root.join("extensions");
    testing::install(
        &extensions,
        "folders",
        &format!(
            "name = \"folders\"\ncommand = [\"{}\"]\n",
            env!("CARGO_BIN_EXE_kotodama-folders")
        ),
    );
    let shell = extensions.join("shell-example");
    fs::create_dir_all(&shell).unwrap();
    for file in ["extension.sh", "manifest.toml"] {
        fs::copy(example_dir().join(file), shell.join(file)).unwrap();
    }
    let manifest = fs::read_to_string(shell.join("manifest.toml")).unwrap().replace(
        r#"command = ["./extension.sh"]"#,
        r#"command = ["/bin/sh", "-c", "tee -a received | ./extension.sh"]"#,
    );
    fs::write(shell.join("manifest.toml"), manifest).unwrap();

    let config = root.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        format!(
            "[extensions.folders]\n\
             folders = [{{ path = \"{}\", kind = \"projects\" }}, {{ path = \"{}\", kind = \"scripts\" }}]\n\
             open = [\"true\"]\nterminal = [\"true\"]\n",
            root.join("Projects").display(),
            root.join("Scripts").display()
        ),
    )
    .unwrap();

    let runtime = root.join("runtime");
    fs::create_dir_all(&runtime).unwrap();
    let paths = Paths {
        socket: runtime.join("kotodama.sock"),
        config_dirs: vec![config],
        extension_dirs: vec![extensions],
        history: root.join("state/history.json"),
        usage_log: root.join("state/usage.jsonl"),
        cache: root.join("cache"),
    };
    let socket = paths.socket.clone();
    thread::spawn(move || kotodamad::run(&paths));

    let mut client = Client {
        channel: testing::connect(&socket, TIMEOUT),
        shown: 0,
    };
    client.settle();
    client
}

struct Client {
    channel: Channel<UnixStream, UnixStream>,
    /// The version of the last Results read.
    shown: u32,
}

impl Client {
    fn send(&mut self, message: FromFrontEnd) {
        self.channel.send(&message).unwrap();
    }

    fn recv(&mut self) -> ToFrontEnd {
        self.channel.recv().unwrap().expect("the Daemon hung up")
    }

    /// Results for `query`, once they are `done`. The Daemon sends Late
    /// Results whenever an Extension comes up or sends Items, and once the
    /// rerank after a keystroke is over they keep the Results on screen in
    /// the order they were. So the
    /// Results read after a Query can be Late Results sent before the Daemon
    /// got the Query, and only what `done` checks can be relied on.
    fn search(&mut self, query: &str, done: impl Fn(&[ShownResult]) -> bool) -> Vec<ShownResult> {
        let start = Instant::now();
        loop {
            self.send(FromFrontEnd::Query { text: query.into() });
            let results = loop {
                if let ToFrontEnd::Results { version, results, .. } = self.recv() {
                    self.shown = version;
                    break results;
                }
            };
            if done(&results) {
                return results;
            }
            assert!(start.elapsed() < TIMEOUT, "got {results:?}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the shell example is up and has sent its Items, so it
    /// causes no Late Results after a test's Query. The Folder source's Items
    /// are waited for by [`Client::projects`].
    fn settle(&mut self) {
        self.search("timer", |results| {
            results
                .iter()
                .any(|result| result.extension == "shell-example" && result.item.id == "timer")
        });
    }

    /// The Folder source's Results for `query`, once there are `count`. With
    /// them, and the shell example settled, the Daemon has every Item it
    /// will get, so the next Results it sends answer the next Query.
    fn projects(&mut self, query: &str, count: usize) -> Vec<ShownResult> {
        let folders = |results: &[ShownResult]| -> Vec<ShownResult> {
            results
                .iter()
                .filter(|result| result.extension == "folders")
                .cloned()
                .collect()
        };
        folders(&self.search(query, |results| folders(results).len() == count))
    }

    /// Runs `action` of `extension` on the Folder source's Item `id`, and
    /// waits for the menu to close.
    fn run(&mut self, id: &str, extension: &str, action: &str) {
        self.send(FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "folders".into(),
                id: id.into(),
            },
            shown: self.shown,
            action: ActionRef {
                extension: extension.into(),
                id: action.into(),
            },
            arguments: Default::default(),
        });
        loop {
            match self.recv() {
                ToFrontEnd::Close => return,
                ToFrontEnd::ActionFailed { message, .. } => panic!("{message}"),
                _ => {}
            }
        }
    }
}

fn root() -> TempDir {
    tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

#[test]
fn the_shell_examples_contributed_action_shows_on_a_project_and_gets_its_path() {
    let root = root();
    let project = root.path().join("Projects/kotodama");
    repo(&project);
    let mut ui = start(root.path());

    // The shell example's Actions are offered once it is running.
    let start = Instant::now();
    let result = loop {
        let result = ui.projects("kotodama", 1).remove(0);
        if result
            .contributed
            .iter()
            .any(|offered| offered.extension == "shell-example" && offered.action.id == "paste-path")
        {
            break result;
        }
        assert!(start.elapsed() < TIMEOUT, "{:?}", result.contributed);
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(result.item.title, "kotodama");

    ui.run(&result.item.id, "shell-example", "paste-path");
    let received = fs::read_to_string(root.path().join("extensions/shell-example/received")).unwrap();
    let run = received
        .lines()
        .find(|line| line.contains("\"run_action\""))
        .expect("the shell example got no run_action");
    assert!(run.contains(&format!("\"path\":\"{}\"", project.display())), "{run}");
}

#[test]
fn a_picked_project_ranks_first_after() {
    let root = root();
    repo(&root.path().join("Projects/app-one"));
    repo(&root.path().join("Projects/app-two"));
    let mut ui = start(root.path());

    let before = ui.projects("app", 2);
    let other = before[1].item.clone();
    ui.run(&other.id, "folders", "open");

    let after = ui.projects("app", 2);
    assert_eq!(after[0].item.title, other.title);
}

#[test]
fn a_picked_script_ranks_first_after() {
    let root = root();
    for name in ["deploy-one.sh", "deploy-two.sh"] {
        let path = root.path().join("Scripts").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut ui = start(root.path());

    let before = ui.projects("deploy", 2);
    let other = before[1].item.clone();
    ui.run(&other.id, "folders", "run");

    let after = ui.projects("deploy", 2);
    assert_eq!(after[0].item.title, other.title);
}
