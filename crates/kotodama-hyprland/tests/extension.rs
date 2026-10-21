//! Drives the Hyprland Extension the way the Daemon does, over its stdio,
//! with a fake Hyprland serving its two sockets from recorded output.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(10);
const SIGNATURE: &str = "fixture_1729000000_1234567890";
const FIREFOX: &str = "0x55d0c6a1b2c0";
const FOOT: &str = "0x55d0c6b3e1c0";
const BINDS: &str = include_str!("fixtures/binds.json");

/// What the fake Hyprland shares with its socket threads.
#[derive(Default)]
struct State {
    binds: String,
    requests: Vec<String>,
    events: Option<UnixStream>,
}

/// Hyprland's request socket and event socket in a runtime directory of its
/// own, as `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`.
struct FakeHyprland {
    runtime: Arc<TempDir>,
    state: Arc<(Mutex<State>, Condvar)>,
}

impl FakeHyprland {
    fn start() -> Self {
        FakeHyprland::start_in(Arc::new(tempfile::tempdir().expect("make the runtime dir")), SIGNATURE)
    }

    /// A Hyprland instance `signature` in `runtime`, which another may have
    /// run in before.
    fn start_in(runtime: Arc<TempDir>, signature: &str) -> Self {
        let dir = runtime.path().join("hypr").join(signature);
        std::fs::create_dir_all(&dir).expect("make the instance dir");
        let state = Arc::new((
            Mutex::new(State {
                binds: BINDS.to_owned(),
                ..State::default()
            }),
            Condvar::new(),
        ));

        let requests = UnixListener::bind(dir.join(".socket.sock")).expect("bind the request socket");
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for stream in requests.incoming() {
                let mut stream = stream.expect("accept a request");
                // Hyprland reads one request, answers it and hangs up.
                let mut buf = [0; 8192];
                let len = stream.read(&mut buf).expect("read a request");
                let request = String::from_utf8_lossy(&buf[..len]).into_owned();
                let mut state = shared.0.lock().expect("fake Hyprland mutex poisoned");
                let reply = match request.as_str() {
                    "j/clients" => include_str!("fixtures/clients.json").to_owned(),
                    "j/binds" => state.binds.clone(),
                    dispatch if dispatch.starts_with("/dispatch ") => "ok".to_owned(),
                    _ => "unknown request".to_owned(),
                };
                state.requests.push(request);
                shared.1.notify_all();
                drop(state);
                let _ = stream.write_all(reply.as_bytes());
            }
        });

        let events = UnixListener::bind(dir.join(".socket2.sock")).expect("bind the event socket");
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            let (stream, _) = events.accept().expect("accept the Extension on the event socket");
            shared.0.lock().expect("fake Hyprland mutex poisoned").events = Some(stream);
            shared.1.notify_all();
        });
        FakeHyprland { runtime, state }
    }

    /// The Extension, started as the Daemon starts it inside this Hyprland.
    fn extension(&self) -> Spawned {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kotodama-hyprland"));
        command
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env("HYPRLAND_INSTANCE_SIGNATURE", SIGNATURE);
        let mut extension = Spawned::start(&mut command, WAIT);
        extension.send(ToExtension::Config(Table::new()));
        extension
    }

    /// Sends an event line once the Extension listens.
    fn emit(&self, line: &str) {
        let (lock, ready) = &*self.state;
        let state = ready
            .wait_timeout_while(lock.lock().expect("fake Hyprland mutex poisoned"), WAIT, |state| {
                state.events.is_none()
            })
            .expect("fake Hyprland mutex poisoned")
            .0;
        let mut stream = state.events.as_ref().expect("nothing listens to events");
        writeln!(stream, "{line}").expect("send an event");
    }

    /// Closes the event socket, as Hyprland exiting does.
    fn exit(&self) {
        self.emit("workspace>>1");
        self.state.0.lock().expect("fake Hyprland mutex poisoned").events = None;
    }

    fn set_binds(&self, json: &str) {
        json.clone_into(&mut self.state.0.lock().expect("fake Hyprland mutex poisoned").binds);
    }

    /// The `/dispatch` requests so far, without the prefix.
    fn dispatched(&self) -> Vec<String> {
        let state = self.state.0.lock().expect("fake Hyprland mutex poisoned");
        state
            .requests
            .iter()
            .filter_map(|request| request.strip_prefix("/dispatch "))
            .map(str::to_owned)
            .collect()
    }
}

