use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::Display;

fn display(name: Option<&str>, runtime: &Path) -> Display {
    Display::new(name.map(Into::into), Some(runtime.to_owned()))
}

#[test]
fn a_named_display_is_its_socket_in_the_runtime_dir() {
    let runtime = tempfile::tempdir().unwrap();
    assert_eq!(
        display(Some("wayland-5"), runtime.path()).sockets(),
        [runtime.path().join("wayland-5")]
    );
}

#[test]
fn a_display_named_by_its_path_is_that_path() {
    let runtime = tempfile::tempdir().unwrap();
    assert_eq!(
        display(Some("/run/elsewhere/wayland-0"), runtime.path()).sockets(),
        [Path::new("/run/elsewhere/wayland-0")]
    );
}

#[test]
fn without_a_name_every_wayland_socket_in_the_runtime_dir_lowest_number_first() {
    let runtime = tempfile::tempdir().unwrap();
    let dir = runtime.path();
    let _ten = UnixListener::bind(dir.join("wayland-10")).unwrap();
    let _two = UnixListener::bind(dir.join("wayland-2")).unwrap();
    std::fs::write(dir.join("wayland-2.lock"), "").unwrap();
    std::fs::write(dir.join("wayland-3"), "not a socket").unwrap();
    let _other = UnixListener::bind(dir.join("pipewire-0")).unwrap();
    // A socket linked in from elsewhere counts too.
    let elsewhere = tempfile::tempdir().unwrap();
    let _linked = UnixListener::bind(elsewhere.path().join("socket")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join("socket"), dir.join("wayland-7")).unwrap();

    assert_eq!(
        display(None, dir).sockets(),
        [dir.join("wayland-2"), dir.join("wayland-7"), dir.join("wayland-10")]
    );
}

#[test]
fn an_empty_name_is_no_name() {
    let runtime = tempfile::tempdir().unwrap();
    let _one = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
    assert_eq!(
        display(Some(""), runtime.path()).sockets(),
        [runtime.path().join("wayland-1")]
    );
}

#[test]
fn connecting_without_a_compositor_says_so() {
    let runtime = tempfile::tempdir().unwrap();
    let Err(err) = display(None, runtime.path()).connect() else {
        panic!("connected to nothing");
    };
    assert!(err.contains("no Wayland compositor"), "{err}");
}

/// Runs `wait` on a thread, so a test that fails doesn't hang.
fn waits(display: Display, mut failures: usize) -> mpsc::Receiver<Vec<String>> {
    let (done, waited) = mpsc::channel();
    thread::spawn(move || {
        let mut said = Vec::new();
        display.wait(
            |_| {
                if failures == 0 {
                    Ok(())
                } else {
                    failures -= 1;
                    Err("not yet".into())
                }
            },
            |err| said.push(err.to_owned()),
        );
        let _ = done.send(said);
    });
    waited
}

#[test]
fn waiting_ends_once_the_compositor_makes_its_socket() {
    let runtime = tempfile::tempdir().unwrap();
    let waited = waits(display(None, runtime.path()), 0);
    thread::sleep(Duration::from_millis(300));
    assert!(waited.try_recv().is_err(), "connected to nothing");

    let _compositor = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
    let said = waited.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(said.len(), 1, "says it waits once: {said:?}");
}

#[test]
fn waiting_for_a_named_display_ignores_other_sockets() {
    let runtime = tempfile::tempdir().unwrap();
    let waited = waits(display(Some("wayland-2"), runtime.path()), 0);
    let _other = UnixListener::bind(runtime.path().join("wayland-1")).unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(waited.try_recv().is_err(), "connected to the wrong display");

    let _named = UnixListener::bind(runtime.path().join("wayland-2")).unwrap();
    waited.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn a_connection_that_fails_is_tried_again() {
    let runtime = tempfile::tempdir().unwrap();
    let _compositor = UnixListener::bind(runtime.path().join("wayland-0")).unwrap();
    let said = waits(display(None, runtime.path()), 2)
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert_eq!(said, ["not yet"]);
}
