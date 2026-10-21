//! One end of a connection: framing, the handshake and the agreed Wire format
//! together.
//!
//! # Owns
//!
//! - [`Channel`], and its halves [`ChannelReader`] and [`ChannelWriter`] for a reading and a
//!   writing thread.
//!
//! # Not here
//!
//! - The frame itself: `frame.rs`. Which Wire format two hellos agree on: `handshake.rs`. What a
//!   peer on an older version is sent: `version.rs`.

use std::fmt;
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Error;
use crate::Hello;
use crate::Since;
use crate::Version;
use crate::WireFormat;
use crate::frame;
use crate::version;
use crate::version::PROTOCOL_VERSION;

/// Sends and receives whole messages in the Wire format agreed at the handshake.
/// Works over any byte streams: a Unix socket, or an Extension's stdio.
pub struct Channel<R, W: Write> {
    reader: BufReader<R>,
    writer: BufWriter<W>,
    format: WireFormat,
    /// The protocol version the other side named in its hello.
    peer: Version,
    buf: Vec<u8>,
}

impl<R, W: Write> fmt::Debug for Channel<R, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field("format", &self.format)
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

/// The largest frame buffer a [`ChannelReader`] keeps between messages.
const KEEP_BUFFER: usize = 64 * 1024;

/// The receiving half of a [`Channel`], for a thread that only reads.
pub struct ChannelReader<R> {
    reader: BufReader<R>,
    format: WireFormat,
    buf: Vec<u8>,
}

impl<R> fmt::Debug for ChannelReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelReader")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

/// The sending half of a [`Channel`], for a thread that only writes.
pub struct ChannelWriter<W: Write> {
    writer: BufWriter<W>,
    format: WireFormat,
    peer: Version,
    buf: Vec<u8>,
}

impl<W: Write> fmt::Debug for ChannelWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelWriter")
            .field("format", &self.format)
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

impl<R: Read, W: Write> Channel<R, W> {
    /// An Extension's or a Front end's side: sends `ours`, reads the Daemon's
    /// [`Hello`], and settles on the fastest Wire format both support. Both
    /// sides call theirs at once; neither waits for the other to go first.
    ///
    /// # Errors
    ///
    /// As [`Hello::accept_daemon`]: a Daemon older than `ours` is refused.
    pub fn handshake(reader: R, writer: W, ours: &Hello) -> Result<Self, Error> {
        Self::open(reader, writer, ours, Hello::accept_daemon)
    }

    /// The Daemon's side of [`Channel::handshake`].
    ///
    /// # Errors
    ///
    /// As [`Hello::accept_peer`]: a peer newer than `ours` is refused.
    pub fn accept(reader: R, writer: W, ours: &Hello) -> Result<Self, Error> {
        Self::open(reader, writer, ours, Hello::accept_peer)
    }

    fn open(
        reader: R,
        writer: W,
        ours: &Hello,
        agree: impl FnOnce(&Hello, &Hello) -> Result<WireFormat, Error>,
    ) -> Result<Self, Error> {
        let mut channel = Channel {
            reader: BufReader::new(reader),
            writer: BufWriter::new(writer),
            format: WireFormat::Json,
            peer: PROTOCOL_VERSION,
            buf: Vec::new(),
        };
        channel.send(ours)?;
        let peer: Hello = channel
            .recv()?
            .ok_or_else(|| Error::Io(std::io::ErrorKind::UnexpectedEof.into()))?;
        channel.format = agree(ours, &peer)?;
        channel.peer = peer.protocol();
        Ok(channel)
    }

    /// The protocol version the other side speaks.
    pub fn peer(&self) -> Version {
        self.peer
    }

    /// The Wire format agreed at the handshake.
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// Sends one message, without the fields the peer's version lacks, and
    /// flushes it.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] if the message doesn't encode in the agreed Wire
    /// format, and [`Error::Io`] if the stream can't be written.
    pub fn send<T: Serialize + ?Sized>(&mut self, message: &T) -> Result<(), Error> {
        self.buf.clear();
        self.format.encode_for_into(self.peer, message, &mut self.buf)?;
        frame::write(&mut self.writer, &self.buf).map_err(Error::Io)?;
        Ok(())
    }