/// The Items sent at start: the Shortcuts Item and the binds, then the
/// windows.
fn started(extension: &Spawned) -> Vec<Item> {
    [extension.added(), extension.added()].concat()
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

fn window(address: &str) -> ItemRef {
    ItemRef {
        id: address.into(),
        kind: ItemKind::Window {
            address: address.into(),
        },
    }
}

fn shortcuts() -> ItemRef {
    ItemRef {
        id: "shortcuts".into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("hyprland.shortcuts").expect("a valid kind")),
    }
}

fn run(extension: &mut Spawned, invocation: u64, action: &str, item: ItemRef) -> Outcome {
    run_with(extension, invocation, action, item, BTreeMap::new())
}

fn run_with(
    extension: &mut Spawned,
    invocation: u64,
    action: &str,
    item: ItemRef,
    arguments: BTreeMap<String, String>,
) -> Outcome {
    extension.start_run(invocation, action, item, arguments);
    extension.outcome(invocation, |_| false)
}

/// Runs the Shortcuts Item's primary Action and returns the View it pushes.
fn open_shortcuts(extension: &mut Spawned, invocation: u64) -> Vec<Item> {
    extension.start_run(invocation, "open", shortcuts(), BTreeMap::new());
    let view = RefCell::new(None);
    let outcome = extension.outcome(invocation, |message| match message {
        FromExtension::PushView(pushed) => {
            *view.borrow_mut() = Some(pushed.clone());
            true
        }
        FromExtension::AddItems { .. } | FromExtension::RemoveItems { .. } => true,
        _ => false,
    });
    assert_eq!(outcome, Outcome::KeepOpen);
    let view = view.into_inner().expect("the Shortcuts View");
    assert_eq!(view.title, "Keyboard Shortcuts");
    match view.content {
        ViewContent::List { items, .. } => items,
        other => panic!("expected a list View, got {other:?}"),
    }
}

#[test]
fn the_windows_the_shortcuts_item_and_the_described_binds_are_sent_at_start() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    let items = started(&extension);
    let ids = ids(&items);
    assert_eq!(ids[0], "shortcuts");
    assert!(ids.contains(&FIREFOX) && ids.contains(&FOOT));
    // Five binds have a description and aren't mouse binds.
    let binds = items
        .iter()
        .filter(|item| item.kind.name() == "hyprland.shortcut")
        .count();
    assert_eq!(binds, 5);
    assert_eq!(items.len(), 1 + 2 + 5);
    assert!(!items[0].hidden, "the search finds the Shortcuts Item too");
    extension.quiet();
}

#[test]
fn only_the_windows_an_event_changes_are_sent_again() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    started(&extension);

    hyprland.emit("openwindow>>55d0c6c2a4e0,3,org.gnome.Nautilus,Home");
    let opened = extension.added();
    assert_eq!(ids(&opened), ["0x55d0c6c2a4e0"]);
    assert_eq!(opened[0].title, "Home");

    hyprland.emit("movewindowv2>>55d0c6b3e1c0,4,4");
    let moved = extension.added();
    assert_eq!(ids(&moved), [FOOT]);
    assert_eq!(moved[0].subtitle.as_deref(), Some("foot · workspace 4"));

    hyprland.emit("closewindow>>55d0c6a1b2c0");
    assert_eq!(extension.removed(), [FIREFOX]);

    // Focus moving changes no Item.
    hyprland.emit("activewindowv2>>55d0c6b3e1c0");
    hyprland.emit("activewindow>>foot,~/src/kotodama");
    extension.quiet();
}

