//! Drives the apps Extension the way the Daemon does: over its stdio, with its
//! XDG data directories pointed at a temporary directory.

use std::collections::BTreeMap;
use std::fs;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;
use tempfile::TempDir;

/// Long enough for a change on disk to be noticed and settle.
const WAIT: Duration = Duration::from_secs(5);

/// Declared first, so the Extension is killed before its directory goes.
struct Apps {
    extension: Spawned,
    dir: TempDir,
}

impl Deref for Apps {
    type Target = Spawned;
    fn deref(&self) -> &Spawned {
        &self.extension
    }
}

impl DerefMut for Apps {
    fn deref_mut(&mut self) -> &mut Spawned {
        &mut self.extension
    }
}

impl Apps {
    /// Starts the Extension on `dir`, as from [`data_dirs`], and does the
    /// handshake.
    fn start(dir: TempDir) -> Self {
        Apps::start_with(dir, &[])
    }

    /// Starts the Extension with `env` added to its environment.
    fn start_with(dir: TempDir, env: &[(&str, &str)]) -> Self {
        let extension = Spawned::start(
            Command::new(env!("CARGO_BIN_EXE_kotodama-apps"))
                .env("XDG_DATA_HOME", dir.path().join("home"))
                .env("XDG_DATA_DIRS", dir.path().join("system"))
                .env("XDG_CURRENT_DESKTOP", "Hyprland")
                .env("LANG", "pl_PL.UTF-8")
                .env_remove("LC_ALL")
                .env_remove("LC_MESSAGES")
                // Started directly, not in a scope of the user's systemd.
                .env("KOTODAMA_LAUNCH_PREFIX", "")
                .envs(env.iter().copied()),
            WAIT,
        );
        Apps { dir, extension }
    }

    /// Starts the Extension and sends it `config`.
    fn configured(dir: TempDir, config: Table) -> Self {
        let mut apps = Apps::start(dir);
        apps.send(ToExtension::Config(config));
        apps
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs `action` on the app `id` and returns the outcome. The withdrawal
    /// of an earlier run's pick, from an app that exited, may come first.
    fn run(&mut self, id: &str, action: &str) -> Outcome {
        self.run_as(7, id, action)
    }

    /// Runs `action` on the app `id` as `invocation`.
    fn run_as(&mut self, invocation: u64, id: &str, action: &str) -> Outcome {
        let item = ItemRef {
            id: id.into(),
            kind: ItemKind::App {
                desktop_id: format!("{id}.desktop"),
            },
        };
        self.start_run(invocation, action, item, BTreeMap::new());
        self.outcome(invocation, |message| {
            matches!(message, FromExtension::WithdrawPick { .. })
        })
    }
}

/// A data directory for the user and one for the system, both empty.
fn data_dirs() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("home/applications")).unwrap();
    fs::create_dir_all(dir.path().join("system/applications")).unwrap();
    dir
}

/// Writes a `.desktop` file under `<dir>/<data>/applications/`.
fn install(dir: &Path, data: &str, file: &str, text: &str) -> PathBuf {
    let path = dir.join(data).join("applications").join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn app(name: &str, exec: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n")
}

fn titles(items: &[Item]) -> Vec<&str> {
    let mut titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    titles.sort();
    titles
}

fn find<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items
        .iter()
        .find(|item| item.id == id)
        .unwrap_or_else(|| panic!("no Item `{id}` in {items:?}"))
}

fn strings(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|s| Value::String((*s).into())).collect())
}

#[test]
fn each_desktop_file_becomes_an_app_item() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "firefox.desktop",
        "[Desktop Entry]\nType=Application\nName=Firefox\nGenericName=Web Browser\n\
         Icon=firefox\nExec=firefox %u\n",
    );
    let apps = Apps::configured(dir, Table::new());

    let items = apps.added();
    assert_eq!(items.len(), 1);
    let firefox = &items[0];
    assert_eq!(firefox.id, "firefox");
    assert_eq!(
        firefox.kind,
        ItemKind::App {
            desktop_id: "firefox.desktop".into()
        }
    );
    // Shares its History with the same app from any other Extension.
    assert_eq!(firefox.identity.as_deref(), Some("app:firefox.desktop"));
    assert_eq!(firefox.title, "Firefox");
    assert_eq!(firefox.subtitle.as_deref(), Some("Web Browser"));
    assert_eq!(firefox.icon.as_deref(), Some("firefox"));
    assert_eq!(firefox.actions[0].id, "open");
}

