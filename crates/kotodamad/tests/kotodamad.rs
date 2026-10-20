//! The `kotodamad` binary, as a Front end sees it: over its socket, with real
//! Extension processes. The test Extension is a bash script, so these need
//! bash 4+, which the dev environment provides.

use std::fs;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use kotodama_protocol::*;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(10);

struct Daemon {
    child: Child,
    root: TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recorder/extension.sh")
}

impl Daemon {
    /// A home with no Extensions or config yet; add them before `start`.
    fn home() -> TempDir {
        let root = TempDir::new().unwrap();
        for dir in [
            "runtime",
            "config/kotodama/extensions",
            "etc/kotodama",
            "share",
        ] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        root
    }

    fn start(root: TempDir) -> Self {
        let child = spawn(root.path());
        Daemon { child, root }
    }

    /// Kills the Daemon and starts a new one on the same home.
    fn restart(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = spawn(self.root.path());
    }

    fn extension_dir(&self, name: &str) -> PathBuf {
        extension_dir(&self.root, name)
    }

    fn connect(&self) -> Client {
        let socket = self.root.path().join("runtime/kotodama.sock");
        let stream = wait_for("the socket", || UnixStream::connect(&socket).ok());
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let channel = Channel::handshake(
            stream.try_clone().unwrap(),
            stream,
            &Hello::new(&WireFormat::ALL),
        )
        .unwrap();
        Client { channel }
    }
}

fn spawn(root: &Path) -> Child {
    with_home(&mut Command::new(env!("CARGO_BIN_EXE_kotodamad")), root)
        .spawn()
        .unwrap()
}

fn with_home<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
    command
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CONFIG_DIRS", root.join("etc"))
        .env("XDG_DATA_DIRS", root.join("share"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root)
        .env_remove("WAYLAND_DISPLAY")
}

