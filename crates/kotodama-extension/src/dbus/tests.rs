use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::thread;

use super::*;

/// A message from the peer: `kind`, the header fields, then `body` of type
/// `signature`.
fn message(kind: u8, fields: impl FnOnce(&mut Marshal), signature: &str, body: &Marshal) -> Vec<u8> {
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
    message(SIGNAL, |m| m.field(MEMBER, "s", "JobRemoved"), "uoss", &signal)
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
        serial: 0,
        reply_serial: None,
        path: None,
        interface: None,
        sender: None,
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
fn a_call_read_by_a_service_is_answered_to_its_serial() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let mut caller = Bus {
        stream: ours,
        serial: 0,
    };
    let mut service = Bus {
        stream: theirs,
        serial: 40,
    };

    let serial = caller.send(&call()).unwrap();
    let received = service.read().unwrap();
    assert!(received.is_call("/org/example", "org.example.Thing", "Do"));
    assert!(!received.is_call("/org/example", "org.example.Thing", "Undo"));
    let mut read = received.body();
    assert_eq!(read.string(), Some("hello"));
    assert_eq!(read.u32(), Some(7));
    let mut answer = Marshal::default();
    answer.string("done");
    service.reply(&received, "s", &answer).unwrap();

    let returned = caller.read().unwrap();
    assert_eq!(returned.reply_serial, Some(serial));
    assert_eq!(returned.body().string(), Some("done"));
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

/// A method return as a peer sends it, in either byte order, read off a
/// socket: the answer to serial 1, whose body, `body`, is of type `signature`.
fn reply(little: bool, signature: &str, body: &[u8]) -> Message {
    let word = |value: u32| {
        if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
    };
    let mut bytes = vec![if little { b'l' } else { b'B' }, METHOD_RETURN, 0, 1];
    bytes.extend(word(body.len() as u32));
    bytes.extend(word(7));
    bytes.extend(word(14 + signature.len() as u32));
    bytes.extend([REPLY_SERIAL, 1, b'u', 0]);
    bytes.extend(word(1));
    bytes.extend([SIGNATURE, 1, b'g', 0, signature.len() as u8]);
    bytes.extend(signature.as_bytes());
    bytes.push(0);
    bytes.resize(bytes.len().next_multiple_of(8), 0);
    bytes.extend(body);

    let (mut theirs, ours) = UnixStream::pair().unwrap();
    theirs.write_all(&bytes).unwrap();
    let message = Bus {
        stream: ours,
        serial: 1,
    }
    .read()
    .unwrap();
    assert_eq!(message.reply_serial, Some(1));
    assert_eq!(message.signature, signature);
    message
}

/// A reader of `bytes`, a body in the byte order `little` says.
fn body(bytes: &[u8], little: bool) -> Unmarshal<'_> {
    Unmarshal { bytes, at: 0, little }
}