#[test]
fn keywords_generic_names_and_ids_make_an_app_findable() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "org.gnome.Nautilus.desktop",
        "[Desktop Entry]\nType=Application\nName=Files\nName[pl]=Pliki\n\
         GenericName=File Manager\nComment=Access and organize files\n\
         Keywords=folder;explorer;\nKeywords[pl]=katalog;\nExec=nautilus --new-window\n",
    );
    let apps = Apps::configured(dir, Table::new());

    let items = apps.added();
    let files = find(&items, "org.gnome.Nautilus");
    // The Extension runs in Polish.
    assert_eq!(files.title, "Pliki");
    assert_eq!(files.keywords, ["katalog", "File Manager", "org.gnome.Nautilus"]);
}

#[test]
fn an_app_without_a_generic_name_shows_its_comment() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "btop.desktop",
        "[Desktop Entry]\nType=Application\nName=btop++\nComment=Resource monitor\nExec=btop\n",
    );
    let apps = Apps::configured(dir, Table::new());
    assert_eq!(apps.added()[0].subtitle.as_deref(), Some("Resource monitor"));
}

#[test]
fn desktop_actions_are_extra_actions_after_open() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "firefox.desktop",
        "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n\
         Actions=new-window;new-private-window;\n\
         [Desktop Action new-window]\nName=New Window\nExec=firefox --new-window %u\n\
         [Desktop Action new-private-window]\nName=New Private Window\n\
         Name[pl]=Nowe okno prywatne\nExec=firefox --private-window %u\n",
    );
    let apps = Apps::configured(dir, Table::new());

    let items = apps.added();
    let actions: Vec<(&str, &str)> = items[0]
        .actions
        .iter()
        .map(|action| (action.id.as_str(), action.title.as_str()))
        .collect();
    assert_eq!(
        actions,
        [
            ("open", "Open"),
            ("desktop-action:new-window", "New Window"),
            ("desktop-action:new-private-window", "Nowe okno prywatne"),
        ]
    );
}

#[test]
fn entries_that_ask_not_to_be_shown_are_left_out() {
    let dir = data_dirs();
    let system = |file: &str, text: &str| {
        install(dir.path(), "system", file, text);
    };
    system("shown.desktop", &app("Shown", "shown"));
    system(
        "nodisplay.desktop",
        "[Desktop Entry]\nType=Application\nName=NoDisplay\nExec=x\nNoDisplay=true\n",
    );
    system(
        "hidden.desktop",
        "[Desktop Entry]\nType=Application\nName=Hidden\nExec=x\nHidden=true\n",
    );
    system(
        "gnome-only.desktop",
        "[Desktop Entry]\nType=Application\nName=GNOME only\nExec=x\nOnlyShowIn=GNOME;\n",
    );
    system(
        "hyprland-only.desktop",
        "[Desktop Entry]\nType=Application\nName=Hyprland only\nExec=x\nOnlyShowIn=GNOME;Hyprland;\n",
    );
    system(
        "not-hyprland.desktop",
        "[Desktop Entry]\nType=Application\nName=Not Hyprland\nExec=x\nNotShowIn=Hyprland;\n",
    );
    system(
        "uninstalled.desktop",
        "[Desktop Entry]\nType=Application\nName=Uninstalled\nExec=x\nTryExec=/nonexistent/x\n",
    );
    system(
        "installed.desktop",
        "[Desktop Entry]\nType=Application\nName=Installed\nExec=sh\nTryExec=sh\n",
    );
    system(
        "link.desktop",
        "[Desktop Entry]\nType=Link\nName=Link\nURL=https://example.com\n",
    );
    system("not-an-app.txt", &app("Not a desktop file", "x"));
    let apps = Apps::configured(dir, Table::new());

    assert_eq!(titles(&apps.added()), ["Hyprland only", "Installed", "Shown"]);
}

