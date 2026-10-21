//! Just enough D-Bus for a few calls: connecting, authenticating, calling a
//! method and reading the messages that come back, on a socket that lives as
//! long as the calls.
//!
//! Kotodama calls D-Bus only on rare events, a notification when a job ends,
//! a scope for a program it starts, or logind when the user powers off, so a
//! D-Bus library would keep a connection and a thread for the process's
//! whole life to save a few milliseconds on each.
//!
//! # Owns
//!
//! Finding and connecting to the session bus, the system bus and the systemd
//! user manager, the EXTERNAL authentication, marshalling method calls and
//! their returns, and reading calls, replies and signals.
//!
//! # Not here
//!
//! What is called: notifications are `kotodamad`'s `notify`, scopes
//! `crate::scope`, logind the `system` Extension's.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// How long the bus and the service called get to answer.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A message from the bus larger than this isn't one we are waiting for.
const MAX_MESSAGE: usize = 1 << 20;

const METHOD_CALL: u8 = 1;
const METHOD_RETURN: u8 = 2;
const ERROR: u8 = 3;
const SIGNAL: u8 = 4;

const PATH: u8 = 1;
const INTERFACE: u8 = 2;
const MEMBER: u8 = 3;
const ERROR_NAME: u8 = 4;
const REPLY_SERIAL: u8 = 5;
const DESTINATION: u8 = 6;
const SENDER: u8 = 7;
const SIGNATURE: u8 = 8;

/// Where the system bus listens when `DBUS_SYSTEM_BUS_ADDRESS` is unset, as
/// the D-Bus specification fixes it.
const SYSTEM_BUS: &str = "unix:path=/var/run/dbus/system_bus_socket";

/// What went wrong talking over D-Bus.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Neither `DBUS_SESSION_BUS_ADDRESS` nor `XDG_RUNTIME_DIR` is set, or
    /// the address names no Unix socket.
    #[error("no address for the bus")]
    NoAddress,
    #[error("connecting to the bus")]
    Connect(#[source] io::Error),
    /// The peer turned down the EXTERNAL authentication, with this line.
    #[error("the bus refused us: {0}")]
    Refused(String),
    #[error("talking to the bus")]
    Io(#[source] io::Error),
    #[error("the bus sent a message too large to be an answer")]
    TooLarge,
    /// The call failed: the error's name and message.
    #[error("the call failed: {0}")]
    Failed(String),
}

impl Error {
    /// Whether there was nothing to connect to: no session bus, or no
    /// systemd user manager.
    #[must_use]
    pub fn is_absent(&self) -> bool {
        match self {
            Error::NoAddress => true,
            Error::Connect(err) => matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ),
            _ => false,
        }
    }
}

/// A connection to a bus, or to a peer such as the systemd user manager.
#[derive(Debug)]
pub struct Bus {
    stream: UnixStream,
    /// The serial of the last message sent.
    serial: u32,
}

impl Bus {
    /// Connects to the first address in `DBUS_SESSION_BUS_ADDRESS` that is a
    /// Unix socket, or to `$XDG_RUNTIME_DIR/bus`, where systemd puts the
    /// session bus, and says hello. The hello's answer is read with the
    /// first call's, so it costs no round trip of its own.
    ///
    /// # Errors
    ///
    /// When there is no session bus, or it can't be reached or refuses us.
    pub fn session() -> Result<Bus, Error> {
        Bus::hello(session_stream()?)
    }

    /// Connects to the system bus, at `DBUS_SYSTEM_BUS_ADDRESS` or where the
    /// D-Bus specification puts it, and says hello as [`Bus::session`] does.
    ///
    /// # Errors
    ///
    /// When there is no system bus ([`Error::is_absent`]), or it can't be
    /// reached or refuses us.
    pub fn system() -> Result<Bus, Error> {
        let address = std::env::var("DBUS_SYSTEM_BUS_ADDRESS");
        Bus::at(address.as_deref().unwrap_or(SYSTEM_BUS))
    }

    /// Connects to the bus at `address`, a D-Bus address such as
    /// `unix:path=/run/dbus/system_bus_socket`, and says hello as
    /// [`Bus::session`] does.
    ///
    /// # Errors
    ///
    /// When `address` names no Unix socket, or the bus can't be reached or
    /// refuses us.
    pub fn at(address: &str) -> Result<Bus, Error> {
        Bus::hello(connect_address(address)?)
    }

    fn hello(stream: UnixStream) -> Result<Bus, Error> {
        let mut bus = Bus::authenticated(stream)?;
        let hello = bus.call_message(&Call::new(
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
            "org.freedesktop.DBus",
        ));
        bus.write(&hello)?;
        Ok(bus)
    }