#[test]
fn a_windows_actions_dispatch_on_it_and_close_the_menu() {
    let hyprland = FakeHyprland::start();
    let mut extension = hyprland.extension();
    started(&extension);

    assert_eq!(run(&mut extension, 1, "focus", window(FIREFOX)), Outcome::Close);
    assert_eq!(run(&mut extension, 2, "float", window(FOOT)), Outcome::Close);
    let workspace = [("Workspace".to_owned(), "3".to_owned())].into();
    assert_eq!(
        run_with(&mut extension, 3, "move", window(FOOT), workspace),
        Outcome::Close
    );
    assert_eq!(run(&mut extension, 4, "close", window(FIREFOX)), Outcome::Close);
    assert_eq!(
        hyprland.dispatched(),
        [
            "focuswindow address:0x55d0c6a1b2c0",
            "togglefloating address:0x55d0c6b3e1c0",
            "movetoworkspacesilent 3,address:0x55d0c6b3e1c0",
            "closewindow address:0x55d0c6a1b2c0",
        ]
    );
}

#[test]
fn a_binds_item_runs_its_dispatcher() {
    let hyprland = FakeHyprland::start();
    let mut extension = hyprland.extension();
    let items = started(&extension);
    let close = items.iter().find(|item| item.title == "Close the window").unwrap();
    assert_eq!(run(&mut extension, 1, "run", close.to_ref()), Outcome::Close);
    assert_eq!(hyprland.dispatched(), ["killactive"]);
}

#[test]
fn a_mouse_bind_in_the_shortcuts_view_isnt_run_from_the_menu() {
    let hyprland = FakeHyprland::start();
    let mut extension = hyprland.extension();
    started(&extension);
    let view = open_shortcuts(&mut extension, 1);
    let drag = view.iter().find(|item| item.title == "SUPER + mouse:272").unwrap();
    assert!(matches!(
        run(&mut extension, 2, "run", drag.to_ref()),
        Outcome::Failed { .. }
    ));
    assert!(hyprland.dispatched().is_empty());
}

#[test]
fn a_bind_added_at_runtime_is_in_the_shortcuts_view_when_it_next_opens() {
    let hyprland = FakeHyprland::start();
    let mut extension = hyprland.extension();
    started(&extension);

    let first = open_shortcuts(&mut extension, 1);
    assert_eq!(first.len(), 10);
    assert_eq!(first[0].title, "XF86AudioRaiseVolume");
    extension.send(ToExtension::ViewPopped {
        view: "shortcuts".into(),
    });

    // As `hyprctl keyword bind SUPER, F9, exec, true` adds one, with no event.
    let added = BINDS.replacen(
        '[',
        r#"[{"modmask": 64, "submap": "", "key": "F9", "keycode": 0, "mouse": false,
            "description": "", "dispatcher": "exec", "arg": "true"},"#,
        1,
    );
    hyprland.set_binds(&added);
    let again = open_shortcuts(&mut extension, 2);
    assert_eq!(again.len(), 11);
    assert!(
        again
            .iter()
            .any(|item| item.title == "SUPER + F9" && item.subtitle.as_deref() == Some("exec true"))
    );
}

#[test]
fn a_config_reload_sends_the_binds_that_changed() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    started(&extension);

    let added = BINDS.replacen(
        '[',
        r#"[{"modmask": 64, "submap": "", "key": "P", "keycode": 0, "mouse": false,
            "description": "Pick a color", "dispatcher": "exec", "arg": "hyprpicker"},"#,
        1,
    );
    hyprland.set_binds(&added);
    hyprland.emit("configreloaded>>");
    let changed = extension.added();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].title, "Pick a color");
    assert_eq!(changed[0].subtitle.as_deref(), Some("SUPER + P"));
}

/// The CPU time a process has used, in clock ticks.
fn cpu_ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("read the process's stat");
    // The fields after the command, which is in parentheses.
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .expect("a command in parentheses")
        .1
        .split_whitespace()
        .collect();
    // utime and stime, fields 14 and 15 of the whole line.
    fields[11].parse::<u64>().expect("utime") + fields[12].parse::<u64>().expect("stime")
}