#[test]
fn the_users_entry_replaces_or_hides_the_systems() {
    let dir = data_dirs();
    install(dir.path(), "system", "editor.desktop", &app("Editor", "ed"));
    install(dir.path(), "home", "editor.desktop", &app("My Editor", "ed"));
    install(dir.path(), "system", "game.desktop", &app("Game", "game"));
    install(
        dir.path(),
        "home",
        "game.desktop",
        "[Desktop Entry]\nType=Application\nName=Game\nExec=game\nHidden=true\n",
    );
    let apps = Apps::configured(dir, Table::new());

    assert_eq!(titles(&apps.added()), ["My Editor"]);
}

#[test]
fn an_entry_in_a_subdirectory_has_the_subdirectory_in_its_id() {
    let dir = data_dirs();
    install(dir.path(), "system", "kde/konsole.desktop", &app("Konsole", "konsole"));
    let apps = Apps::configured(dir, Table::new());

    let items = apps.added();
    assert_eq!(items[0].id, "kde-konsole");
    assert_eq!(
        items[0].kind,
        ItemKind::App {
            desktop_id: "kde-konsole.desktop".into()
        }
    );
}

#[test]
fn installing_an_app_adds_it_without_a_restart() {
    let dir = data_dirs();
    install(dir.path(), "system", "old.desktop", &app("Old", "old"));
    let apps = Apps::configured(dir, Table::new());
    assert_eq!(titles(&apps.added()), ["Old"]);

    install(apps.path(), "system", "new.desktop", &app("New", "new"));
    assert_eq!(titles(&apps.added()), ["New"]);
    apps.quiet();
}

#[test]
fn removing_an_app_removes_it_without_a_restart() {
    let dir = data_dirs();
    let path = install(dir.path(), "system", "gone.desktop", &app("Gone", "gone"));
    install(dir.path(), "system", "kept.desktop", &app("Kept", "kept"));
    let apps = Apps::configured(dir, Table::new());
    assert_eq!(apps.added().len(), 2);

    fs::remove_file(path).unwrap();
    assert_eq!(apps.removed(), ["gone"]);
    apps.quiet();
}

#[test]
fn an_updated_app_replaces_its_item() {
    let dir = data_dirs();
    let path = install(dir.path(), "system", "app.desktop", &app("Before", "app"));
    let apps = Apps::configured(dir, Table::new());
    assert_eq!(titles(&apps.added()), ["Before"]);

    fs::write(path, app("After", "app")).unwrap();
    assert_eq!(titles(&apps.added()), ["After"]);
}

#[test]
fn apps_in_an_applications_directory_created_later_are_found() {
    let dir = data_dirs();
    fs::remove_dir(dir.path().join("home/applications")).unwrap();
    let apps = Apps::configured(dir, Table::new());
    apps.quiet();

    install(apps.path(), "home", "late.desktop", &app("Late", "late"));
    assert_eq!(titles(&apps.added()), ["Late"]);
}

#[test]
fn an_app_a_profile_switch_brings_is_added_without_a_restart() {
    let dir = data_dirs();
    // The system data directory is a profile: a link to a generation, which
    // a NixOS or home-manager switch replaces with a link to the next.
    fs::remove_dir_all(dir.path().join("system")).unwrap();
    install(dir.path(), "gen-1", "old.desktop", &app("Old", "old"));
    install(dir.path(), "gen-2", "old.desktop", &app("Old", "old"));
    install(dir.path(), "gen-2", "new.desktop", &app("New", "new"));
    symlink("gen-1", dir.path().join("system")).unwrap();
    let apps = Apps::configured(dir, Table::new());
    assert_eq!(titles(&apps.added()), ["Old"]);

    // As `ln -sfn` and Nix do it: a new link renamed over the old one.
    symlink("gen-2", apps.path().join("system.new")).unwrap();
    fs::rename(apps.path().join("system.new"), apps.path().join("system")).unwrap();
    assert_eq!(titles(&apps.added()), ["New"]);
    apps.quiet();
}

