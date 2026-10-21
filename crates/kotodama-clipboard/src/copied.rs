//! What to read from a clipboard offer, and what the read bytes become.
//!
//! # Owns
//!
//! - Which MIME type of an offer to read, and leaving a sensitive offer unread.
//! - Turning the read bytes into text, or an image with its thumbnail.
//!
//! # Not here
//!
//! - Reading the offer off the compositor: `wayland.rs`.
//! - Keeping what was read: `history.rs`.

use std::fmt;
use std::io::Cursor;

use image::ImageError;
use image::ImageFormat;
use image::ImageReader;
use image::Limits;

/// The MIME type password managers add to what they copy, so clipboard
/// histories leave it out. Its presence is enough: nothing in that offer is
/// read.
pub const SENSITIVE: &str = "x-kde-passwordManagerHint";

/// Text types, best first: UTF-8 ones before the X11 names that may not be.
pub const TEXT: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
    "TEXT",
];

/// The image types a thumbnail can be made of.
const IMAGES: [&str; 2] = ["image/png", "image/jpeg"];

/// The longest side of a thumbnail, in pixels.
pub const THUMBNAIL: u32 = 256;

/// Decoding an image allocates at most this much, so a huge or hostile
/// image can't take the Extension's memory.
const DECODE_LIMIT: u64 = 256 * 1024 * 1024;

/// What to do with a clipboard offer, given the MIME types it offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A password manager marked it: read nothing, keep nothing.
    Sensitive,
    /// Read it as this type.
    Read(String),
    /// Nothing worth keeping, such as only rich text or a file manager's
    /// private types.
    Nothing,
}

/// Picks what to read from an offer: never anything from a sensitive one,
/// text if there is any, then an image.
pub fn choose<'a>(mimes: impl IntoIterator<Item = &'a str>) -> Choice {
    let mut text: Option<usize> = None;
    let mut image: Option<&str> = None;
    for mime in mimes {
        if mime == SENSITIVE {
            return Choice::Sensitive;
        }
        if let Some(rank) = TEXT.iter().position(|known| *known == mime) {
            text = Some(text.map_or(rank, |best| best.min(rank)));
        } else if image.is_none() && IMAGES.contains(&mime) {
            image = Some(mime);
        }
    }
    match (text, image) {
        (Some(rank), _) => Choice::Read(TEXT[rank].to_owned()),
        (None, Some(mime)) => Choice::Read(mime.to_owned()),
        (None, None) => Choice::Nothing,
    }
}

/// What was read off the clipboard, ready to keep. Making one does the slow
/// part, decoding an image for its thumbnail, so it can happen off the thread
/// that answers the Daemon. Its `Debug` leaves out what was copied, which
/// may be a secret no password manager marked.
pub enum Copied {
    /// Text, never blank.
    Text(String),
    /// A PNG or JPEG image, `width` by `height` pixels.
    Image {
        mime: String,
        /// The image as copied, in `mime`.
        data: Vec<u8>,
        width: u32,
        height: u32,
        /// The thumbnail, as PNG.
        thumbnail: Vec<u8>,
    },
}

impl Copied {
    /// The bytes read as `mime`, which [`choose`] picked.
    ///
    /// # Errors
    ///
    /// [`ReadError`] for blank text, a type that is neither text nor a kept
    /// image, and an image that doesn't decode or thumbnail.
    pub fn read(mime: &str, data: Vec<u8>) -> Result<Copied, ReadError> {
        if TEXT.contains(&mime) {
            let text =
                String::from_utf8(data).unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned());
            if text.trim().is_empty() {
                return Err(ReadError::Blank);
            }
            return Ok(Copied::Text(text));
        }
        let format = ImageFormat::from_mime_type(mime).ok_or_else(|| ReadError::not_kept(mime))?;
        let mut reader = ImageReader::with_format(Cursor::new(&data), format);
        let mut limits = Limits::default();
        limits.max_alloc = Some(DECODE_LIMIT);
        reader.limits(limits);
        let image = reader.decode().map_err(|err| ReadError::decode(mime, err))?;
        let (width, height) = (image.width(), image.height());
        let small = image.thumbnail(THUMBNAIL, THUMBNAIL);
        drop(image);
        let mut thumbnail = Cursor::new(Vec::new());
        small
            .write_to(&mut thumbnail, ImageFormat::Png)
            .map_err(ReadError::Thumbnail)?;
        Ok(Copied::Image {
            mime: mime.to_owned(),
            data,
            width,
            height,
            thumbnail: thumbnail.into_inner(),
        })
    }

    /// The bytes to keep and paste back.
    pub fn data(&self) -> &[u8] {
        match self {
            Copied::Text(text) => text.as_bytes(),
            Copied::Image { data, .. } => data,
        }
    }
}

impl fmt::Debug for Copied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Copied::Text(_) => f.debug_tuple("Text").field(&format_args!("<redacted>")).finish(),
            Copied::Image {
                mime, width, height, ..
            } => f
                .debug_struct("Image")
                .field("mime", mime)
                .field("width", width)
                .field("height", height)
                .finish_non_exhaustive(),
        }
    }
}

/// Why what was read off the clipboard isn't kept.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("blank text")]
    Blank,
    #[error("{0} isn't kept")]
    NotKept(String),
    #[error("the {mime} doesn't decode")]
    Decode {
        mime: String,
        #[source]
        source: ImageError,
    },
    #[error("couldn't make a thumbnail")]
    Thumbnail(#[source] ImageError),
}

impl ReadError {
    fn not_kept(mime: &str) -> Self {
        ReadError::NotKept(mime.to_owned())
    }

    fn decode(mime: &str, source: ImageError) -> Self {
        ReadError::Decode {
            mime: mime.to_owned(),
            source,
        }
    }
}
