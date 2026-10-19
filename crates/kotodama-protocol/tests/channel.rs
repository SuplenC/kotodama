//! A Channel over a real Unix socket pair, split so one thread reads while
//! another writes.

use std::os::unix::net::UnixStream;
use std::thread;

use kotodama_protocol::*;

fn connected() -> (
    Channel<UnixStream, UnixStream>,
    Channel<UnixStream, UnixStream>,
) {
    let (a, b) = UnixStream::pair().unwrap();
    let (a2, b2) = (a.try_clone().unwrap(), b.try_clone().unwrap());
    let peer = thread::spawn(move || {
        Channel::handshake(
            b,
            b2,
            &Hello::new(&[WireFormat::Json, WireFormat::Postcard]),
        )
        .unwrap()
    });
    let ours = Channel::handshake(a, a2, &Hello::new(&WireFormat::ALL)).unwrap();
    (ours, peer.join().unwrap())
}

#[test]
fn split_halves_keep_the_agreed_format_and_carry_messages_both_ways() {
    let (daemon, front_end) = connected();
    let (mut daemon_rx, mut daemon_tx) = daemon.split();
    assert_eq!(daemon_rx.format(), WireFormat::Postcard);
    assert_eq!(daemon_tx.format(), WireFormat::Postcard);
    let (mut front_rx, mut front_tx) = front_end.split();

    let writer = thread::spawn(move || {
        front_tx
            .send(&FromFrontEnd::Query { text: "fir".into() })
            .unwrap();
    });
    assert_eq!(
        daemon_rx.recv::<FromFrontEnd>().unwrap(),
        Some(FromFrontEnd::Query { text: "fir".into() })
    );
    writer.join().unwrap();

    daemon_tx.send(&ToFrontEnd::Close).unwrap();
    assert_eq!(
        front_rx.recv::<ToFrontEnd>().unwrap(),
        Some(ToFrontEnd::Close)
    );
}

#[test]
fn a_reader_sees_the_end_once_the_peer_drops_its_writer() {
    let (daemon, front_end) = connected();
    let (mut daemon_rx, _daemon_tx) = daemon.split();
    let (front_rx, front_tx) = front_end.split();
    drop(front_tx);
    drop(front_rx);
    assert_eq!(daemon_rx.recv::<FromFrontEnd>().unwrap(), None);
}