#[test]
fn config_hides_and_renames_apps_by_desktop_id() {
    let dir = data_dirs();
    install(dir.path(), "system", "nvim.desktop", &app("Neovim wrapper", "nvim"));
    install(dir.path(), "system", "qt5ct.desktop", &app("Qt5 Settings", "qt5ct"));
    install(dir.path(), "system", "qt6ct.desktop", &app("Qt6 Settings", "qt6ct"));
    install(dir.path(), "system", "firefox.desktop", &app("Firefox", "firefox"));
    let apps = Apps::configured(
        dir,
        Table::from([
            ("hidden".into(), strings(&["qt5ct", "qt6ct.desktop"])),
            (
                "names".into(),
                Value::Table(Table::from([("nvim".into(), Value::String("Neovim".into()))])),
            ),
        ]),
    );

    let items = apps.added();
    assert_eq!(titles(&items), ["Firefox", "Neovim"]);
    // Still found by its own name.
    assert!(find(&items, "nvim").keywords.contains(&"Neovim wrapper".to_owned()));
}

#[test]
fn a_config_change_sends_only_what_it_changed() {
    let dir = data_dirs();
    install(dir.path(), "system", "a.desktop", &app("A", "a"));
    install(dir.path(), "system", "b.desktop", &app("B", "b"));
    install(dir.path(), "system", "c.desktop", &app("C", "c"));
    let mut apps = Apps::configured(dir, Table::new());
    assert_eq!(apps.added().len(), 3);

    apps.send(ToExtension::Config(Table::from([
        ("hidden".into(), strings(&["a"])),
        (
            "names".into(),
            Value::Table(Table::from([("b".into(), Value::String("Bee".into()))])),
        ),
    ])));
    assert_eq!(apps.removed(), ["a"]);
    assert_eq!(titles(&apps.added()), ["Bee"]);
    apps.quiet();
}

/// Waits for a file an app writes, and returns what it wrote.
fn wait_for(path: &Path) -> String {
    for _ in 0..100 {
        if let Ok(text) = fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("{} was never written", path.display());
}

#[test]
fn open_starts_the_app_in_its_directory() {
    let dir = data_dirs();
    let work = dir.path().join("work");
    fs::create_dir(&work).unwrap();
    install(
        dir.path(),
        "system",
        "pwd.desktop",
        &format!(
            "[Desktop Entry]\nType=Application\nName=Pwd\nExec=sh -c \"pwd > ran\" %U\nPath={}\n",
            work.display()
        ),
    );
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    assert_eq!(apps.run("pwd", "open"), Outcome::Close);
    assert_eq!(
        wait_for(&work.join("ran")).trim_end(),
        work.canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn terminal_apps_start_in_the_configured_terminal() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "htop.desktop",
        "[Desktop Entry]\nType=Application\nName=htop\nExec=htop -d 10\nTerminal=true\n",
    );
    let out = dir.path().join("terminal");
    // A "terminal" that writes down what it was asked to run.
    let script = format!("printf '%s\\n' \"$@\" > {}", out.display());
    let terminal = ["sh", "-c", script.as_str(), "terminal"];
    let mut apps = Apps::configured(dir, Table::from([("terminal".into(), strings(&terminal))]));
    apps.added();

    assert_eq!(apps.run("htop", "open"), Outcome::Close);
    assert_eq!(wait_for(&out), "htop\n-d\n10\n");
}

#[test]
fn a_desktop_action_runs_its_own_command() {
    let dir = data_dirs();
    let out = dir.path().join("which");
    install(
        dir.path(),
        "system",
        "app.desktop",
        &format!(
            "[Desktop Entry]\nType=Application\nName=App\nExec=sh -c \"echo main > {out}\"\n\
             Actions=other;\n[Desktop Action other]\nName=Other\nExec=sh -c \"echo other > {out}\"\n",
            out = out.display()
        ),
    );
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    assert_eq!(apps.run("app", "desktop-action:other"), Outcome::Close);
    assert_eq!(wait_for(&out), "other\n");
}

#[test]
fn an_action_on_a_missing_app_or_program_fails_with_a_message() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "broken.desktop",
        &app("Broken", "/nonexistent/program"),
    );
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    for (id, action) in [("broken", "open"), ("broken", "desktop-action:nope"), ("gone", "open")] {
        assert!(matches!(apps.run(id, action), Outcome::Failed { .. }), "{id} {action}");
    }
}