    /// Connects to the systemd user manager's own socket,
    /// `$XDG_RUNTIME_DIR/systemd/private`, as `systemctl --user` does. It is
    /// a peer, not a bus: it takes no hello, and sends its signals to every
    /// peer without being asked.
    ///
    /// # Errors
    ///
    /// When there is no user manager ([`Error::is_absent`]), or it can't be
    /// reached or refuses us.
    pub fn user_manager() -> Result<Bus, Error> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::NoAddress)?;
        let path = Path::new(&dir).join("systemd/private");
        let stream = UnixStream::connect(path).map_err(Error::Connect)?;
        Bus::authenticated(stream)
    }

    fn authenticated(stream: UnixStream) -> Result<Bus, Error> {
        stream.set_read_timeout(Some(TIMEOUT)).map_err(Error::Io)?;
        stream.set_write_timeout(Some(TIMEOUT)).map_err(Error::Io)?;
        let mut bus = Bus { stream, serial: 0 };
        bus.authenticate()?;
        Ok(bus)
    }

    /// The EXTERNAL mechanism: the peer checks the uid on the socket against
    /// the one claimed. `BEGIN` goes out with the first message.
    fn authenticate(&mut self) -> Result<(), Error> {
        let uid: String = uid()
            .to_string()
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.stream
            .write_all(format!("\0AUTH EXTERNAL {uid}\r\n").as_bytes())
            .map_err(Error::Io)?;
        let mut line = Vec::new();
        let mut byte = [0];
        while !line.ends_with(b"\r\n") {
            self.stream.read_exact(&mut byte).map_err(Error::Io)?;
            line.push(byte[0]);
            if line.len() > 512 {
                break;
            }
        }
        if !line.starts_with(b"OK ") {
            return Err(Error::Refused(
                String::from_utf8_lossy(&line).trim().to_owned(),
            ));
        }
        self.stream.write_all(b"BEGIN\r\n").map_err(Error::Io)
    }

    /// Waits for messages without a time limit from now on, as a service
    /// waiting for calls does.
    ///
    /// # Errors
    ///
    /// When the socket refuses the change.
    pub fn wait_forever(&mut self) -> Result<(), Error> {
        self.stream.set_read_timeout(None).map_err(Error::Io)
    }

    /// Answers the method call `to` with a return whose body, of type
    /// `signature`, is `body`.
    ///
    /// # Errors
    ///
    /// When it can't be written.
    pub fn reply(&mut self, to: &Message, signature: &str, body: Marshal) -> Result<(), Error> {
        self.serial += 1;
        let mut m = Marshal::default();
        // Little-endian, a method return, no flags, protocol version 1.
        m.0.extend_from_slice(&[b'l', METHOD_RETURN, 0, 1]);
        m.u32(body.0.len() as u32);
        m.u32(self.serial);
        m.array(8, |m| {
            m.pad(8);
            m.byte(REPLY_SERIAL);
            m.signature("u");
            m.u32(to.serial);
            if let Some(sender) = &to.sender {
                m.field(DESTINATION, "s", sender);
            }
            if !signature.is_empty() {
                m.field(SIGNATURE, "g", signature);
            }
        });
        m.pad(8);
        m.0.extend_from_slice(&body.0);
        self.write(&m.0)
    }

    /// Sends `call` and returns its serial, without waiting for the answer.
    ///
    /// # Errors
    ///
    /// When it can't be written.
    pub fn send(&mut self, call: &Call) -> Result<u32, Error> {
        let message = self.call_message(call);
        self.write(&message)?;
        Ok(self.serial)
    }

    /// Sends `call` and waits for its answer; the messages before it are
    /// passed to `meanwhile`.
    ///
    /// # Errors
    ///
    /// When the call can't be made or answered in time, or answers with an
    /// error.
    pub fn call(
        &mut self,
        call: &Call,
        mut meanwhile: impl FnMut(&Message),
    ) -> Result<Message, Error> {
        let serial = self.send(call)?;
        loop {
            let message = self.read()?;
            if message.reply_serial != Some(serial) {
                meanwhile(&message);
                continue;
            }
            return match message.kind {
                METHOD_RETURN => Ok(message),
                _ => Err(Error::Failed(message.error())),
            };
        }
    }

    fn call_message(&mut self, call: &Call) -> Vec<u8> {
        self.serial += 1;
        call.encode(self.serial)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.stream.write_all(bytes).map_err(Error::Io)
    }

    /// Reads the next message.
    ///
    /// # Errors
    ///
    /// When none comes in time, or it can't be read.
    pub fn read(&mut self) -> Result<Message, Error> {
        let mut fixed = [0u8; 16];
        self.stream.read_exact(&mut fixed).map_err(Error::Io)?;
        let little = fixed[0] == b'l';
        let word = |at: usize| {
            let word = [fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]];
            if little {
                u32::from_le_bytes(word)
            } else {
                u32::from_be_bytes(word)
            }
        };
        let body_len = word(4) as usize;
        let fields_len = word(12) as usize;
        let rest = fields_len.next_multiple_of(8) + body_len;
        if rest > MAX_MESSAGE {
            return Err(Error::TooLarge);
        }
        let mut bytes = vec![0; rest];
        self.stream.read_exact(&mut bytes).map_err(Error::Io)?;
        let body = bytes.split_off(fields_len.next_multiple_of(8));
        bytes.truncate(fields_len);
        let mut message = Message {
            kind: fixed[1],
            little,
            serial: word(8),
            reply_serial: None,
            path: None,
            interface: None,
            sender: None,
            member: None,
            error_name: None,
            signature: String::new(),
            body,
        };
        message.read_fields(&bytes);
        Ok(message)
    }
}

