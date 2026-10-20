//! One end of a connection: framing, the handshake and the agreed Wire format
//! together.

use std::io::{BufRead, BufReader, BufWriter, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Hello, WireFormat, frame};

/// Sends and receives whole messages in the Wire format agreed at the handshake.
/// Works over any byte streams: a Unix socket, or an Extension's stdio.
pub struct Channel<R, W: Write> {
    reader: BufReader<R>,
    writer: BufWriter<W>,
    format: WireFormat,
    buf: Vec<u8>,
}

/// The largest frame buffer a [`ChannelReader`] keeps between messages.
const KEEP_BUFFER: usize = 64 * 1024;

/// The receiving half of a [`Channel`], for a thread that only reads.
pub struct ChannelReader<R> {
    reader: BufReader<R>,
    format: WireFormat,
    buf: Vec<u8>,
}

/// The sending half of a [`Channel`], for a thread that only writes.
pub struct ChannelWriter<W: Write> {
    writer: BufWriter<W>,
    format: WireFormat,
    buf: Vec<u8>,
}

impl<R: Read, W: Write> Channel<R, W> {
    /// Sends `ours`, reads the peer's [`Hello`], and settles on the fastest Wire
    /// format both support. Both sides call this at once; neither waits for the
    /// other to go first.
    pub fn handshake(reader: R, writer: W, ours: &Hello) -> Result<Self, Error> {
        let mut channel = Channel {
            reader: BufReader::new(reader),
            writer: BufWriter::new(writer),
            format: WireFormat::Json,
            buf: Vec::new(),
        };
        channel.send(ours)?;
        let peer: Hello = channel
            .recv()?
            .ok_or_else(|| Error::Io(std::io::ErrorKind::UnexpectedEof.into()))?;
        channel.format = ours.negotiate(&peer)?;
        Ok(channel)
    }

    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// Sends one message and flushes it.
    pub fn send<T: Serialize + ?Sized>(&mut self, message: &T) -> Result<(), Error> {
        self.buf.clear();
        self.format.encode_into(message, &mut self.buf)?;
        frame::write(&mut self.writer, &self.buf)?;
        Ok(())
    }

    /// The next message, or `None` once the peer has closed the connection.
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
                buf: Vec::new(),
            },
        )
    }
}

impl<R: Read> ChannelReader<R> {
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// The next message, or `None` once the peer has closed the connection. A
    /// [`Error::Decode`] leaves the reader at the next frame, so a caller may
    /// skip the bad message and carry on.
    ///
    /// A reader lives as long as its connection, so after a large frame, such
    /// as a first batch of Items, it frees the buffer rather than keep it.
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
    pub fn format(&self) -> WireFormat {
        self.format
    }

    /// Sends one message and flushes it.
    pub fn send<T: Serialize + ?Sized>(&mut self, message: &T) -> Result<(), Error> {
        self.buf.clear();
        self.format.encode_into(message, &mut self.buf)?;
        frame::write(&mut self.writer, &self.buf)?;
        Ok(())
    }
}
