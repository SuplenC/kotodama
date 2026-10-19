//! Desktop notifications through `org.freedesktop.Notifications` on the session
//! bus.
//!
//! The Daemon sends one only when a job finishes, so it speaks just enough
//! D-Bus for that one call: it connects, authenticates, says hello, calls
//! `Notify`, waits for the answer and hangs up. A D-Bus library would keep a
//! connection and a thread for the Daemon's whole life to save a few
//! milliseconds on a rare event.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// How long the bus and the notification service get to answer.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A message from the bus larger than this isn't one we are waiting for.
const MAX_MESSAGE: usize = 1 << 20;

const HELLO: u32 = 1;
const NOTIFY: u32 = 2;

/// Shows a notification with `summary` and `body`. Blocks until the
/// notification service has answered, so call it off the main loop.
pub fn send(summary: &str, body: &str) -> io::Result<()> {
    let mut bus = connect()?;
    bus.set_read_timeout(Some(TIMEOUT))?;
    bus.set_write_timeout(Some(TIMEOUT))?;
    authenticate(&mut bus)?;
    let mut out = method_call(
        HELLO,
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "Hello",
        "org.freedesktop.DBus",
        None,
        &[],
    );
    out.extend(method_call(
        NOTIFY,
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
        "org.freedesktop.Notifications",
        Some("susssasa{sv}i"),
        &notify_body(summary, body),
    ));
    bus.write_all(&out)?;
    wait_for_reply(&mut bus, NOTIFY)
}

/// Connects to the first address in `DBUS_SESSION_BUS_ADDRESS` that is a Unix
/// socket, or to `$XDG_RUNTIME_DIR/bus`, where systemd puts the session bus.
fn connect() -> io::Result<UnixStream> {
    let not_found = || io::Error::new(io::ErrorKind::NotFound, "no session bus address");
    let Some(addresses) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS") else {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(not_found)?;
        return UnixStream::connect(std::path::Path::new(&dir).join("bus"));
    };
    let addresses = addresses.to_string_lossy();
    let mut last = not_found();
    for address in addresses.split(';') {
        let Some(params) = address.strip_prefix("unix:") else {
            continue;
        };
        for param in params.split(',') {
            let connected = match param.split_once('=') {
                Some(("path", path)) => UnixStream::connect(unescape(path)),
                Some(("abstract", name)) => connect_abstract(&unescape(name)),
                _ => continue,
            };
            match connected {
                Ok(stream) => return Ok(stream),
                Err(err) => last = err,
            }
        }
    }
    Err(last)
}

#[cfg(target_os = "linux")]
fn connect_abstract(name: &str) -> io::Result<UnixStream> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::SocketAddr;
    UnixStream::connect_addr(&SocketAddr::from_abstract_name(name)?)
}

#[cfg(not(target_os = "linux"))]
fn connect_abstract(_name: &str) -> io::Result<UnixStream> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Undoes the `%xx` escapes a D-Bus address value may have.
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = value
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The EXTERNAL mechanism: the bus checks the uid on the socket against the
/// one claimed.
fn authenticate(bus: &mut UnixStream) -> io::Result<()> {
    let uid: String = uid()
        .to_string()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    bus.write_all(format!("\0AUTH EXTERNAL {uid}\r\n").as_bytes())?;
    let mut line = Vec::new();
    let mut byte = [0];
    while !line.ends_with(b"\r\n") {
        bus.read_exact(&mut byte)?;
        line.push(byte[0]);
        if line.len() > 512 {
            break;
        }
    }
    if !line.starts_with(b"OK ") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "the session bus refused us: {}",
                String::from_utf8_lossy(&line).trim()
            ),
        ));
    }
    bus.write_all(b"BEGIN\r\n")
}

#[cfg(target_os = "linux")]
fn uid() -> u32 {
    // SAFETY: getuid can't fail and has no preconditions.
    unsafe { libc::getuid() }
}

#[cfg(not(target_os = "linux"))]
fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(std::env::temp_dir()).map_or(0, |meta| meta.uid())
}

/// D-Bus marshalling, little-endian. Offsets count from the start of the
/// buffer, which must sit at an 8-byte boundary of the message.
#[derive(Default)]
struct Marshal(Vec<u8>);

impl Marshal {
    fn pad(&mut self, align: usize) {
        while !self.0.len().is_multiple_of(align) {
            self.0.push(0);
        }
    }

