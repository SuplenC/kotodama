//! The `kotodamad` binary, as a Front end sees it: over its socket, with real
//! Extension processes. The test Extension is a bash script, so these need
//! bash 4+, which the dev environment provides.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use kotodama_extension::testing;
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
        for dir in ["runtime", EXTENSIONS, "etc/kotodama", "share"] {
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
        Client {
            channel: testing::connect(&socket, TIMEOUT),
            shown: 0,
        }
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
        // A test run from a shell an older Kotodama started still inherits
        // its prefix, and its KOTODAMA_LAUNCH_SCOPED would hide the prefix a
        // test sets.
        .env_remove(LAUNCH_PREFIX)
        .env_remove(LAUNCH_SCOPED)
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

/// The user's extension directory in a home.
const EXTENSIONS: &str = "config/kotodama/extensions";

fn extension_dir(root: &TempDir, name: &str) -> PathBuf {
    root.path().join(EXTENSIONS).join(name)
}

/// Installs the recording test Extension as `name`, indexing `items`.
fn install(root: &TempDir, name: &str, manifest_extra: &str, items: &[Item]) {
    install_in(&root.path().join(EXTENSIONS), name, manifest_extra, items);
}

/// Installs the recording test Extension as `name` in the extension
/// directory `extensions`. The Manifest goes in last, so a Daemon watching
/// finds the Extension whole.
fn install_in(extensions: &Path, name: &str, manifest_extra: &str, items: &[Item]) {
    let dir = extensions.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(), dir.join("extension.sh")).unwrap();
    fs::write(dir.join("items.json"), serde_json::to_string(items).unwrap()).unwrap();
    testing::install(
        extensions,
        name,
        &format!("name = \"{name}\"\ncommand = [\"extension.sh\"]\n{manifest_extra}"),
    );
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
    /// The version of the last Results read.
    shown: u32,
}

impl Client {
    fn send(&mut self, message: &FromFrontEnd) {
        self.channel.send(message).unwrap();
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
        self.send(&FromFrontEnd::Query { text: query.into() });
        let (version, titles) = self.until(|message| match message {
            ToFrontEnd::Results { version, results, .. } => {
                Some((version, results.into_iter().map(|row| row.item.title).collect()))
            }
            _ => None,
        });
        self.shown = version;
        titles
    }

