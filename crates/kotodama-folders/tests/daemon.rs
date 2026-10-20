//! The Folder source under the real Daemon, as a Front end sees it: another
//! Extension's Contributed action on its projects, and Frecency ranking its
//! Items. The other Extension is the shell example (`examples/shell-extension`),
//! which needs bash 4+ and jq, as the dev environment provides; `tee` records
//! what the Daemon sends it.

use std::fs;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

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
    let folders = extensions.join("folders");
    fs::create_dir_all(&folders).unwrap();
    fs::write(
        folders.join("manifest.toml"),
        format!(
            "name = \"folders\"\ncommand = [\"{}\"]\n",
            env!("CARGO_BIN_EXE_kotodama-folders")
        ),
    )
    .unwrap();
    let shell = extensions.join("shell-example");
    fs::create_dir_all(&shell).unwrap();
    for file in ["extension.sh", "manifest.toml"] {
        fs::copy(example_dir().join(file), shell.join(file)).unwrap();
    }
    let manifest = fs::read_to_string(shell.join("manifest.toml"))
        .unwrap()
        .replace(
            r#"command = ["./extension.sh"]"#,
            r#"command = ["/bin/sh", "-c", "tee -a received | ./extension.sh"]"#,
        );
    fs::write(shell.join("manifest.toml"), manifest).unwrap();

    let config = root.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        format!(
            "[extensions.folders]\nfolders = [{{ path = \"{}\", kind = \"projects\" }}]\nopen = [\"true\"]\n",
            root.join("Projects").display()
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
    };
    let socket = paths.socket.clone();
    thread::spawn(move || kotodamad::run(&paths));

    let start = Instant::now();
    let stream = loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
            break stream;
        }
        assert!(start.elapsed() < TIMEOUT, "the Daemon never listened");
        thread::sleep(Duration::from_millis(20));
    };
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    let channel = Channel::handshake(
        stream.try_clone().unwrap(),
        stream,
        &Hello::new(&WireFormat::ALL),
    )
    .unwrap();
    Client { channel }
}

struct Client {
    channel: Channel<UnixStream, UnixStream>,
}

impl Client {
    fn send(&mut self, message: FromFrontEnd) {
        self.channel.send(&message).unwrap();
    }

    fn recv(&mut self) -> ToFrontEnd {
        self.channel.recv().unwrap().expect("the Daemon hung up")
    }

    /// The Folder source's Results for `query`, once there are `count`.
    fn projects(&mut self, query: &str, count: usize) -> Vec<ShownResult> {
        let start = Instant::now();
        loop {
            self.send(FromFrontEnd::Query { text: query.into() });
            let results = loop {
                if let ToFrontEnd::Results { results } = self.recv() {
                    break results;
                }
            };
            let projects: Vec<ShownResult> = results
                .into_iter()
                .filter(|result| result.extension == "folders")
                .collect();
            if projects.len() == count {
                return projects;
            }
            assert!(start.elapsed() < TIMEOUT, "got {projects:?}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Runs `action` of `extension` on the Folder source's Item `id`, and
    /// waits for the menu to close.
    fn run(&mut self, id: &str, extension: &str, action: &str) {
        self.send(FromFrontEnd::RunAction {
            result: ResultRef {
                extension: "folders".into(),
                id: id.into(),
            },
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
        if result.contributed.iter().any(|offered| {
            offered.extension == "shell-example" && offered.action.id == "paste-path"
        }) {
            break result;
        }
        assert!(start.elapsed() < TIMEOUT, "{:?}", result.contributed);
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(result.item.title, "kotodama");

    ui.run(&result.item.id, "shell-example", "paste-path");
    let received =
        fs::read_to_string(root.path().join("extensions/shell-example/received")).unwrap();
    let run = received
        .lines()
        .find(|line| line.contains("\"run_action\""))
        .expect("the shell example got no run_action");
    assert!(
        run.contains(&format!("\"path\":\"{}\"", project.display())),
        "{run}"
    );
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