fn hex(lines: &[&str]) -> Vec<u8> {
    let hex = lines.concat();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

/// `org.freedesktop.DBus.Properties.GetAll`'s answer, an `a{sv}`, written
/// by hand from the D-Bus specification, little-endian: `WirelessEnabled`
/// `b` true, `State` `u` 70, `Version` `s` "1.46.0", `PrimaryConnection` `o`,
/// `Devices` `ao` of two paths, `Strength` `y` 87, `Signal` `i` -61, `Metric`
/// `n` -3, `Mtu` `q` 1500, `Offset` `x` -42, `Timestamp` `t` 1729584000123,
/// `Ratio` `d` 0.5 and `Kind` `g` "a{sv}".
const GET_ALL_LITTLE: &[&str] = &[
    "db010000000000000f000000576972656c657373456e61626c6564000162000001000000000000000500000053746174",
    "650001750000000046000000000000000700000056657273696f6e000173000006000000312e34362e30000000000000",
    "110000005072696d617279436f6e6e656374696f6e00016f00000000320000002f6f72672f667265656465736b746f70",
    "2f4e6574776f726b4d616e616765722f416374697665436f6e6e656374696f6e2f330000000000000700000044657669",
    "6365730002616f005e000000290000002f6f72672f667265656465736b746f702f4e6574776f726b4d616e616765722f",
    "446576696365732f31000000290000002f6f72672f667265656465736b746f702f4e6574776f726b4d616e616765722f",
    "446576696365732f320000000000000008000000537472656e677468000179005700000000000000060000005369676e",
    "616c000169000000c3ffffff00000000060000004d657472696300016e00fdff030000004d74750001710000dc050000",
    "060000004f6666736574000178000000d6ffffffffffffff0900000054696d657374616d700001740000000000000000",
    "7b6c3db39201000005000000526174696f00016400000000000000000000e03f040000004b696e640001670005617b73",
    "767d00",
];

/// [`GET_ALL_LITTLE`], big-endian.
const GET_ALL_BIG: &[&str] = &[
    "000001db000000000000000f576972656c657373456e61626c6564000162000000000001000000000000000553746174",
    "650001750000000000000046000000000000000756657273696f6e000173000000000006312e34362e30000000000000",
    "000000115072696d617279436f6e6e656374696f6e00016f00000000000000322f6f72672f667265656465736b746f70",
    "2f4e6574776f726b4d616e616765722f416374697665436f6e6e656374696f6e2f330000000000000000000744657669",
    "6365730002616f000000005e000000292f6f72672f667265656465736b746f702f4e6574776f726b4d616e616765722f",
    "446576696365732f31000000000000292f6f72672f667265656465736b746f702f4e6574776f726b4d616e616765722f",
    "446576696365732f320000000000000000000008537472656e677468000179005700000000000000000000065369676e",
    "616c000169000000ffffffc300000000000000064d657472696300016e00fffd000000034d7475000171000005dc0000",
    "000000064f6666736574000178000000ffffffffffffffd60000000954696d657374616d700001740000000000000000",
    "00000192b33d6c7b00000005526174696f000164000000003fe0000000000000000000044b696e640001670005617b73",
    "767d00",
];

const ACTIVE_CONNECTION: &str = "/org/freedesktop/NetworkManager/ActiveConnection/3";
const DEVICES: [&str; 2] = [
    "/org/freedesktop/NetworkManager/Devices/1",
    "/org/freedesktop/NetworkManager/Devices/2",
];

#[derive(Debug, PartialEq)]
enum Value<'a> {
    Bool(bool),
    Byte(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    Str(&'a str),
    Signature(&'a str),
    Paths(Vec<&'a str>),
}

/// The value of type `signature` next in `read`.
fn value<'a>(read: &mut Unmarshal<'a>, signature: &str) -> Option<Value<'a>> {
    Some(match signature {
        "b" => Value::Bool(read.bool()?),
        "y" => Value::Byte(read.byte()?),
        "n" => Value::I16(read.i16()?),
        "q" => Value::U16(read.u16()?),
        "i" => Value::I32(read.i32()?),
        "u" => Value::U32(read.u32()?),
        "x" => Value::I64(read.i64()?),
        "t" => Value::U64(read.u64()?),
        "d" => Value::F64(read.f64()?),
        "s" | "o" => Value::Str(read.string()?),
        "g" => Value::Signature(read.signature()?),
        "ao" => {
            let mut elements = read.array("o")?;
            let mut paths = Vec::new();
            while !elements.is_empty() {
                paths.push(elements.string()?);
            }
            Value::Paths(paths)
        }
        _ => return None,
    })
}

/// Every property in `body`, an `a{sv}` and nothing after it.
fn properties(mut body: Unmarshal<'_>) -> Option<Vec<(&str, Value<'_>)>> {
    let mut entries = body.array("{sv}")?;
    let mut all = Vec::new();
    while !entries.is_empty() {
        entries.align_struct();
        let name = entries.string()?;
        let signature = entries.variant()?;
        all.push((name, value(&mut entries, signature)?));
    }
    body.is_empty().then_some(all)
}

fn get_all_properties() -> Vec<(&'static str, Value<'static>)> {
    vec![
        ("WirelessEnabled", Value::Bool(true)),
        ("State", Value::U32(70)),
        ("Version", Value::Str("1.46.0")),
        ("PrimaryConnection", Value::Str(ACTIVE_CONNECTION)),
        ("Devices", Value::Paths(DEVICES.to_vec())),
        ("Strength", Value::Byte(87)),
        ("Signal", Value::I32(-61)),
        ("Metric", Value::I16(-3)),
        ("Mtu", Value::U16(1500)),
        ("Offset", Value::I64(-42)),
        ("Timestamp", Value::U64(1_729_584_000_123)),
        ("Ratio", Value::F64(0.5)),
        ("Kind", Value::Signature("a{sv}")),
    ]
}

#[test]
fn a_get_of_a_bool_reads_its_variant_in_either_byte_order() {
    let little = reply(true, "v", &[1, b'b', 0, 0, 1, 0, 0, 0]);
    let big = reply(false, "v", &[1, b'b', 0, 0, 0, 0, 0, 1]);
    let off = reply(true, "v", &[1, b'b', 0, 0, 0, 0, 0, 0]);

    for (answer, expected) in [(little, true), (big, true), (off, false)] {
        let mut body = answer.body();
        assert_eq!(body.variant(), Some("b"));
        assert_eq!(body.bool(), Some(expected));
        assert!(body.is_empty());
    }
}

#[test]
fn a_get_of_a_string_reads_its_variant_in_either_byte_order() {
    let name = b"wlp2s0\0";
    let little = reply(true, "v", &[&[1, b's', 0, 0, 6, 0, 0, 0][..], name].concat());
    let big = reply(false, "v", &[&[1, b's', 0, 0, 0, 0, 0, 6][..], name].concat());

    for answer in [little, big] {
        let mut body = answer.body();
        assert_eq!(body.variant(), Some("s"));
        assert_eq!(body.string(), Some("wlp2s0"));
        assert!(body.is_empty());
    }
}

#[test]
fn a_get_of_an_object_path_reads_its_variant_in_either_byte_order() {
    let path = b"/org/freedesktop/NetworkManager/ActiveConnection/3\0";
    let little = reply(true, "v", &[&[1, b'o', 0, 0, 50, 0, 0, 0][..], path].concat());
    let big = reply(false, "v", &[&[1, b'o', 0, 0, 0, 0, 0, 50][..], path].concat());

    for answer in [little, big] {
        let mut body = answer.body();
        assert_eq!(body.variant(), Some("o"));
        assert_eq!(body.string(), Some(ACTIVE_CONNECTION));
        assert!(body.is_empty());
    }
}

#[test]
fn a_get_all_reads_every_property_in_either_byte_order() {
    let little = reply(true, "a{sv}", &hex(GET_ALL_LITTLE));
    let big = reply(false, "a{sv}", &hex(GET_ALL_BIG));

    assert_eq!(properties(little.body()), Some(get_all_properties()));
    assert_eq!(properties(big.body()), Some(get_all_properties()));
}

#[test]
fn a_get_all_marshalled_here_is_the_bytes_the_specification_gives() {
    let mut m = Marshal::default();
    m.array(8, |m| {
        let mut entry = |name: &str, signature: &str, value: &dyn Fn(&mut Marshal)| {
            m.pad(8);
            m.string(name);
            m.signature(signature);
            value(m);
        };
        entry("WirelessEnabled", "b", &|m| m.bool(true));
        entry("State", "u", &|m| m.u32(70));
        entry("Version", "s", &|m| m.string("1.46.0"));
        entry("PrimaryConnection", "o", &|m| m.string(ACTIVE_CONNECTION));
        entry("Devices", "ao", &|m| {
            m.array(4, |m| {
                for device in DEVICES {
                    m.string(device);
                }
            });
        });
        entry("Strength", "y", &|m| m.byte(87));
        entry("Signal", "i", &|m| m.i32(-61));
        entry("Metric", "n", &|m| m.i16(-3));
        entry("Mtu", "q", &|m| m.u16(1500));
        entry("Offset", "x", &|m| m.i64(-42));
        entry("Timestamp", "t", &|m| m.u64(1_729_584_000_123));
        entry("Ratio", "d", &|m| m.f64(0.5));
        entry("Kind", "g", &|m| m.signature("a{sv}"));
    });

    assert_eq!(m.0, hex(GET_ALL_LITTLE));
    assert_eq!(properties(body(&m.0, true)), Some(get_all_properties()));
}

#[test]
fn every_number_reads_back_as_marshalled_at_its_extremes() {
    let mut m = Marshal::default();
    m.byte(u8::MAX);
    m.bool(false);
    m.i16(i16::MIN);
    m.u16(u16::MAX);
    m.i32(i32::MIN);
    m.u32(u32::MAX);
    m.i64(i64::MIN);
    m.u64(u64::MAX);
    m.f64(-1.5e300);

    let mut read = body(&m.0, true);
    assert_eq!(read.byte(), Some(u8::MAX));
    assert_eq!(read.bool(), Some(false));
    assert_eq!(read.i16(), Some(i16::MIN));
    assert_eq!(read.u16(), Some(u16::MAX));
    assert_eq!(read.i32(), Some(i32::MIN));
    assert_eq!(read.u32(), Some(u32::MAX));
    assert_eq!(read.i64(), Some(i64::MIN));
    assert_eq!(read.u64(), Some(u64::MAX));
    assert_eq!(read.f64().map(f64::to_bits), Some((-1.5e300_f64).to_bits()));
    assert!(read.is_empty());
}

#[test]
fn a_property_not_wanted_is_skipped_whatever_its_type() {
    for (bytes, little) in [(hex(GET_ALL_LITTLE), true), (hex(GET_ALL_BIG), false)] {
        let mut entries = body(&bytes, little).array("{sv}").unwrap();
        let mut mtu = None;
        while !entries.is_empty() {
            entries.align_struct();
            let name = entries.string().unwrap();
            let signature = entries.variant().unwrap();
            if name == "Mtu" {
                mtu = entries.u16();
            } else {
                entries.skip(signature).unwrap();
            }
        }
        assert_eq!(mtu, Some(1500));

        let mut whole = body(&bytes, little);
        assert_eq!(whole.skip("a{sv}"), Some(()));
        assert!(whole.is_empty());
    }
}

#[test]
fn a_boolean_other_than_0_or_1_is_malformed() {
    let answer = reply(true, "v", &[1, b'b', 0, 0, 2, 0, 0, 0]);

    let mut read = answer.body();
    assert_eq!(read.variant(), Some("b"));
    assert_eq!(read.bool(), None);
    assert_eq!(answer.body().skip("v"), None);
}

#[test]
fn a_get_all_cut_short_anywhere_is_malformed() {
    for (bytes, little) in [(hex(GET_ALL_LITTLE), true), (hex(GET_ALL_BIG), false)] {
        for end in 0..bytes.len() {
            let cut = &bytes[..end];
            assert_eq!(properties(body(cut, little)), None, "cut at {end}");
            assert_eq!(body(cut, little).skip("a{sv}"), None, "cut at {end}");
        }
    }
}

#[test]
fn a_get_all_whose_array_ends_early_reads_whole_properties_only() {
    let expected = get_all_properties();
    for (bytes, little) in [(hex(GET_ALL_LITTLE), true), (hex(GET_ALL_BIG), false)] {
        for end in 8..bytes.len() {
            let mut cut = bytes[..end].to_vec();
            let len = (end - 8) as u32;
            cut[..4].copy_from_slice(&if little { len.to_le_bytes() } else { len.to_be_bytes() });

            // Cut between two properties, it is the properties before the cut.
            if let Some(read) = properties(body(&cut, little)) {
                assert_eq!(read, expected[..read.len()], "cut at {end}");
                assert!(read.len() < expected.len(), "cut at {end}");
            }
            let _ = body(&cut, little).skip("a{sv}");
        }
    }
}

#[test]
fn a_string_without_its_nul_or_longer_than_the_body_is_malformed() {
    assert_eq!(body(&[2, 0, 0, 0, b'h', b'i', b'!'], true).string(), None);
    assert_eq!(body(&[9, 0, 0, 0, b'h', b'i', 0], true).string(), None);
    assert_eq!(body(&[u8::MAX; 8], true).string(), None);
    assert_eq!(body(&[2, b'a', b'y'], true).signature(), None);
    assert!(body(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0], true).array("y").is_none());
}

#[test]
fn a_variant_holds_one_single_type() {
    for signature in ["", "ii", "a", "(", "()", "(i", "{sv}", "a{vs}", "a{s}", "a{sss}", "z"] {
        let mut bytes = vec![signature.len() as u8];
        bytes.extend(signature.as_bytes());
        bytes.extend([0; 16]);
        assert_eq!(body(&bytes, true).variant(), None, "{signature:?}");
    }
    for signature in ["y", "as", "a{sv}", "(ia{oas})", "aav", "a{ya{sv}}"] {
        let mut bytes = vec![signature.len() as u8];
        bytes.extend(signature.as_bytes());
        bytes.push(0);
        assert_eq!(body(&bytes, true).variant(), Some(signature));
    }
}

#[test]
fn nesting_deeper_than_the_specification_allows_is_malformed() {
    let nested = |depth: usize| {
        let mut bytes = [1, b'v', 0].repeat(depth);
        bytes.extend([1, b'y', 0, 7]);
        bytes
    };

    let shallow = nested(10);
    let mut read = body(&shallow, true);
    assert_eq!(read.skip("v"), Some(()));
    assert!(read.is_empty());
    assert_eq!(body(&nested(100_000), true).skip("v"), None);
    assert_eq!(body(&[0; 8], true).skip(&format!("{}y", "a".repeat(100))), None);
}

#[test]
fn an_array_of_empty_structs_is_malformed_rather_than_endless() {
    assert_eq!(
        body(&[8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], true).skip("a()"),
        None
    );
}

#[test]
fn bytes_of_any_kind_never_panic_a_reader() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let signatures = ["a{sv}", "v", "(yaxs)", "aav", "a{oa{sa{sv}}}", "g", "ad"];

    for _ in 0..20_000 {
        let len = (next() % 64) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        // A small first word, so an array or a string sometimes fits.
        if let Some(first) = bytes.first_mut() {
            *first %= 48;
        }
        for little in [true, false] {
            for signature in signatures {
                let _ = body(&bytes, little).skip(signature);
            }
            let _ = properties(body(&bytes, little));
        }
    }
}
