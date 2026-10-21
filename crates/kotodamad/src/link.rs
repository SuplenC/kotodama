//! A connection to an Extension or a Front end once its handshake is done:
//! one thread writes what the Daemon sends, leaving out what the peer's
//! protocol version can't read, and the calling thread reads.

use std::io::Read;
use std::io::Write;
use std::sync::mpsc;
use std::sync::mpsc::SyncSender;

use kotodama_protocol::Channel;
use kotodama_protocol::Error;
use kotodama_protocol::Since;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::host::spawn;

/// How many messages may wait for a peer to read them. A peer that leaves this
/// many unread has stopped reading, and the Daemon cuts it off.
pub(crate) const OUTBOX: usize = 1024;

/// Starts the writing thread and hands `up` the bounded sender that feeds it,
/// then passes each message read to `received` until the peer hangs up. A
/// message that doesn't decode is logged and skipped: its frame was read
/// whole, so the next one is intact. With `rejected`, the peer is also sent
/// the message it makes of the reason, so its author reads it too.
pub(crate) fn pump<R, W, In, Out>(
    name: &str,
    channel: Channel<R, W>,
    up: impl FnOnce(SyncSender<Out>),
    mut received: impl FnMut(In),
    rejected: Option<fn(String) -> Out>,
) -> Result<(), Error>
where
    R: Read,
    W: Write + Send + 'static,
    In: DeserializeOwned,
    Out: Serialize + Since + Send + 'static,
{
    let (mut reader, mut writer) = channel.split();
    let (sender, outbox) = mpsc::sync_channel::<Out>(OUTBOX);
    spawn(format!("{name}-w"), move || {
        // Ends when the Daemon drops the sender or the peer stops taking bytes.
        for message in outbox {
            // A peer on an older minor version isn't sent what its version
            // lacks; one on the Daemon's own version reads everything.
            if !writer.reads(&message) {
                continue;
            }
            if writer.send(&message).is_err() {
                break;
            }
        }
    });
    let reply = rejected.map(|rejected| (rejected, sender.clone()));
    up(sender);
    loop {
        match reader.recv::<In>() {
            Ok(Some(message)) => received(message),
            Ok(None) => return Ok(()),
            Err(err @ Error::Decode { .. }) => {
                let reason = format!(
                    "{err}: {}",
                    std::error::Error::source(&err).map_or(String::new(), ToString::to_string)
                );
                log::warn!("{name}: rejected a message: {reason}");
                // A peer too slow to read its outbox misses the reason rather
                // than holding up what it sends.
                if let Some((rejected, sender)) = &reply {
                    let _ = sender.try_send(rejected(reason));
                }
            }
            Err(err) => return Err(err),
        }
    }
}
