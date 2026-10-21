use std::io::BufReader;
use std::io::Cursor;

use kotodama_protocol::Error;
use kotodama_protocol::frame;
use kotodama_protocol::frame::MAX_FRAME_LEN;

fn read_all(bytes: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    let mut reader = BufReader::new(bytes);
    let mut frames = Vec::new();
    let mut buf = Vec::new();
    while frame::read(&mut reader, &mut buf)? {
        frames.push(buf.clone());
    }
    Ok(frames)
}

#[test]
fn a_frame_is_its_decimal_length_a_newline_and_the_payload() {
    let mut out = Vec::new();
    frame::write(&mut out, b"{\"a\":1}").unwrap();
    assert_eq!(out, b"7\n{\"a\":1}");
}

#[test]
fn frames_written_back_to_back_read_back_in_order() {
    let mut out = Vec::new();
    for payload in [&b"one"[..], b"", b"three\nwith a newline"] {
        frame::write(&mut out, payload).unwrap();
    }
    assert_eq!(read_all(&out).unwrap(), [&b"one"[..], b"", b"three\nwith a newline"]);
}

#[test]
fn end_of_stream_between_frames_is_a_clean_close() {
    assert_eq!(read_all(b"").unwrap(), Vec::<Vec<u8>>::new());
}

#[test]
fn end_of_stream_inside_a_frame_is_an_error() {
    for truncated in [&b"10\nabc"[..], b"12"] {
        let err = read_all(truncated).unwrap_err();
        assert!(
            matches!(err, Error::Io(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof),
            "{err:?}"
        );
    }
}

#[test]
fn a_header_that_is_not_a_decimal_length_is_rejected() {
    for bad in [&b"\n"[..], b"+3\nabc", b"-1\n", b"0x10\n", b"3 \nabc", b"{\"a\":1}\n"] {
        let err = read_all(bad).unwrap_err();
        assert!(matches!(err, Error::BadFrameHeader), "{bad:?}: {err:?}");
    }
}

#[test]
fn a_frame_longer_than_the_limit_is_rejected_before_reading_it() {
    let header = format!("{}\n", MAX_FRAME_LEN + 1);
    let err = read_all(header.as_bytes()).unwrap_err();
    assert!(
        matches!(err, Error::FrameTooLong(len) if len == MAX_FRAME_LEN + 1),
        "{err:?}"
    );

    let endless_digits = vec![b'9'; 4096];
    let err = read_all(&endless_digits).unwrap_err();
    assert!(matches!(err, Error::BadFrameHeader | Error::FrameTooLong(_)), "{err:?}");
}

#[test]
fn reads_frames_split_across_many_small_reads() {
    let mut out = Vec::new();
    frame::write(&mut out, b"hello").unwrap();
    frame::write(&mut out, b"world").unwrap();
    let mut reader = BufReader::with_capacity(1, Cursor::new(out));
    let mut buf = Vec::new();
    assert!(frame::read(&mut reader, &mut buf).unwrap());
    assert_eq!(buf, b"hello");
    assert!(frame::read(&mut reader, &mut buf).unwrap());
    assert_eq!(buf, b"world");
    assert!(!frame::read(&mut reader, &mut buf).unwrap());
}
