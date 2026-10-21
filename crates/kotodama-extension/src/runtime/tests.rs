use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use super::missing_wayland_display;
use super::wait_for;

/// A program started before the compositor's environment reached it
/// passes on the display it would have had.
#[test]
fn a_launched_program_gets_the_display_the_launcher_lacks() {
    let runtime = tempfile::tempdir().unwrap();
    let dir = runtime.path();
    assert_eq!(missing_wayland_display(None, Some(dir)), None);

    let _three = UnixListener::bind(dir.join("wayland-3")).unwrap();
    let _one = UnixListener::bind(dir.join("wayland-1")).unwrap();
    assert_eq!(missing_wayland_display(None, Some(dir)), Some("wayland-1".into()));
    assert_eq!(
        missing_wayland_display(Some("".as_ref()), Some(dir)),
        Some("wayland-1".into())
    );
    // One it has already, it passes on as it is.
    assert_eq!(missing_wayland_display(Some("wayland-3".as_ref()), Some(dir)), None);
    assert_eq!(missing_wayland_display(None, None), None);
}

#[test]
fn waiting_ends_once_something_is_made_where_it_watches() {
    let runtime = tempfile::tempdir().unwrap();
    let socket = runtime.path().join("compositor");
    let (done, waited) = mpsc::channel();
    let dir = runtime.path().to_owned();
    let looked_for = socket.clone();
    thread::spawn(move || {
        let mut said = Vec::new();
        wait_for(
            &[&dir],
            || {
                if looked_for.exists() {
                    Ok(())
                } else {
                    Err("not there".into())
                }
            },
            |err| said.push(err.to_owned()),
        );
        let _ = done.send(said);
    });
    // Long enough that the wait between tries has grown past what the
    // test allows below, so only the watch can end it in time.
    thread::sleep(Duration::from_millis(1600));
    assert!(waited.try_recv().is_err(), "found nothing");

    let made = Instant::now();
    std::fs::write(&socket, "").unwrap();
    let said = waited.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(made.elapsed() < Duration::from_millis(500), "{:?}", made.elapsed());
    assert_eq!(said, ["not there"], "says it waits once");
}

/// A compositor makes its socket a moment before it listens on it: a try
/// that fails just after a change goes again soon, not after the long
/// wait the backoff had grown to.
#[test]
fn a_try_that_fails_just_after_a_change_is_soon_tried_again() {
    let runtime = tempfile::tempdir().unwrap();
    let socket = runtime.path().join("compositor");
    let (done, waited) = mpsc::channel();
    let dir = runtime.path().to_owned();
    let looked_for = socket.clone();
    thread::spawn(move || {
        let mut refused = 1;
        wait_for(
            &[&dir],
            || {
                if !looked_for.exists() {
                    Err("not there".into())
                } else if refused > 0 {
                    refused -= 1;
                    Err("refused".into())
                } else {
                    Ok(())
                }
            },
            |_| {},
        );
        let _ = done.send(());
    });
    thread::sleep(Duration::from_millis(1600));

    let made = Instant::now();
    std::fs::write(&socket, "").unwrap();
    waited.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(made.elapsed() < Duration::from_millis(600), "{:?}", made.elapsed());
}

#[test]
fn a_missing_directory_is_tried_again_after_a_while() {
    let runtime = tempfile::tempdir().unwrap();
    let missing = runtime.path().join("hypr");
    let mut tries = 0;
    wait_for(
        &[&missing],
        || {
            tries += 1;
            if tries < 3 { Err("no".into()) } else { Ok(()) }
        },
        |_| {},
    );
    assert_eq!(tries, 3);
}
