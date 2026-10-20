use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use super::*;

/// A message from the peer: `kind`, the header fields, then `body` of type
/// `signature`.
fn message(
    kind: u8,
    fields: impl FnOnce(&mut Marshal),
    signature: &str,
    body: &Marshal,
) -> Vec<u8> {
    let mut m = Marshal::default();
    m.0.extend_from_slice(&[b'l', kind, 0, 1]);
    m.u32(body.0.len() as u32);
    m.u32(99);
    m.array(8, |m| {
        fields(m);
        if !signature.is_empty() {
            m.field(SIGNATURE, "g", signature);
        }
    });
    m.pad(8);
    m.0.extend_from_slice(&body.0);
    m.0
}

fn reply_serial(m: &mut Marshal, serial: u32) {
    m.pad(8);
    m.byte(REPLY_SERIAL);
    m.signature("u");
    m.u32(serial);
}

/// The return of the call `serial`, with the object path `path`.
pub(crate) fn method_return(serial: u32, path: &str) -> Vec<u8> {
    let mut answer = Marshal::default();
    answer.string(path);
    message(METHOD_RETURN, |m| reply_serial(m, serial), "o", &answer)
}

/// The systemd manager's signal that the job 4, for `unit`, finished with
/// `result`.
pub(crate) fn job_removed(unit: &str, result: &str) -> Vec<u8> {
    let mut signal = Marshal::default();
    signal.u32(4);
    signal.string("/org/freedesktop/systemd1/job/4");
    signal.string(unit);
    signal.string(result);
    message(
        SIGNAL,
        |m| m.field(MEMBER, "s", "JobRemoved"),
        "uoss",
        &signal,
    )
}

/// A connection to the peer at the other end of `stream`.
pub(crate) fn bus(stream: UnixStream) -> Bus {
    Bus::authenticated(stream).unwrap()
}

/// Plays the peer on the other end of `stream`: accepts the authentication,
/// reads one message, and answers with what `answer` makes of its serial and
/// its bytes. Returns the message.
pub(crate) fn peer(
    stream: UnixStream,
    answer: impl FnOnce(u32, &[u8]) -> Vec<u8> + Send + 'static,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut auth = Vec::new();
        reader.read_until(b'\n', &mut auth).unwrap();
        assert!(auth.starts_with(b"\0AUTH EXTERNAL "), "{auth:?}");
        writer.write_all(b"OK 0123456789abcdef\r\n").unwrap();
        let mut begin = Vec::new();
        reader.read_until(b'\n', &mut begin).unwrap();
        assert_eq!(begin, b"BEGIN\r\n");
        let mut fixed = [0u8; 16];
        reader.read_exact(&mut fixed).unwrap();
        let word = |at: usize| u32::from_le_bytes(fixed[at..at + 4].try_into().unwrap());
        let mut rest = vec![0; (word(12) as usize).next_multiple_of(8) + word(4) as usize];
        reader.read_exact(&mut rest).unwrap();
        let mut call = fixed.to_vec();
        call.extend(rest);
        // The other end may have hung up once it had what it waited for.
        let _ = writer.write_all(&answer(word(8), &call));
        call
    })
}

fn call() -> Call<'static> {
    let mut body = Marshal::default();
    body.string("hello");
    body.u32(7);
    Call::new("/org/example", "org.example.Thing", "Do", "org.example").with_body("su", body)
}

#[test]
fn a_call_is_answered_by_the_return_with_its_serial_after_the_signals_before_it() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = peer(theirs, |serial, _| {
        let mut bytes = job_removed("app-x.scope", "done");
        bytes.extend(method_return(serial, "/job/4"));
        bytes
    });
    let mut bus = bus(ours);
    let mut signals = Vec::new();
    let answer = bus
        .call(&call(), |message| {
            if message.is_signal("JobRemoved", "uoss") {
                let mut body = message.body();
                let job = body.u32();
                let path = body.string().map(str::to_owned);
                let unit = body.string().map(str::to_owned);
                let result = body.string().map(str::to_owned);
                signals.push((job, path, unit, result));
            }
        })
        .unwrap();
    assert_eq!(answer.body().string(), Some("/job/4"));
    assert_eq!(
        signals,
        [(
            Some(4),
            Some("/org/freedesktop/systemd1/job/4".to_owned()),
            Some("app-x.scope".to_owned()),
            Some("done".to_owned())
        )]
    );

    let sent = peer.join().unwrap();
    // Little-endian, a method call, version 1, serial 1.
    assert_eq!(&sent[..4], &[b'l', 1, 0, 1]);
    assert_eq!(&sent[8..12], &1u32.to_le_bytes());
    let mut fields = Message {
        kind: 1,
        little: true,
        reply_serial: None,
        member: None,
        error_name: None,
        signature: String::new(),
        body: Vec::new(),
    };
    let fields_len = u32::from_le_bytes(sent[12..16].try_into().unwrap()) as usize;
    fields.read_fields(&sent[16..16 + fields_len]);
    assert_eq!(fields.member.as_deref(), Some("Do"));
    assert_eq!(fields.signature, "su");
    let body = &sent[16 + fields_len.next_multiple_of(8)..];
    let mut read = Unmarshal {
        bytes: body,
        at: 0,
        little: true,
    };
    assert_eq!(read.string(), Some("hello"));
    assert_eq!(read.u32(), Some(7));
}

#[test]
fn a_call_answered_with_an_error_fails_with_its_name_and_message() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = peer(theirs, |serial, _| {
        let mut body = Marshal::default();
        body.string("Unit app-x.scope already exists.");
        message(
            ERROR,
            |m| {
                m.field(ERROR_NAME, "s", "org.freedesktop.systemd1.UnitExists");
                reply_serial(m, serial);
            },
            "s",
            &body,
        )
    });
    let mut bus = bus(ours);
    let err = bus.call(&call(), |_| {}).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the call failed: org.freedesktop.systemd1.UnitExists: Unit app-x.scope already exists."
    );
    peer.join().unwrap();
}

#[test]
fn a_peer_that_turns_down_the_authentication_refuses_us_with_its_line() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    let peer = thread::spawn(move || {
        let mut auth = [0u8; 8];
        theirs.read_exact(&mut auth).unwrap();
        theirs.write_all(b"REJECTED EXTERNAL\r\n").unwrap();
    });
    let err = Bus::authenticated(ours).unwrap_err();
    assert_eq!(err.to_string(), "the bus refused us: REJECTED EXTERNAL");
    peer.join().unwrap();
}

#[test]
fn nothing_to_connect_to_is_absent_and_a_refusal_is_not() {
    assert!(Error::NoAddress.is_absent());
    assert!(Error::Connect(io::ErrorKind::NotFound.into()).is_absent());
    assert!(Error::Connect(io::ErrorKind::ConnectionRefused.into()).is_absent());
    assert!(!Error::Connect(io::ErrorKind::PermissionDenied.into()).is_absent());
    assert!(!Error::Refused("REJECTED".into()).is_absent());
}

#[test]
fn an_address_value_is_unescaped() {
    assert_eq!(unescape("/run/user/1000/b%75s"), "/run/user/1000/bus");
    assert_eq!(unescape("100%"), "100%");
}