    fn byte(&mut self, byte: u8) {
        self.0.push(byte);
    }

    fn u32(&mut self, value: u32) {
        self.pad(4);
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.pad(4);
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    /// A string or an object path.
    fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
    }

    fn signature(&mut self, value: &str) {
        self.byte(value.len() as u8);
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
    }

    /// One header field: its code and a variant holding a string-like value.
    fn field(&mut self, code: u8, signature: &str, value: &str) {
        self.pad(8);
        self.byte(code);
        self.signature(signature);
        if signature == "g" {
            self.signature(value);
        } else {
            self.string(value);
        }
    }
}

fn method_call(
    serial: u32,
    path: &str,
    interface: &str,
    member: &str,
    destination: &str,
    signature: Option<&str>,
    body: &[u8],
) -> Vec<u8> {
    let mut m = Marshal::default();
    // Little-endian, a method call, no flags, protocol version 1.
    m.0.extend_from_slice(&[b'l', 1, 0, 1]);
    m.u32(body.len() as u32);
    m.u32(serial);
    m.u32(0); // The header fields' length, filled in below.
    let start = m.0.len();
    m.field(1, "o", path);
    m.field(2, "s", interface);
    m.field(3, "s", member);
    m.field(6, "s", destination);
    if let Some(signature) = signature {
        m.field(8, "g", signature);
    }
    let len = (m.0.len() - start) as u32;
    m.0[12..16].copy_from_slice(&len.to_le_bytes());
    m.pad(8);
    m.0.extend_from_slice(body);
    m.0
}

/// `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints,
/// expire_timeout)`.
fn notify_body(summary: &str, body: &str) -> Vec<u8> {
    let mut m = Marshal::default();
    m.string("Kotodama");
    m.u32(0);
    m.string("");
    m.string(summary);
    m.string(body);
    m.u32(0); // No actions.
    m.u32(0); // No hints; a dict's entries align to 8 even when there are none.
    m.pad(8);
    m.i32(-1); // The server's default timeout.
    m.0
}

/// Reads messages until the answer to `serial` arrives.
fn wait_for_reply(bus: &mut UnixStream, serial: u32) -> io::Result<()> {
    loop {
        let mut fixed = [0u8; 16];
        bus.read_exact(&mut fixed)?;
        let little = fixed[0] == b'l';
        let u32_at = |bytes: &[u8], at: usize| -> Option<u32> {
            let word: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
            Some(if little {
                u32::from_le_bytes(word)
            } else {
                u32::from_be_bytes(word)
            })
        };
        let kind = fixed[1];
        let body_len = u32_at(&fixed, 4).unwrap_or(0) as usize;
        let fields_len = u32_at(&fixed, 12).unwrap_or(0) as usize;
        let rest = fields_len.next_multiple_of(8) + body_len;
        if rest > MAX_MESSAGE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the session bus sent a message too large to be an answer",
            ));
        }
        let mut message = vec![0; rest];
        bus.read_exact(&mut message)?;
        let fields = &message[..fields_len];
        if reply_serial(fields, &u32_at) == Some(serial) {
            // 2 is a method return, 3 an error.
            return if kind == 3 {
                Err(io::Error::other(
                    "the notification service answered with an error",
                ))
            } else {
                Ok(())
            };
        }
    }
}

/// The REPLY_SERIAL header field, if the message has one.
fn reply_serial(fields: &[u8], u32_at: &impl Fn(&[u8], usize) -> Option<u32>) -> Option<u32> {
    let mut at = 0;
    while at < fields.len() {
        at = at.next_multiple_of(8);
        let code = *fields.get(at)?;
        let signature_len = *fields.get(at + 1)? as usize;
        let signature = fields.get(at + 2..at + 2 + signature_len)?;
        at += 2 + signature_len + 1;
        match signature {
            b"u" => {
                at = at.next_multiple_of(4);
                let value = u32_at(fields, at)?;
                if code == 5 {
                    return Some(value);
                }
                at += 4;
            }
            b"s" | b"o" => {
                at = at.next_multiple_of(4);
                at += 4 + u32_at(fields, at)? as usize + 1;
            }
            b"g" => at += 1 + *fields.get(at)? as usize + 1,
            _ => return None,
        }
    }
    None
}