/// Under systemd the prefix starts each app in a scope of its own, so stopping
/// the Daemon doesn't take the user's apps with it.
#[test]
fn apps_start_through_the_launch_prefix_and_a_missing_program_still_fails() {
    let dir = data_dirs();
    let out = dir.path().join("wrapped");
    let ran = dir.path().join("ran");
    // A prefix that writes down its own arguments, then runs the rest.
    let wrapper = dir.path().join("wrap");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nshift\nexec \"$@\"\n",
            out.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    install(
        dir.path(),
        "system",
        "app.desktop",
        &app("App", &format!("touch {}", ran.display())),
    );
    install(
        dir.path(),
        "system",
        "broken.desktop",
        &app("Broken", "/nonexistent/program"),
    );
    let mut apps = Apps::start_with(
        dir,
        &[("KOTODAMA_LAUNCH_PREFIX", &format!("{} --scope", wrapper.display()))],
    );
    apps.send(ToExtension::Config(Table::new()));
    apps.added();

    assert_eq!(apps.run("app", "open"), Outcome::Close);
    assert_eq!(wait_for(&out), format!("--scope\ntouch\n{}\n", ran.display()));
    for _ in 0..100 {
        if ran.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(ran.exists(), "the app ran");
    assert!(matches!(apps.run("broken", "open"), Outcome::Failed { .. }));
}

/// The Daemon hands Extensions a prefix that only makes a scope, for those
/// that can't make one themselves; this one makes its own.
#[test]
fn a_prefix_that_only_makes_a_scope_is_passed_over() {
    let dir = data_dirs();
    let out = dir.path().join("wrapped");
    let ran = dir.path().join("ran");
    let wrapper = dir.path().join("wrap");
    fs::write(
        &wrapper,
        format!("#!/bin/sh\necho wrapped > {}\nexec \"$@\"\n", out.display()),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    install(
        dir.path(),
        "system",
        "app.desktop",
        &app("App", &format!("touch {}", ran.display())),
    );
    let mut apps = Apps::start_with(
        dir,
        &[
            ("KOTODAMA_LAUNCH_PREFIX", wrapper.to_str().unwrap()),
            ("KOTODAMA_LAUNCH_SCOPED", "1"),
            // No user manager, so the app starts without a scope.
            ("XDG_RUNTIME_DIR", "/nonexistent"),
        ],
    );
    apps.send(ToExtension::Config(Table::new()));
    apps.added();

    assert_eq!(apps.run("app", "open"), Outcome::Close);
    for _ in 0..100 {
        if ran.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(ran.exists(), "the app ran");
    assert!(!out.exists(), "the app ran through the prefix");
}

/// Writes a shell script named `name` in `dir`, running `body`, and returns
/// its path. Named for its test alone, so no other test's process looks like
/// a copy of it already running.
fn script(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[test]
fn an_app_that_exits_right_after_it_starts_has_its_pick_withdrawn() {
    let dir = data_dirs();
    let oops = script(dir.path(), "kotodama-lone-oops", "exit 0");
    install(dir.path(), "system", "oops.desktop", &app("Oops", &oops));
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    assert_eq!(apps.run_as(41, "oops", "open"), Outcome::Close);
    assert_eq!(
        apps.recv_timeout(WAIT),
        Ok(FromExtension::WithdrawPick { invocation: 41 })
    );
}

#[test]
fn an_app_that_runs_longer_than_withdraw_within_keeps_its_pick() {
    let dir = data_dirs();
    install(dir.path(), "system", "slow.desktop", &app("Slow", "sleep 0.6"));
    let quick = script(dir.path(), "kotodama-quick", "exit 0");
    install(dir.path(), "system", "quick.desktop", &app("Quick", &quick));
    let mut apps = Apps::configured(dir, Table::from([("withdraw_within".into(), Value::Float(0.3))]));
    apps.added();

    assert_eq!(apps.run_as(1, "slow", "open"), Outcome::Close);
    // Long enough for `sleep` to have exited and been reaped.
    thread::sleep(Duration::from_millis(900));
    apps.quiet();
    // The quick app, run after under the same setting, has its pick
    // withdrawn.
    assert_eq!(apps.run_as(2, "quick", "open"), Outcome::Close);
    assert_eq!(
        apps.recv_timeout(WAIT),
        Ok(FromExtension::WithdrawPick { invocation: 2 })
    );
    apps.quiet();
}

#[test]
fn an_app_still_running_keeps_its_pick() {
    let dir = data_dirs();
    install(dir.path(), "system", "long.desktop", &app("Long", "sleep 3"));
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    assert_eq!(apps.run_as(5, "long", "open"), Outcome::Close);
    apps.quiet();
}

#[test]
fn a_launcher_that_leaves_the_app_running_and_exits_keeps_its_pick() {
    let dir = data_dirs();
    // Starts the "app" in the background, as a launcher script does, and
    // exits at once.
    let launcher = script(dir.path(), "kotodama-launcher", "sleep 3 &\nexit 0");
    install(dir.path(), "system", "launched.desktop", &app("Launched", &launcher));
    let mut apps = Apps::configured(dir, Table::from([("withdraw_within".into(), Value::Integer(1))]));
    apps.added();

    assert_eq!(apps.run_as(6, "launched", "open"), Outcome::Close);
    // Past `withdraw_within`, with the app it left still running.
    thread::sleep(Duration::from_millis(1200));
    apps.quiet();
}

#[test]
fn a_launch_that_hands_off_to_a_copy_already_running_keeps_its_pick() {
    let dir = data_dirs();
    // The second launch of a single-instance app: it tells the copy that
    // runs already to open a window, and exits at once.
    let handoff = script(dir.path(), "kotodama-handoff", "exit 0");
    install(dir.path(), "system", "single.desktop", &app("Single", &handoff));
    let mut running = Command::new("sh")
        .arg0("kotodama-handoff")
        // Not `exec`ed away by the shell, so the name it runs under stays.
        .args(["-c", "sleep 3; :"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // Start times count in clock ticks, of 10 ms.
    thread::sleep(Duration::from_millis(50));
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    assert_eq!(apps.run_as(8, "single", "open"), Outcome::Close);
    apps.quiet();
    running.kill().unwrap();
    running.wait().unwrap();
}

#[test]
fn a_withdraw_within_of_zero_never_withdraws_a_pick() {
    let dir = data_dirs();
    install(dir.path(), "system", "oops.desktop", &app("Oops", "true"));
    let mut apps = Apps::configured(dir, Table::from([("withdraw_within".into(), Value::Integer(0))]));
    apps.added();

    assert_eq!(apps.run_as(3, "oops", "open"), Outcome::Close);
    apps.quiet();
}

#[test]
fn the_manifest_names_the_extension_apps() {
    let manifest =
        Manifest::from_toml(&fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("manifest.toml")).unwrap())
            .unwrap();
    assert_eq!(manifest.name, "apps");
    assert!(manifest.live_query.is_none());
}

#[test]
fn an_app_whose_directory_is_missing_says_so() {
    let dir = data_dirs();
    install(
        dir.path(),
        "system",
        "lost.desktop",
        "[Desktop Entry]\nType=Application\nName=Lost\nExec=sh\nPath=/nonexistent/dir\n",
    );
    let mut apps = Apps::configured(dir, Table::new());
    apps.added();

    let Outcome::Failed { message } = apps.run("lost", "open") else {
        panic!("expected a failure");
    };
    assert!(message.contains("/nonexistent/dir"), "{message}");
}

#[test]
fn the_extension_exits_when_the_daemon_hangs_up() {
    let mut apps = Apps::start(data_dirs());
    apps.hang_up();
    assert!(apps.child.wait().unwrap().success());
}