    fn run(&mut self, extension: &str, id: &str, action: &str) {
        self.send(&FromFrontEnd::RunAction {
            result: ResultRef {
                extension: extension.into(),
                id: id.into(),
            },
            shown: self.shown,
            action: ActionRef {
                extension: extension.into(),
                id: action.into(),
            },
            arguments: BTreeMap::default(),
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
        app_only: false,
        weak_keywords: Vec::new(),
        actions: vec![Action::new("open", "Open")],
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
    Command::new("kill").args(["-9", pid.trim()]).status().unwrap();

    // The daemon keeps answering while the Extension is down, and says so.
    ui.until(|message| match message {
        ToFrontEnd::Status { down, .. } if down == ["bookmarks"] => Some(()),
        _ => None,
    });
    assert_eq!(ui.titles("thing"), ["Other thing"]);

    ui.until(|message| match message {
        ToFrontEnd::Status { down, .. } if down.is_empty() => Some(()),
        _ => None,
    });
    ui.wait_for_titles("kotodama", &["Kotodama"]);
    assert!(pid_file.exists(), "a new process started");
}

#[test]
fn a_config_change_reaches_the_extension_without_a_restart_and_a_broken_file_keeps_the_last_good_config() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    let user_config = root.path().join("config/kotodama");
    fs::write(user_config.join("a.toml"), "[extensions.bookmarks]\nv = 1\n").unwrap();
    let daemon = Daemon::start(root);
    let dir = daemon.extension_dir("bookmarks");
    wait_for("the first config", || {
        (received(&dir) == [r#"{"config":{"v":1}}"#]).then_some(())
    });

    fs::write(user_config.join("a.toml"), "[extensions.bookmarks]\nv = 2\n").unwrap();
    wait_for("the reloaded config", || {
        (received(&dir).last().map(String::as_str) == Some(r#"{"config":{"v":2}}"#)).then_some(())
    });

    // While a file is broken the whole last good config stays, so another
    // file's change doesn't reach the Extension until it is fixed.
    fs::write(user_config.join("c.toml"), "this is not toml").unwrap();
    thread::sleep(Duration::from_millis(300));
    fs::write(user_config.join("b.toml"), "[extensions.bookmarks]\nv = 3\n").unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(received(&dir).last().map(String::as_str), Some(r#"{"config":{"v":2}}"#));
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

/// Saves `text` to `path` as an editor does: a new file renamed over the old.
fn save_by_rename(path: &Path, text: &str) {
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, text).unwrap();
    fs::rename(&temp, path).unwrap();
}

fn last_config_is(dir: &Path, v: i64) -> Option<()> {
    let want = format!(r#"{{"config":{{"v":{v}}}}}"#);
    (received(dir).last() == Some(&want)).then_some(())
}

#[test]
fn an_edit_to_the_file_a_symlinked_config_file_leads_to_reloads_the_config() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    // As KubaOS links the user's file in from a directory of its own.
    let elsewhere = root.path().join("config/kubaos/kotodama");
    fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("user.toml");
    fs::write(&target, "[extensions.bookmarks]\nv = 1\n").unwrap();
    symlink(&target, root.path().join("config/kotodama/user.toml")).unwrap();
    let daemon = Daemon::start(root);
    let dir = daemon.extension_dir("bookmarks");
    wait_for("the first config", || last_config_is(&dir, 1));

    save_by_rename(&target, "[extensions.bookmarks]\nv = 2\n");
    wait_for("the config after a save by rename", || last_config_is(&dir, 2));

    fs::write(&target, "[extensions.bookmarks]\nv = 3\n").unwrap();
    wait_for("the config after a write in place", || last_config_is(&dir, 3));
}

#[test]
fn a_symlinked_config_file_pointed_elsewhere_is_followed_there() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    let elsewhere = root.path().join("elsewhere");
    let further = root.path().join("further");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(&further).unwrap();
    let first = elsewhere.join("first.toml");
    let second = further.join("second.toml");
    fs::write(&first, "[extensions.bookmarks]\nv = 1\n").unwrap();
    fs::write(&second, "[extensions.bookmarks]\nv = 2\n").unwrap();
    // A link to a link: the config file leads to `hop`, which leads on.
    let hop = elsewhere.join("hop.toml");
    symlink(&first, &hop).unwrap();
    symlink(&hop, root.path().join("config/kotodama/zz.toml")).unwrap();
    let daemon = Daemon::start(root);
    let dir = daemon.extension_dir("bookmarks");
    wait_for("the first config", || last_config_is(&dir, 1));

    // The link in the middle is pointed at a file in another directory.
    let new_hop = elsewhere.join("hop.toml.new");
    symlink(&second, &new_hop).unwrap();
    fs::rename(&new_hop, &hop).unwrap();
    wait_for("the config the link leads to now", || last_config_is(&dir, 2));

    // Its new target is watched, and the old one counts for nothing.
    save_by_rename(&second, "[extensions.bookmarks]\nv = 3\n");
    wait_for("the config after an edit of the new target", || last_config_is(&dir, 3));
    fs::write(&first, "[extensions.bookmarks]\nv = 9\n").unwrap();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(last_config_is(&dir, 3), Some(()), "{:?}", received(&dir));
}

/// Each thread of process `pid` with how often it has woken: its voluntary context switches.
#[cfg(target_os = "linux")]
fn wakeups_by_thread(pid: u32) -> BTreeMap<String, u64> {
    let mut wakeups = BTreeMap::new();
    for task in fs::read_dir(format!("/proc/{pid}/task")).unwrap().flatten() {
        let status = fs::read_to_string(task.path().join("status")).unwrap_or_default();
        let count = status
            .lines()
            .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
            .and_then(|count| count.trim().parse::<u64>().ok());
        if let Some(count) = count {
            wakeups.insert(task.file_name().to_string_lossy().into_owned(), count);
        }
    }
    wakeups
}

/// How often the threads of process `pid` woke since `before`: a thread started since counts
/// whole, one that ended counts for nothing.
#[cfg(target_os = "linux")]
fn woke_since(pid: u32, before: &BTreeMap<String, u64>) -> u64 {
    wakeups_by_thread(pid)
        .iter()
        .map(|(thread, now)| now.saturating_sub(before.get(thread).copied().unwrap_or(0)))
        .sum()
}

/// Longer than any settle of the Daemon's start, the hot Items' one second included.
#[cfg(target_os = "linux")]
const IDLE_FOR: Duration = Duration::from_millis(1500);

#[cfg(target_os = "linux")]
#[test]
fn an_idle_daemon_sleeps_while_its_config_and_extensions_are_read() {
    // Regression: #240.
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    // As KubaOS lays the config out: the user's file linked in from a directory of its own.
    let elsewhere = root.path().join("config/kubaos/kotodama");
    fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("user.toml");
    fs::write(&target, "[extensions.bookmarks]\nv = 1\n").unwrap();
    symlink(&target, root.path().join("config/kotodama/user.toml")).unwrap();
    let daemon = Daemon::start(root);
    let dir = daemon.extension_dir("bookmarks");
    wait_for("the first config", || last_config_is(&dir, 1));
    let pid = daemon.child.id();
    let start = Instant::now();
    loop {
        let before = wakeups_by_thread(pid);
        thread::sleep(IDLE_FOR);
        if woke_since(pid, &before) == 0 {
            break;
        }
        assert!(start.elapsed() < TIMEOUT, "the Daemon never went idle after its start");
    }
    let before = wakeups_by_thread(pid);

    // Others reading what the Daemon watches, as a shell or an editor does.
    for _ in 0..50 {
        let _ = fs::read(&target).unwrap();
        let _ = fs::read_dir(daemon.root.path().join("config/kotodama"))
            .unwrap()
            .count();
        let _ = fs::read(dir.join("manifest.toml")).unwrap();
        thread::sleep(Duration::from_millis(40));
    }

    let woke = woke_since(pid, &before);
    assert!(woke <= 2, "the idle Daemon woke {woke} times in 2 s");
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
    testing::install(
        &root.path().join(EXTENSIONS),
        "silent",
        "name = \"silent\"\ncommand = [\"hang.sh\"]\n",
    );
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();

    ui.until(|message| match message {
        ToFrontEnd::Status { down, .. } if down == ["silent"] => Some(()),
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
    wait_for("the Extension to die with the Daemon", || (!alive()).then_some(()));
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

    install(&daemon.root, "bookmarks", "", &[item("kotodama", "Kotodama")]);
    ui.wait_for_titles("kotodama", &["Kotodama"]);

    fs::remove_dir_all(daemon.extension_dir("bookmarks")).unwrap();
    ui.wait_for_titles("kotodama", &[]);
}

/// What each problem in a Status is about.
fn abouts(problems: &[Problem]) -> Vec<&str> {
    problems.iter().map(|problem| problem.about.as_str()).collect()
}

#[test]
fn front_ends_hear_what_is_wrong_and_an_extension_starts_once_its_missing_program_appears() {
    let root = Daemon::home();
    testing::install(&root.path().join(EXTENSIONS), "broken", "name = \"broken\"\n");
    install(&root, "late", "", &[item("late", "Late thing")]);
    let program = extension_dir(&root, "late").join("extension.sh");
    fs::remove_file(&program).unwrap();
    fs::write(
        root.path().join("config/kotodama/a.toml"),
        "[history]\nhalf_life_days = 0\n",
    )
    .unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();

    let problems = ui.until(|message| match message {
        ToFrontEnd::Status { problems, .. } if problems.len() == 3 => Some(problems),
        _ => None,
    });
    let abouts = abouts(&problems);
    assert!(abouts[0].starts_with("history.half_life_days"), "{abouts:?}");
    assert!(abouts[1].ends_with("broken/manifest.toml"), "{abouts:?}");
    assert_eq!(abouts[2], "extension late");

    fs::copy(fixture(), &program).unwrap();
    ui.until(|message| match message {
        ToFrontEnd::Status { problems, .. } if problems.len() == 2 => Some(()),
        _ => None,
    });
    ui.wait_for_titles("thing", &["Late thing"]);
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
        &root.path().join("gen-2/kotodama/extensions"),
        "bookmarks",
        "",
        &[item("kotodama", "Kotodama")],
    );
    symlink("gen-1", root.path().join("share")).unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("kotodama"), Vec::<String>::new());

    // As `ln -sfn` and Nix do it: a new link renamed over the old one.
    let (new, share) = (daemon.root.path().join("share.new"), daemon.root.path().join("share"));
    symlink("gen-2", &new).unwrap();
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
fn an_alias_that_cannot_run_reaches_the_front_end_as_a_row_that_decodes() {
    let root = Daemon::home();
    fs::write(
        root.path().join("config/kotodama/keywords.toml"),
        "[aliases]\nn = { extension = \"notes\", item = \"todo\" }\n",
    )
    .unwrap();
    let daemon = Daemon::start(root);
    let mut ui = daemon.connect();

    ui.send(&FromFrontEnd::Query { text: "n".into() });
    let results = ui.until(|message| match message {
        ToFrontEnd::Results { results, .. } => Some(results),
        _ => None,
    });

    let row = &results[0];
    assert_eq!(row.alias.as_deref(), Some("n"));
    assert_eq!(row.unavailable.as_deref(), Some("No Extension notes"));
    assert_eq!(row.item.actions, [], "nothing to run");
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
    wait_for("the pick on disk", || state.join("history.json").exists().then_some(()));
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
    with_home(&mut command, root.path()).env("KOTODAMA_LAUNCH_PREFIX", format!("{} --scope", wrapper.display()));
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

/// The launch prefix is for Extensions: a program the Daemon starts for the
/// user, and all it starts in turn, must not take it for its own.
#[test]
fn a_quicklinks_command_inherits_no_launch_prefix() {
    let root = Daemon::home();
    let out = root.path().join("seen");
    let seen = root.path().join("see");
    fs::write(
        &seen,
        format!(
            "#!/bin/sh\necho \"${{KOTODAMA_LAUNCH_PREFIX-unset}} ${{KOTODAMA_LAUNCH_SCOPED-unset}}\" > {0}.tmp\nmv {0}.tmp {0}\n",
            out.display()
        ),
    )
    .unwrap();
    Command::new("chmod")
        .args(["+x", seen.to_str().unwrap()])
        .status()
        .unwrap();
    fs::write(
        root.path().join("config/kotodama/links.toml"),
        format!(
            "[quicklinks.see]\ntitle = \"See\"\ncommand = [\"{}\"]\n",
            seen.display()
        ),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodamad"));
    with_home(&mut command, root.path()).env(LAUNCH_PREFIX, "env");
    let daemon = Daemon {
        child: command.spawn().unwrap(),
        root,
    };
    let mut ui = daemon.connect();
    wait_for("the Quicklink", || {
        ui.titles("see")
            .first()
            .is_some_and(|title| title.starts_with("See"))
            .then_some(())
    });
    ui.run("kotodama", "quicklink:see", "open");

    wait_for("the command", || {
        (fs::read_to_string(&out).ok()? == "unset unset\n").then_some(())
    });
}

/// With no prefix of its own and a user manager to make scopes, the Daemon
/// hands its Extensions the prefix that makes one.
#[cfg(target_os = "linux")]
#[test]
fn an_extension_gets_the_scope_prefix_when_there_is_a_user_manager() {
    let root = Daemon::home();
    fs::create_dir(root.path().join("runtime/systemd")).unwrap();
    let _manager = std::os::unix::net::UnixListener::bind(root.path().join("runtime/systemd/private")).unwrap();
    install(&root, "bookmarks", "", &[item("fir", "Fir")]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodamad"));
    // A prefix the user set in the shell running the tests would replace the
    // scope prefix.
    with_home(&mut command, root.path()).env_remove(LAUNCH_PREFIX);
    let daemon = Daemon {
        child: command.spawn().unwrap(),
        root,
    };
    let mut ui = daemon.connect();
    ui.wait_for_titles("fir", &["Fir"]);

    let pid = fs::read_to_string(daemon.extension_dir("bookmarks").join("pid")).unwrap();
    let environ = fs::read(format!("/proc/{}/environ", pid.trim())).unwrap();
    let vars: Vec<String> = environ
        .split(|&byte| byte == 0)
        .map(|var| String::from_utf8_lossy(var).into_owned())
        .collect();
    assert!(vars.contains(&format!("{LAUNCH_PREFIX}={SCOPE_PREFIX}")), "{vars:?}");
    assert!(vars.contains(&format!("{LAUNCH_SCOPED}=1")), "{vars:?}");
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
    let mut ui = Client {
        channel: testing::front_end(waiting, TIMEOUT),
        shown: 0,
    };
    ui.wait_for_titles("kotodama", &["Kotodama"]);
    assert!(socket.exists(), "the Daemon left systemd's socket in place");
}

// Peers on other protocol versions. Protocol 1.0 is the first released
// version, and every Daemon after it keeps talking to a peer built for it.
// The 1.0 peers here are recorded: the recording Extension's hello names 1.0
// and speaks JSON, and the Front end below sends the very bytes 1.0 pinned.

/// Starts the Daemon with its log in `kotodamad.log` in its home.
fn start_logged(root: TempDir) -> Daemon {
    let log = fs::File::create(root.path().join("kotodamad.log")).unwrap();
    let child = with_home(&mut Command::new(env!("CARGO_BIN_EXE_kotodamad")), root.path())
        .stderr(log)
        .spawn()
        .unwrap();
    Daemon { child, root }
}

fn log(daemon: &Daemon) -> String {
    fs::read_to_string(daemon.root.path().join("kotodamad.log")).unwrap_or_default()
}

fn connect_raw(daemon: &Daemon) -> UnixStream {
    let socket = daemon.root.path().join("runtime/kotodama.sock");
    let stream = wait_for("the socket", || UnixStream::connect(&socket).ok());
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream
}

#[test]
fn a_front_end_and_an_extension_built_for_protocol_1_0_work_with_this_daemon() {
    use std::io::BufReader;

    let root = Daemon::home();
    install(&root, "bookmarks", "", &[item("fir", "Fir")]);
    let daemon = Daemon::start(root);
    let mut stream = connect_raw(&daemon);
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut buf = Vec::new();

    frame::write(&mut stream, br#"{"version":1,"minor":0,"formats":["protobuf","json"]}"#).unwrap();
    assert!(frame::read(&mut reader, &mut buf).unwrap());
    let hello: Hello = WireFormat::Json.decode(&buf).unwrap();
    assert!(hello.protocol() >= Version::new(1, 0), "{hello:?}");

    // `FromFrontEnd::Query { text: "fir" }` as protocol 1.0 encodes it in
    // protobuf, copied from tests/layouts/1.0.txt in kotodama-protocol.
    let query = [0x0a, 0x05, 0x0a, 0x03, b'f', b'i', b'r'];
    wait_for("Fir in the Results", || {
        frame::write(&mut stream, &query).unwrap();
        loop {
            assert!(frame::read(&mut reader, &mut buf).unwrap(), "the Daemon hung up");
            if let ToFrontEnd::Results { results, .. } = WireFormat::Protobuf.decode(&buf).unwrap() {
                let titles: Vec<String> = results.into_iter().map(|row| row.item.title).collect();
                return (titles == ["Fir"]).then_some(());
            }
        }
    });
}

#[test]
fn a_front_end_built_for_a_newer_major_version_is_turned_away_with_the_reason() {
    use std::io::Read;

    let daemon = start_logged(Daemon::home());
    let newer = PROTOCOL_VERSION.major + 1;
    let mut stream = connect_raw(&daemon);
    frame::write(
        &mut stream,
        format!(r#"{{"version":{newer},"minor":0,"formats":["json"]}}"#).as_bytes(),
    )
    .unwrap();

    // The Daemon's hello, from which the Front end can tell the same, then
    // the end of the stream.
    let mut received = Vec::new();
    stream.read_to_end(&mut received).unwrap();
    assert!(String::from_utf8_lossy(&received).contains("\"version\""));
    let reason =
        format!("the peer needs protocol {newer}.0, newer than this Daemon's {PROTOCOL_VERSION}: update Kotodama");
    wait_for("the reason in the log", || log(&daemon).contains(&reason).then_some(()));

    // The Daemon still serves everyone else.
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("anything"), Vec::<String>::new());
}

#[test]
fn an_extension_built_for_a_newer_major_version_is_turned_away_with_the_reason() {
    let root = Daemon::home();
    install(&root, "future", "", &[item("fir", "Fir")]);
    let script = extension_dir(&root, "future").join("extension.sh");
    let newer = PROTOCOL_VERSION.major + 1;
    let recorded = fs::read_to_string(&script).unwrap();
    let rewritten = recorded.replace(r#""version":1,"minor":0"#, &format!(r#""version":{newer},"minor":0"#));
    assert_ne!(recorded, rewritten, "the recording Extension names its version");
    fs::write(&script, rewritten).unwrap();

    let daemon = start_logged(root);
    let reason = format!(
        "future: the peer needs protocol {newer}.0, newer than this Daemon's {PROTOCOL_VERSION}: update Kotodama"
    );
    wait_for("the reason in the log", || log(&daemon).contains(&reason).then_some(()));
    let mut ui = daemon.connect();
    assert_eq!(ui.titles("fir"), Vec::<String>::new());
}

#[test]
fn an_extension_is_told_in_words_why_a_message_of_its_was_rejected() {
    let root = Daemon::home();
    install(&root, "bookmarks", "", &[]);
    fs::write(
        extension_dir(&root, "bookmarks").join("items.json"),
        r#"[{"id":"a","kind":"url","url":"https://a","title":"A","actions":[]}]"#,
    )
    .unwrap();
    let mut theirs = item("b", "B");
    theirs.kind = ItemKind::Namespaced(NamespacedKind::new("other.note").unwrap());
    install(&root, "notes", "", &[theirs]);
    let daemon = Daemon::start(root);

    for (extension, reason) in [
        (
            "bookmarks",
            "Item `a` has no Actions; it needs at least its primary Action",
        ),
        (
            "notes",
            "Item `b` has another Extension's kind `other.note`; name your own kinds `notes.<kind>`",
        ),
    ] {
        let dir = daemon.extension_dir(extension);
        let rejection = wait_for(&format!("{extension}'s rejection"), || {
            received(&dir)
                .into_iter()
                .find(|line| line.starts_with(r#"{"rejected":{"reason":"#))
        });
        assert!(rejection.contains(reason), "{rejection}");
    }
}