/// A method call.
#[derive(Clone, Debug)]
pub struct Call<'a> {
    pub path: &'a str,
    pub interface: &'a str,
    pub member: &'a str,
    pub destination: &'a str,
    /// The body's signature; empty for no body.
    pub signature: &'a str,
    /// The body, marshalled with [`Marshal`].
    pub body: Vec<u8>,
}

impl<'a> Call<'a> {
    /// A call with no arguments.
    #[must_use]
    pub fn new(path: &'a str, interface: &'a str, member: &'a str, destination: &'a str) -> Self {
        Call {
            path,
            interface,
            member,
            destination,
            signature: "",
            body: Vec::new(),
        }
    }

    /// The call with arguments: `body`, of type `signature`.
    #[must_use]
    pub fn with_body(mut self, signature: &'a str, body: Marshal) -> Self {
        self.signature = signature;
        self.body = body.0;
        self
    }

    fn encode(&self, serial: u32) -> Vec<u8> {
        let mut m = Marshal::default();
        // Little-endian, a method call, no flags, protocol version 1.
        m.0.extend_from_slice(&[b'l', 1, 0, 1]);
        m.u32(self.body.len() as u32);
        m.u32(serial);
        m.array(8, |m| {
            m.field(PATH, "o", self.path);
            m.field(INTERFACE, "s", self.interface);
            m.field(MEMBER, "s", self.member);
            m.field(DESTINATION, "s", self.destination);
            if !self.signature.is_empty() {
                m.field(SIGNATURE, "g", self.signature);
            }
        });
        m.pad(8);
        m.0.extend_from_slice(&self.body);
        m.0
    }
}

/// D-Bus marshalling, little-endian. Offsets count from the start of the
/// buffer, which must sit at an 8-byte boundary of the message, as a body
/// does.
#[derive(Debug, Default)]
pub struct Marshal(Vec<u8>);

impl Marshal {
    pub fn pad(&mut self, align: usize) {
        while !self.0.len().is_multiple_of(align) {
            self.0.push(0);
        }
    }

    pub fn byte(&mut self, byte: u8) {
        self.0.push(byte);
    }