fn stays_quiet_and_idle(mut extension: Spawned) {
    let before = cpu_ticks(extension.child.id());
    extension.quiet();
    assert!(
        cpu_ticks(extension.child.id()) - before <= 1,
        "the Extension kept the CPU busy"
    );
    assert!(
        extension
            .child
            .try_wait()
            .expect("check whether the Extension exited")
            .is_none(),
        "the Extension exited, and the Daemon would start it again"
    );
    // An Action still gets an answer.
    match run(&mut extension, 1, "focus", window(FIREFOX)) {
        Outcome::Failed { message } => assert!(message.contains("Hyprland"), "{message}"),
        other => panic!("expected a failure, got {other:?}"),
    }
    // And the Daemon hanging up still ends it.
    extension.hang_up();
    let status = extension.child.wait().expect("wait for the Extension");
    assert!(status.success());
}

#[test]
fn without_hyprland_the_extension_starts_and_stays_quiet() {
    let runtime = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodama-hyprland"));
    // No Hyprland in its runtime directory either: it waits there for one.
    command
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE");
    let mut extension = Spawned::start(&mut command, WAIT);
    extension.send(ToExtension::Config(Table::new()));
    assert_eq!(extension.status(), not_running());
    stays_quiet_and_idle(extension);
}

fn not_running() -> (bool, Option<String>) {
    (false, Some("Hyprland isn't running".to_owned()))
}

#[test]
fn the_extension_is_usable_only_while_hyprland_runs() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    started(&extension);
    let at_start = [extension.status(), extension.status()];

    hyprland.exit();
    let _gone = [extension.removed(), extension.removed()].concat();

    assert_eq!(at_start, [not_running(), (true, None)]);
    assert_eq!(extension.status(), not_running());
}

#[test]
fn a_hyprland_gone_before_the_extension_started_leaves_it_quiet() {
    let runtime = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodama-hyprland"));
    command
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIGNATURE);
    let mut extension = Spawned::start(&mut command, WAIT);
    extension.send(ToExtension::Config(Table::new()));
    stays_quiet_and_idle(extension);
}

#[test]
fn hyprland_exiting_takes_its_windows_away() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    started(&extension);
    hyprland.exit();
    // The binds and the windows go, each sent as they came.
    let removed = [extension.removed(), extension.removed()].concat();
    assert_eq!(removed.len(), 1 + 5 + 2);
    assert!(removed.contains(&FIREFOX.to_owned()));
    extension.quiet();
}

/// Under socket activation the Extension can start before Hyprland, or
/// before `HYPRLAND_INSTANCE_SIGNATURE` reaches the service manager: it
/// waits in the runtime directory and follows Hyprland once it is there.
#[test]
fn a_hyprland_that_starts_after_the_extension_is_followed() {
    let runtime = Arc::new(tempfile::tempdir().unwrap());
    let mut command = Command::new(env!("CARGO_BIN_EXE_kotodama-hyprland"));
    command
        .env("XDG_RUNTIME_DIR", runtime.path())
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE");
    let mut extension = Spawned::start(&mut command, WAIT);
    extension.send(ToExtension::Config(Table::new()));
    extension.quiet();

    let _hyprland = FakeHyprland::start_in(runtime, SIGNATURE);
    let items = started(&extension);
    assert!(ids(&items).contains(&FIREFOX), "{:?}", ids(&items));
}

/// A restarted Hyprland has a new instance signature; the Extension finds it
/// without being restarted itself.
#[test]
fn a_restarted_hyprland_is_followed_again() {
    let hyprland = FakeHyprland::start();
    let extension = hyprland.extension();
    started(&extension);
    hyprland.exit();
    let _gone = [extension.removed(), extension.removed()].concat();

    let _again = FakeHyprland::start_in(Arc::clone(&hyprland.runtime), "fixture_1729000100_42");
    let items = started(&extension);
    assert!(ids(&items).contains(&FIREFOX), "{:?}", ids(&items));
}