/// Starts the Daemon the way systemd's socket activation does: `listener` as
/// file descriptor 3, named by `LISTEN_PID` and `LISTEN_FDS`.
#[cfg(target_os = "linux")]
fn spawn_activated(root: &Path, listener: &std::os::unix::net::UnixListener) -> Child {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let fd = listener.as_raw_fd();
    // LISTEN_PID must be the Daemon's own pid, which only the shell that execs
    // it knows.
    let mut command = Command::new("bash");
    command
        .args(["-c", "LISTEN_PID=$$ LISTEN_FDS=1 exec \"$0\""])
        .arg(env!("CARGO_BIN_EXE_kotodamad"));
    with_home(&mut command, root);
    // SAFETY: dup2 and fcntl are async-signal-safe, as code between fork and
    // exec must be. A copy made by dup2 stays open across exec; dup2 onto
    // itself changes nothing, so then the flag is cleared by hand.
    unsafe {
        command.pre_exec(move || {
            let done = if fd == 3 {
                libc::fcntl(3, libc::F_SETFD, 0)
            } else {
                libc::dup2(fd, 3)
            };
            if done == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().unwrap()
}

fn extension_dir(root: &TempDir, name: &str) -> PathBuf {
    root.path().join("config/kotodama/extensions").join(name)
}

/// Installs the recording test Extension as `name`, indexing `items`.
fn install(root: &TempDir, name: &str, manifest_extra: &str, items: &[Item]) {
    install_in(&extension_dir(root, name), name, manifest_extra, items);
}

/// Installs the recording test Extension as `name` in `dir`.
fn install_in(dir: &Path, name: &str, manifest_extra: &str, items: &[Item]) {
    fs::create_dir_all(dir).unwrap();
    fs::copy(fixture(), dir.join("extension.sh")).unwrap();
    fs::write(
        dir.join("manifest.toml"),
        format!("name = \"{name}\"\ncommand = [\"extension.sh\"]\n{manifest_extra}"),
    )
    .unwrap();
    fs::write(
        dir.join("items.json"),
        serde_json::to_string(items).unwrap(),
    )
    .unwrap();
}

fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
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

    /// Reads messages until one satisfies `want`.
    fn until<T>(&mut self, mut want: impl FnMut(ToFrontEnd) -> Option<T>) -> T {
        loop {
            if let Some(value) = want(self.recv()) {
                return value;
            }
        }
    }

    fn titles(&mut self, query: &str) -> Vec<String> {
        self.send(FromFrontEnd::Query { text: query.into() });
        self.until(|message| match message {
            ToFrontEnd::Results { results } => {
                Some(results.into_iter().map(|row| row.item.title).collect())
            }
            _ => None,
        })
    }

    fn run(&mut self, extension: &str, id: &str, action: &str) {
        self.send(FromFrontEnd::RunAction {
            result: ResultRef {
                extension: extension.into(),
                id: id.into(),
            },
            action: ActionRef {
                extension: extension.into(),
                id: action.into(),
            },
            arguments: Default::default(),
        });
        self.until(|message| (message == ToFrontEnd::Close).then_some(()));
    }

    /// Queries until the Results are `want`.
    fn wait_for_titles(&mut self, query: &str, want: &[&str]) {
        wait_for(&format!("{want:?} for {query:?}"), || {
            (self.titles(query) == want).then_some(())
        });
    }
}

fn item(id: &str, title: &str) -> Item {
    Item {
        id: id.into(),
        kind: ItemKind::Url {
            url: format!("https://{id}"),
        },
        title: title.into(),
        subtitle: None,
        icon: None,
        keyword: None,
        identity: None,
        keywords: vec![],
        hidden: false,
        actions: vec![Action {
            id: "open".into(),
            title: "Open".into(),
            shortcut: None,
            arguments: vec![],
        }],
    }
}

fn received(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("received"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_killed_extension_never_stalls_the_daemon_and_comes_back_on_its_own() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[item("kotodama", "Kotodama")]);
    install(&root, "other", "", &[item("other", "Other thing")]);
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    ui.wait_for_titles("kotodama", &["Kotodama"]);

    let pid_file = daemon.extension_dir("bookmarks").join("pid");
    let pid = fs::read_to_string(&pid_file).unwrap();
    fs::remove_file(&pid_file).unwrap();
    Command::new("kill")
        .args(["-9", pid.trim()])
        .status()
        .unwrap();

    // The daemon keeps answering while the Extension is down, and says so.
    ui.until(|message| match message {
        ToFrontEnd::Status { down } if down == ["bookmarks"] => Some(()),
        _ => None,
    });
    assert_eq!(ui.titles("other"), ["Other thing"]);

    ui.until(|message| match message {
        ToFrontEnd::Status { down } if down.is_empty() => Some(()),
        _ => None,
    });
    ui.wait_for_titles("kotodama", &["Kotodama"]);
    assert!(pid_file.exists(), "a new process started");
}

#[test]
fn a_config_change_reaches_the_extension_without_a_restart_and_a_broken_file_keeps_the_last_good_config()
 {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    let user_config = root.path().join("config/kotodama");
    fs::write(
        user_config.join("a.toml"),
        "[extensions.bookmarks]\nv = 1\n",
    )
    .unwrap();
    let daemon = Daemon::start(root);
    let dir = daemon.extension_dir("bookmarks");
    wait_for("the first config", || {
        (received(&dir) == [r#"{"config":{"v":1}}"#]).then_some(())
    });

    fs::write(
        user_config.join("a.toml"),
        "[extensions.bookmarks]\nv = 2\n",
    )
    .unwrap();
    wait_for("the reloaded config", || {
        (received(&dir).last().map(String::as_str) == Some(r#"{"config":{"v":2}}"#)).then_some(())
    });

    // While a file is broken the whole last good config stays, so another
    // file's change doesn't reach the Extension until it is fixed.
    fs::write(user_config.join("c.toml"), "this is not toml").unwrap();
    thread::sleep(Duration::from_millis(300));
    fs::write(
        user_config.join("b.toml"),
        "[extensions.bookmarks]\nv = 3\n",
    )
    .unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        received(&dir).last().map(String::as_str),
        Some(r#"{"config":{"v":2}}"#)
    );
    fs::remove_file(user_config.join("c.toml")).unwrap();
    wait_for("the config once the file is fixed", || {
        (received(&dir).last().map(String::as_str) == Some(r#"{"config":{"v":3}}"#)).then_some(())
    });
    assert!(
        !received(&dir).contains(&r#"{"config":{}}"#.to_owned()),
        "{:?}",
        received(&dir)
    );
}

#[test]
fn an_extension_that_never_says_hello_is_killed_restarted_and_dies_with_the_daemon() {
    let root = Daemon::home();
    let dir = extension_dir(&root, "silent");
    fs::create_dir_all(&dir).unwrap();
    // Counts its starts, then hangs without a word.
    fs::write(
        dir.join("hang.sh"),
        "#!/usr/bin/env bash\necho started >>starts\necho $$ >pid\nexec sleep 1000\n",
    )
    .unwrap();
    Command::new("chmod")
        .args(["+x", dir.join("hang.sh").to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(
        dir.join("manifest.toml"),
        "name = \"silent\"\ncommand = [\"hang.sh\"]\n",
    )
    .unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();

    ui.until(|message| match message {
        ToFrontEnd::Status { down } if down == ["silent"] => Some(()),
        _ => None,
    });
    let starts = || {
        fs::read_to_string(dir.join("starts"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    wait_for("a second start", || (starts() >= 2).then_some(()));

    let pid = wait_for("the second run's pid", || {
        fs::read_to_string(dir.join("pid"))
            .ok()
            .filter(|pid| !pid.trim().is_empty())
    });
    drop(ui);
    drop(daemon);
    let alive = || {
        Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    wait_for("the Extension to die with the Daemon", || {
        (!alive()).then_some(())
    });
}

#[test]
fn a_front_end_that_never_says_hello_is_disconnected() {
    use std::io::Read;

    let daemon = Daemon::start(Daemon::home());
    let socket = daemon.root.path().join("runtime/kotodama.sock");
    let mut silent = wait_for("the socket", || UnixStream::connect(&socket).ok());
    silent.set_read_timeout(Some(TIMEOUT)).unwrap();

    // The Daemon's hello, then the end of the stream once it gives up on ours.
    let mut received = Vec::new();
    silent.read_to_end(&mut received).unwrap();
    assert!(String::from_utf8_lossy(&received).contains("\"version\""));

    // The Daemon still serves everyone else.
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("anything"), Vec::<String>::new());
}

#[test]
fn an_extension_installed_or_removed_while_the_daemon_runs_is_picked_up() {
    let root = Daemon::home();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("kotodama"), Vec::<String>::new());

    install(
        &daemon.root,
        "bookmarks",
        "",
        &[item("kotodama", "Kotodama")],
    );
    ui.wait_for_titles("kotodama", &["Kotodama"]);

    fs::remove_dir_all(daemon.extension_dir("bookmarks")).unwrap();
    ui.wait_for_titles("kotodama", &[]);
}

#[test]
fn an_extension_a_profile_switch_brings_is_picked_up() {
    let root = Daemon::home();
    // The data directory is a profile: a link to a generation, which a NixOS
    // or home-manager switch replaces with a link to the next. The first has
    // no Extensions at all.
    fs::remove_dir(root.path().join("share")).unwrap();
    fs::create_dir(root.path().join("gen-1")).unwrap();
    install_in(
        &root.path().join("gen-2/kotodama/extensions/bookmarks"),
        "bookmarks",
        "",
        &[item("kotodama", "Kotodama")],
    );
    std::os::unix::fs::symlink("gen-1", root.path().join("share")).unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("kotodama"), Vec::<String>::new());

    // As `ln -sfn` and Nix do it: a new link renamed over the old one.
    let (new, share) = (
        daemon.root.path().join("share.new"),
        daemon.root.path().join("share"),
    );
    std::os::unix::fs::symlink("gen-2", &new).unwrap();
    fs::rename(&new, &share).unwrap();
    ui.wait_for_titles("kotodama", &["Kotodama"]);
}

#[test]
fn the_history_survives_a_restart_and_the_clear_action_empties_it() {
    let root = Daemon::home();
    install(
        &root,
        "bookmarks",
        "",
        &[item("kotlin", "Kotlin"), item("kotodama", "Kotodama")],
    );
    let mut daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    ui.wait_for_titles("kot", &["Kotlin", "Kotodama"]);
    for _ in 0..3 {
        ui.titles("kot");
        ui.run("bookmarks", "kotodama", "open");
    }
    assert_eq!(ui.titles("kot"), ["Kotodama", "Kotlin"]);
    let history = daemon.root.path().join("state/kotodama/history.json");
    wait_for("the third pick on disk", || {
        fs::read_to_string(&history)
            .ok()
            .filter(|text| text.contains(r#""choice":["bookmarks","kotodama"]"#))
    });

    drop(ui);
    daemon.restart();
    let mut ui = daemon.connect();
    ui.wait_for_titles("kot", &["Kotodama", "Kotlin"]);

    ui.titles("clear history");
    ui.run("kotodama", "clear-history", "run");
    // The query typed is ranked again for the cleared History first.
    ui.wait_for_titles("kot", &["Kotlin", "Kotodama"]);
    wait_for("the empty history on disk", || {
        fs::read_to_string(&history)
            .ok()
            .filter(|text| !text.contains("kotodama"))
    });
}

#[test]
fn aliases_and_quicklinks_follow_the_config_directory_without_a_restart() {
    let root = Daemon::home();
    install(
        &root,
        "bookmarks",
        "",
        &[item("kotlin", "Kotlin"), item("kotodama", "Kotodama")],
    );
    let config = root.path().join("config/kotodama/keywords.toml");
    fs::write(
        &config,
        "[aliases]\nk = { extension = \"bookmarks\", item = \"kotlin\" }\n",
    )
    .unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    wait_for("the Alias", || {
        ui.titles("k")
            .first()
            .is_some_and(|title| title == "Kotlin")
            .then_some(())
    });

    fs::write(
        &config,
        "[aliases]\nk = { extension = \"bookmarks\", item = \"kotodama\" }\n\
         [quicklinks.gh]\ntitle = \"GitHub\"\nurl = \"https://github.com\"\n",
    )
    .unwrap();
    wait_for("the changed Alias", || {
        ui.titles("k")
            .first()
            .is_some_and(|title| title == "Kotodama")
            .then_some(())
    });
    ui.wait_for_titles("gh", &["GitHub"]);
}

#[test]
fn the_usage_log_is_written_once_the_config_turns_it_on() {
    let root = Daemon::home();
    install(
        &root,
        "bookmarks",
        "",
        &[item("kotlin", "Kotlin"), item("kotodama", "Kotodama")],
    );
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    ui.wait_for_titles("kot", &["Kotlin", "Kotodama"]);
    ui.run("bookmarks", "kotodama", "open");
    let state = daemon.root.path().join("state/kotodama");
    wait_for("the pick on disk", || {
        state.join("history.json").exists().then_some(())
    });
    let log = state.join("usage.jsonl");
    assert!(!log.exists(), "the usage log is off by default");

    fs::write(
        daemon.root.path().join("config/kotodama/history.toml"),
        "[history]\nusage_log = true\n",
    )
    .unwrap();
    wait_for("the usage log", || {
        ui.titles("kotl");
        log.exists().then_some(())
    });
    ui.run("bookmarks", "kotlin", "open");
    let pick = wait_for("the pick in the usage log", || {
        fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .find(|line| line.contains(r#""kind":"pick""#))
            .map(str::to_owned)
    });
    let pick: serde_json::Value = serde_json::from_str(&pick).unwrap();
    assert_eq!(pick["query"], "kotl");
    assert_eq!(pick["extension"], "bookmarks");
    assert_eq!(pick["id"], "kotlin");
    assert_eq!(pick["action"], "open");
    assert_eq!(pick["row"], 0);
}

/// Under systemd the prefix starts each program in a scope of its own, so
/// stopping the Daemon doesn't take it along.
#[test]
fn a_quicklinks_command_starts_through_the_launch_prefix() {
    let root = Daemon::home();
    let out = root.path().join("wrapped");
    let wrapper = root.path().join("wrap");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}.tmp\nmv {0}.tmp {0}\n",
            out.display()
        ),
    )
    .unwrap();
    Command::new("chmod")
        .args(["+x", wrapper.to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(
        root.path().join("config/kotodama/links.toml"),
        "[quicklinks.say]\ntitle = \"Say\"\ncommand = [\"echo\", \"{query}\"]\n",
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodamad"));
    with_home(&mut command, root.path()).env(
        "KOTODAMA_LAUNCH_PREFIX",
        format!("{} --scope", wrapper.display()),
    );
    let daemon = Daemon {
        child: command.spawn().unwrap(),
        root,
    };
    let mut ui = daemon.connect();
    wait_for("the Quicklink", || {
        ui.titles("say hi")
            .first()
            .is_some_and(|title| title.starts_with("Say"))
            .then_some(())
    });
    ui.run("kotodama", "quicklink:say", "open");

    wait_for("the wrapped command", || {
        (fs::read_to_string(&out).ok()? == "--scope\necho\nhi\n").then_some(())
    });
}

/// systemd holds the socket while the Daemon is stopped, so a `kotodama toggle`
/// then waits in its queue and starts the next Daemon, which must serve it.
#[cfg(target_os = "linux")]
#[test]
fn under_socket_activation_the_daemon_serves_the_socket_it_is_handed_and_the_next_one_takes_over() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[item("kotodama", "Kotodama")]);
    // Bound at the Daemon's own path: a Daemon that bound it itself would find
    // it answering and refuse to start.
    let socket = root.path().join("runtime/kotodama.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let mut daemon = Daemon {
        child: spawn_activated(root.path(), &listener),
        root,
    };
    let mut ui = daemon.connect();
    ui.wait_for_titles("kotodama", &["Kotodama"]);
    drop(ui);

    let _ = daemon.child.kill();
    let _ = daemon.child.wait();
    // Connects while no Daemon runs: the socket queues it.
    let waiting = UnixStream::connect(&socket).unwrap();
    daemon.child = spawn_activated(daemon.root.path(), &listener);
    waiting.set_read_timeout(Some(TIMEOUT)).unwrap();
    let channel = Channel::handshake(
        waiting.try_clone().unwrap(),
        waiting,
        &Hello::new(&WireFormat::ALL),
    )
    .unwrap();
    let mut ui = Client { channel };
    ui.wait_for_titles("kotodama", &["Kotodama"]);
    assert!(socket.exists(), "the Daemon left systemd's socket in place");
}