    pub fn u32(&mut self, value: u32) {
        self.pad(4);
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn i32(&mut self, value: i32) {
        self.pad(4);
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    /// A string or an object path.
    pub fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
    }

    pub fn signature(&mut self, value: &str) {
        self.byte(value.len() as u8);
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
    }

    /// An array whose elements align to `align` and are written by
    /// `elements`.
    pub fn array(&mut self, align: usize, elements: impl FnOnce(&mut Self)) {
        self.u32(0);
        let at = self.0.len() - 4;
        // An empty array still pads to its first element.
        self.pad(align);
        let start = self.0.len();
        elements(self);
        let len = (self.0.len() - start) as u32;
        self.0[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }

    /// A struct of `(name, variant)`, as a property in an `a(sv)`.
    pub fn property(&mut self, name: &str, signature: &str, value: impl FnOnce(&mut Self)) {
        self.pad(8);
        self.string(name);
        self.signature(signature);
        value(self);
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

/// A message read from a bus: a method call, a method's return or error, or
/// a signal.
#[derive(Debug)]
pub struct Message {
    kind: u8,
    little: bool,
    serial: u32,
    reply_serial: Option<u32>,
    path: Option<String>,
    interface: Option<String>,
    /// The unique name of the connection that sent it; the bus fills it in.
    sender: Option<String>,
    member: Option<String>,
    error_name: Option<String>,
    signature: String,
    body: Vec<u8>,
}

impl Message {
    /// Whether it is the signal `member`, whose body is of type `signature`.
    #[must_use]
    pub fn is_signal(&self, member: &str, signature: &str) -> bool {
        self.kind == SIGNAL && self.member.as_deref() == Some(member) && self.signature == signature
    }

    /// Whether it is a call of `interface.member` on the object `path`.
    #[must_use]
    pub fn is_call(&self, path: &str, interface: &str, member: &str) -> bool {
        self.kind == METHOD_CALL
            && self.path.as_deref() == Some(path)
            && self.interface.as_deref() == Some(interface)
            && self.member.as_deref() == Some(member)
    }

    /// Reads its body, which starts at an 8-byte boundary.
    #[must_use]
    pub fn body(&self) -> Unmarshal<'_> {
        Unmarshal {
            bytes: &self.body,
            at: 0,
            little: self.little,
        }
    }

    /// An error's name and message, as `name: message`.
    fn error(&self) -> String {
        let name = self.error_name.as_deref().unwrap_or("an unnamed error");
        if self.kind != ERROR || !self.signature.starts_with('s') {
            return name.to_owned();
        }
        match self.body().string() {
            Some(message) => format!("{name}: {message}"),
            None => name.to_owned(),
        }
    }

    /// Reads the header fields it needs out of `fields`.
    fn read_fields(&mut self, fields: &[u8]) {
        let mut read = Unmarshal {
            bytes: fields,
            at: 0,
            little: self.little,
        };
        while read.at < fields.len() {
            read.align(8);
            let (Some(code), Some(signature)) = (read.byte(), read.signature()) else {
                return;
            };
            let read_one = match signature {
                "u" => read.u32().map(|value| {
                    if code == REPLY_SERIAL {
                        self.reply_serial = Some(value);
                    }
                }),
                "s" | "o" => read.string().map(|value| match code {
                    PATH => self.path = Some(value.to_owned()),
                    INTERFACE => self.interface = Some(value.to_owned()),
                    MEMBER => self.member = Some(value.to_owned()),
                    ERROR_NAME => self.error_name = Some(value.to_owned()),
                    SENDER => self.sender = Some(value.to_owned()),
                    _ => {}
                }),
                "g" => read.signature().map(|value| {
                    if code == SIGNATURE {
                        self.signature = value.to_owned();
                    }
                }),
                _ => None,
            };
            if read_one.is_none() {
                return;
            }
        }
    }
}

/// Reads marshalled values in order.
#[derive(Debug)]
pub struct Unmarshal<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
}

impl<'a> Unmarshal<'a> {
    fn align(&mut self, align: usize) {
        self.at = self.at.next_multiple_of(align);
    }

    fn byte(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    /// The next `u32`, or `None` past the end.
    pub fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let word: [u8; 4] = self.bytes.get(self.at..self.at + 4)?.try_into().ok()?;
        self.at += 4;
        Some(if self.little {
            u32::from_le_bytes(word)
        } else {
            u32::from_be_bytes(word)
        })
    }

    /// The next string or object path, or `None` past the end or if it
    /// isn't UTF-8.
    pub fn string(&mut self) -> Option<&'a str> {
        let len = self.u32()? as usize;
        let text = self.bytes.get(self.at..self.at + len)?;
        self.at += len + 1;
        std::str::from_utf8(text).ok()
    }

    fn signature(&mut self) -> Option<&'a str> {
        let len = usize::from(self.byte()?);
        let text = self.bytes.get(self.at..self.at + len)?;
        self.at += len + 1;
        std::str::from_utf8(text).ok()
    }
}

fn session_stream() -> Result<UnixStream, Error> {
    let Some(addresses) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS") else {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::NoAddress)?;
        return UnixStream::connect(Path::new(&dir).join("bus")).map_err(Error::Connect);
    };
    connect_address(&addresses.to_string_lossy())
}

/// Connects to the first Unix socket in `addresses`, a D-Bus address list
/// separated by `;`.
fn connect_address(addresses: &str) -> Result<UnixStream, Error> {
    let mut last = Error::NoAddress;
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
                Err(err) => last = Error::Connect(err),
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

/// Also a fake peer for the tests of what calls over D-Bus.
#[cfg(test)]
pub(crate) mod tests;
