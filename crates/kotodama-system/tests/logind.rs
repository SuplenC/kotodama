//! Drives the system Extension the way the Daemon does, over its stdio, with
//! a fake logind on a private bus of its own: a real `dbus-daemon` with a
//! test thread owning `org.freedesktop.login1`.

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::process::Child;
use std::process::Command;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use kotodama_extension::dbus::Bus;
use kotodama_extension::dbus::Call;
use kotodama_extension::dbus::Marshal;
use kotodama_extension::testing::Spawned;
use kotodama_protocol::*;
use kotodama_system::logind;
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(10);
/// The methods that take `interactive`, a boolean.
const BY_FLAG: [&str; 4] = ["PowerOff", "Reboot", "Suspend", "Hibernate"];
/// The methods that take a session id.
const BY_SESSION: [&str; 2] = ["LockSession", "TerminateSession"];

/// A bus of its own, as the system bus, in a temporary directory. Dropping
/// it stops the bus.
struct PrivateBus {
    dir: TempDir,
    daemon: Child,
}

impl PrivateBus {
    fn start() -> Self {
        let dir = tempfile::tempdir().expect("make the bus's dir");
        let config = dir.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <listen>unix:path={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                dir.path().join("bus").display()
            ),
        )
        .expect("write the bus's config");
        let daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .spawn()
            .expect("start dbus-daemon, which the dev shell has");
        let socket = dir.path().join("bus");
        for _ in 0..100 {
            if socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        PrivateBus { dir, daemon }
    }

    fn address(&self) -> String {
        format!("unix:path={}", self.dir.path().join("bus").display())
    }

    /// Plays logind on the bus: answers `CanHibernate` with `can_hibernate`,
    /// and every power method with nothing, reporting each as the method's
    /// name and its argument.
    fn logind(&self, can_hibernate: &'static str) -> Receiver<String> {
        let mut bus = Bus::at(&self.address()).expect("connect the fake logind");
        let mut name = Marshal::default();
        name.string(logind::DESTINATION);
        // DBUS_NAME_FLAG_DO_NOT_QUEUE.
        name.u32(4);
        let request = Call::new(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            "org.freedesktop.DBus",
        )
        .with_body("su", name);
        bus.call(&request, |_| {}).expect("own logind's name");
        bus.wait_forever().expect("wait for calls");

        let (called, calls) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(message) = bus.read() {
                let is = |method| message.is_call(logind::PATH, logind::MANAGER, method);
                let call = if is("CanHibernate") {
                    let mut answer = Marshal::default();
                    answer.string(can_hibernate);
                    bus.reply(&message, "s", answer).expect("answer CanHibernate");
                    continue;
                } else if let Some(method) = BY_FLAG.into_iter().find(|&method| is(method)) {
                    format!("{method} {}", message.body().u32() == Some(1))
                } else if let Some(method) = BY_SESSION.into_iter().find(|&method| is(method)) {
                    format!("{method} {}", message.body().string().unwrap_or_default())
                } else {
                    continue;
                };
                bus.reply(&message, "", Marshal::default()).expect("answer the call");
                if called.send(call).is_err() {
                    return;
                }
            }
        });
        calls
    }

    /// The Extension, started as the Daemon starts it, with this bus as the
    /// system bus and no compositor.
    fn extension(&self) -> (Spawned, TempDir) {
        let runtime = tempfile::tempdir().expect("make the runtime dir");
        let mut command = Command::new(env!("CARGO_BIN_EXE_kotodama-system"));
        command
            .env("DBUS_SYSTEM_BUS_ADDRESS", self.address())
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env_remove("WAYLAND_DISPLAY");
        let mut extension = Spawned::start(&mut command, WAIT);
        extension.send(ToExtension::Config(Table::new()));
        (extension, runtime)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn power(id: &str) -> ItemRef {
    ItemRef {
        id: id.into(),
        kind: ItemKind::Namespaced(NamespacedKind::new("system.power").expect("a valid kind")),
    }
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

/// Runs `id`'s primary Action as `invocation` and returns its outcome.
fn run(extension: &mut Spawned, invocation: u64, id: &str) -> Outcome {
    extension.start_run(invocation, "run", power(id), BTreeMap::new());
    extension.outcome(invocation, |_| false)
}

/// The call the fake logind received next.
fn next(calls: &Receiver<String>) -> String {
    calls.recv_timeout(WAIT).expect("logind was called")
}

#[test]
fn hibernate_is_listed_only_when_can_hibernate_says_yes() {
    for (answer, listed) in [("yes", true), ("no", false), ("na", false), ("challenge", false)] {
        let bus = PrivateBus::start();
        let _calls = bus.logind(answer);
        let (extension, _runtime) = bus.extension();

        let items = extension.added();

        assert_eq!(ids(&items).contains(&"hibernate"), listed, "CanHibernate {answer}");
        assert!(ids(&items).contains(&"suspend"));
    }
}

#[test]
fn without_logind_every_power_item_but_hibernate_is_listed_and_running_one_says_why_it_failed() {
    let bus = PrivateBus::start();
    let (mut extension, _runtime) = bus.extension();

    let items = extension.added();
    let outcome = run(&mut extension, 1, "suspend");

    assert_eq!(extension.status(), (true, None));
    assert_eq!(ids(&items), ["power-off", "reboot", "suspend", "lock", "log-out"]);
    let Outcome::Failed { message } = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert!(
        message.starts_with("Couldn't suspend: logind turned it down"),
        "{message}"
    );
}

#[test]
fn suspend_hibernate_and_lock_call_logind_at_once_and_close_the_menu() {
    let bus = PrivateBus::start();
    let calls = bus.logind("yes");
    let (mut extension, _runtime) = bus.extension();
    extension.added();

    for (invocation, (id, call)) in [
        ("suspend", "Suspend false"),
        ("hibernate", "Hibernate false"),
        ("lock", "LockSession auto"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(run(&mut extension, invocation as u64, id), Outcome::Close, "{id}");
        assert_eq!(next(&calls), call);
    }
}

#[test]
fn power_off_reboot_and_log_out_ask_the_front_end_to_confirm_and_call_logind_once_run() {
    let bus = PrivateBus::start();
    let calls = bus.logind("no");
    let (mut extension, _runtime) = bus.extension();
    let items = extension.added();

    let confirmed: Vec<&str> = items
        .iter()
        .filter(|item| item.actions.iter().all(|action| action.confirm))
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(confirmed, ["power-off", "reboot", "log-out"]);
    for (invocation, (id, call)) in [
        ("power-off", "PowerOff false"),
        ("reboot", "Reboot false"),
        ("log-out", "TerminateSession auto"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(run(&mut extension, invocation as u64, id), Outcome::Close, "{id}");
        assert_eq!(next(&calls), call);
    }
}

#[test]
fn a_call_logind_turns_down_fails_with_why() {
    let bus = PrivateBus::start();
    let (mut extension, _runtime) = bus.extension();
    extension.added();

    let outcome = run(&mut extension, 1, "reboot");

    let Outcome::Failed { message } = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert!(
        message.starts_with("Couldn't reboot: logind turned it down"),
        "{message}"
    );
}

#[test]
fn without_a_compositor_the_extension_sends_no_windows() {
    let bus = PrivateBus::start();
    let _calls = bus.logind("no");
    let (extension, _runtime) = bus.extension();

    extension.added();

    extension.quiet();
}