    /// Whether the peer's version can read `message`; see [`ChannelWriter::reads`].
    pub fn reads<T: Since + ?Sized>(&self, message: &T) -> bool {
        reads(self.peer, message)
    }

    /// The next message, or `None` once the peer has closed the connection.
    ///
    /// # Errors
    ///
    /// As [`ChannelReader::recv`].
    pub fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        if !frame::read(&mut self.reader, &mut self.buf)? {
            return Ok(None);
        }
        self.format.decode(&self.buf).map(Some)
    }

    /// Splits the channel so one thread can read while another writes.
    pub fn split(self) -> (ChannelReader<R>, ChannelWriter<W>) {
        (
            ChannelReader {
                reader: self.reader,
                format: self.format,
                buf: self.buf,
            },
            ChannelWriter {
                writer: self.writer,
                format: self.format,
                peer: self.peer,
                buf: Vec::new(),
            },
        )
    }
}

impl<R: Read> ChannelReader<R> {
    /// The Wire format agreed at the handshake.
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// The next message, or `None` once the peer has closed the connection. A
    /// [`Error::Decode`] leaves the reader at the next frame, so a caller may
    /// skip the bad message and carry on.
    ///
    /// A reader lives as long as its connection, so after a large frame, such
    /// as a first batch of Items, it frees the buffer rather than keep it.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the stream fails or ends inside a frame,
    /// [`Error::BadFrameHeader`] and [`Error::FrameTooLong`] for a frame that
    /// isn't one, and [`Error::Decode`] for a payload that isn't a `T`.
    pub fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, Error> {
        if !frame::read(&mut self.reader, &mut self.buf)? {
            return Ok(None);
        }
        let message = self.format.decode(&self.buf).map(Some);
        if self.buf.capacity() > KEEP_BUFFER {
            self.buf = Vec::new();
        }
        message
    }

    /// Waits for the next message to start arriving, as long as a read
    /// waits, such as for the socket's read timeout, and takes nothing. True
    /// once some of it is buffered, or the peer has hung up, so
    /// [`recv`](Self::recv) has something to say at once.
    pub fn wait_for_more(&mut self) -> bool {
        self.reader.fill_buf().is_ok()
    }
}

impl<R> ChannelReader<R> {
    /// Whether some of the next frame has been read already: the peer sent
    /// it along with the last one. [`recv`](Self::recv) may still wait for
    /// the rest of it, but not for the peer to decide to send it.
    pub fn has_buffered(&self) -> bool {
        !self.reader.buffer().is_empty()
    }
}

impl<W: Write> ChannelWriter<W> {
    /// The Wire format agreed at the handshake.
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// The protocol version the other side speaks.
    pub fn peer(&self) -> Version {
        self.peer
    }

    /// Sends one message, without the fields the peer's version lacks, and
    /// flushes it.
    ///
    /// # Errors
    ///
    /// [`Error::Encode`] if the message doesn't encode in the agreed Wire
    /// format, and [`Error::Io`] if the stream can't be written.
    pub fn send<T: Serialize + ?Sized>(&mut self, message: &T) -> Result<(), Error> {
        self.buf.clear();
        self.format.encode_for_into(self.peer, message, &mut self.buf)?;
        frame::write(&mut self.writer, &self.buf).map_err(Error::Io)?;
        Ok(())
    }

    /// Whether the peer's version can read `message`: false for a message, or
    /// a value in one, that a newer minor version added, which the Daemon
    /// doesn't send that peer.
    pub fn reads<T: Since + ?Sized>(&self, message: &T) -> bool {
        reads(self.peer, message)
    }
}

fn reads<T: Since + ?Sized>(peer: Version, message: &T) -> bool {
    // A peer on this version or a newer one reads everything, without the
    // message being looked through.
    !version::older_minor(peer) || message.since() <= peer.minor
}
