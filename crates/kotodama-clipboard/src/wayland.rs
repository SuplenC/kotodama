//! Watching the Wayland clipboard, and setting it, through ext-data-control,
//! or wlr-data-control on a compositor that has only that.
//!
//! The connection lives on a thread of its own. What each new selection
//! offers is checked before anything is read: an offer a password manager
//! marked sensitive is never read at all (see [`choose`](crate::copied::choose)).
//! The rest are read one at a time on a second thread, which also decodes
//! images for their thumbnails, so a slow source or a large image never holds
//! up the clipboard or the Daemon.

use crate::copied::Copied;

/// What the watching thread reports.
pub enum Change {
    /// Something was copied, and read.
    Copied(Copied),
    /// The connection to the compositor is gone.
    Lost,
}

#[cfg(target_os = "linux")]
pub use linux::Clipboard;

/// Without Linux there is no Wayland clipboard to watch.
#[cfg(not(target_os = "linux"))]
pub struct Clipboard;

#[cfg(not(target_os = "linux"))]
impl Clipboard {
    pub fn watch(_changed: impl Fn(Change) + Send + Sync + 'static) -> Result<Clipboard, String> {
        Err("the clipboard needs Wayland, which only Linux has".into())
    }

    pub fn set(&self, _mimes: Vec<String>, _data: Vec<u8>) -> Result<(), String> {
        Err("the clipboard needs Wayland, which only Linux has".into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Duration;

    use rustix::event::{PollFd, PollFlags, Timespec};
    use rustix::pipe::PipeFlags;
    use wayland_client::backend::ObjectId;
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{
        Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop, event_created_child,
    };
    use wayland_protocols::ext::data_control::v1::client::{
        ext_data_control_device_v1::{self as ext_device, ExtDataControlDeviceV1},
        ext_data_control_manager_v1::ExtDataControlManagerV1,
        ext_data_control_offer_v1::{self as ext_offer, ExtDataControlOfferV1},
        ext_data_control_source_v1::{self as ext_source, ExtDataControlSourceV1},
    };
    use wayland_protocols_wlr::data_control::v1::client::{
        zwlr_data_control_device_v1::{self as wlr_device, ZwlrDataControlDeviceV1},
        zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
        zwlr_data_control_offer_v1::{self as wlr_offer, ZwlrDataControlOfferV1},
        zwlr_data_control_source_v1::{self as wlr_source, ZwlrDataControlSourceV1},
    };

    use super::Change;
    use crate::copied::{Choice, Copied, choose};

    /// The most bytes one copy may have; anything bigger isn't kept. Well
    /// under the protocol's 64 MiB frame, so it can always be pasted back.
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    /// How long a source may go without sending a byte before reading it is
    /// given up.
    const READ_TIMEOUT: Duration = Duration::from_secs(5);
    const STACK: usize = 256 * 1024;

    /// The watched clipboard. Dropping it stops the watching.
    pub struct Clipboard {
        requests: Sender<(Vec<String>, Vec<u8>)>,
        wake: OwnedFd,
    }

    impl Clipboard {
        /// Connects to the compositor in `WAYLAND_DISPLAY` and calls
        /// `changed` with each thing copied from now on, on a thread of its
        /// own.
        pub fn watch(
            changed: impl Fn(Change) + Send + Sync + 'static,
        ) -> Result<Clipboard, String> {
            let conn = Connection::connect_to_env().map_err(|err| err.to_string())?;
            let (globals, mut queue) =
                registry_queue_init::<State>(&conn).map_err(|err| err.to_string())?;
            let qh = queue.handle();
            let seat: wl_seat::WlSeat = globals
                .bind(&qh, 1..=1, ())
                .map_err(|err| format!("no seat: {err}"))?;
            let control = match globals.bind::<ExtDataControlManagerV1, _, _>(&qh, 1..=1, ()) {
                Ok(manager) => {
                    let device = manager.get_data_device(&seat, &qh, ());
                    Control::Ext(manager, device)
                }
                Err(_) => {
                    let manager = globals
                        .bind::<ZwlrDataControlManagerV1, _, _>(&qh, 1..=2, ())
                        .map_err(|_| {
                            "the compositor has neither ext-data-control nor wlr-data-control"
                                .to_owned()
                        })?;
                    let device = manager.get_data_device(&seat, &qh, ());
                    Control::Wlr(manager, device)
                }
            };

            let (reads, to_read) = mpsc::channel::<(String, OwnedFd)>();
            let changed = Arc::new(changed);
            let reporter = Arc::clone(&changed);
            thread::Builder::new()
                .name("read".into())
                .stack_size(STACK)
                .spawn(move || read_all(&to_read, &*reporter))
                .map_err(|err| err.to_string())?;

            let mut state = State {
                control,
                offers: HashMap::new(),
                selection: None,
                reads,
                finished: false,
            };
            queue.roundtrip(&mut state).map_err(|err| err.to_string())?;

            let (wake_rx, wake) =
                rustix::pipe::pipe_with(PipeFlags::CLOEXEC).map_err(|err| err.to_string())?;
            let (requests, inbox) = mpsc::channel();
            thread::Builder::new()
                .name("wayland".into())
                .stack_size(STACK)
                .spawn(move || {
                    run(queue, state, &inbox, &wake_rx);
                    changed(Change::Lost);
                })
                .map_err(|err| err.to_string())?;
            Ok(Clipboard { requests, wake })
        }

        /// Puts `data` on the clipboard, offered as each of `mimes`. The
        /// clipboard keeps it until something else is copied.
        pub fn set(&self, mimes: Vec<String>, data: Vec<u8>) -> Result<(), String> {
            if self.requests.send((mimes, data)).is_ok()
                && rustix::io::write(&self.wake, &[1]).is_ok()
            {
                Ok(())
            } else {
                Err("lost the connection to the compositor".into())
            }
        }
    }

    /// Dispatches Wayland events and sets the clipboard when asked, until
    /// the [`Clipboard`] is dropped or the connection is lost.
    fn run(
        mut queue: EventQueue<State>,
        mut state: State,
        inbox: &Receiver<(Vec<String>, Vec<u8>)>,
        wake: &OwnedFd,
    ) {
        let qh = queue.handle();
        loop {
            if queue.dispatch_pending(&mut state).is_err() || state.finished {
                return;
            }
            if queue.flush().is_err() {
                return;
            }
            let Some(guard) = queue.prepare_read() else {
                continue;
            };
            let (woken, readable) = {
                let mut fds = [
                    PollFd::new(wake, PollFlags::IN),
                    PollFd::from_borrowed_fd(guard.connection_fd(), PollFlags::IN),
                ];
                if rustix::event::poll(&mut fds, None).is_err() {
                    continue;
                }
                (!fds[0].revents().is_empty(), !fds[1].revents().is_empty())
            };
            if readable {
                if guard.read().is_err() {
                    kotodama_extension::log!("lost the Wayland connection");
                    return;
                }
            } else {
                drop(guard);
            }
            if woken {
                let mut buf = [0; 64];
                if matches!(rustix::io::read(wake, &mut buf), Ok(0) | Err(_)) {
                    return;
                }
                for (mimes, data) in inbox.try_iter() {
                    state.set_selection(mimes, data, &qh);
                }
            }
        }
    }

    /// Reads each offer handed over, one at a time, and reports what it
    /// becomes.
    fn read_all(to_read: &Receiver<(String, OwnedFd)>, changed: &dyn Fn(Change)) {
        for (mime, fd) in to_read {
            match read(fd).and_then(|data| Copied::read(&mime, data)) {
                Ok(copied) => changed(Change::Copied(copied)),
                Err(err) => kotodama_extension::log!("not kept: {err}"),
            }
        }
    }

    /// Everything the source writes until it closes its end, or an error
    /// once it is too much or the source stalls.
    fn read(fd: OwnedFd) -> Result<Vec<u8>, String> {
        let mut file = File::from(fd);
        let mut data = Vec::new();
        let mut buf = vec![0; 64 * 1024];
        let timeout = Timespec::try_from(READ_TIMEOUT).map_err(|err| err.to_string())?;
        loop {
            let mut fds = [PollFd::new(&file, PollFlags::IN)];
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err("the source stopped sending".into()),
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(err) => return Err(err.to_string()),
            }
            match file.read(&mut buf) {
                Ok(0) => return Ok(data),
                Ok(n) => {
                    if data.len() + n > MAX_BYTES {
                        return Err(format!("more than {} MiB", MAX_BYTES >> 20));
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    enum Control {
        Ext(ExtDataControlManagerV1, ExtDataControlDeviceV1),
        Wlr(ZwlrDataControlManagerV1, ZwlrDataControlDeviceV1),
    }

    enum Offer {
        Ext(ExtDataControlOfferV1),
        Wlr(ZwlrDataControlOfferV1),
    }

    impl Offer {
        fn id(&self) -> ObjectId {
            match self {
                Offer::Ext(offer) => offer.id(),
                Offer::Wlr(offer) => offer.id(),
            }
        }

        fn receive(&self, mime: String, fd: &OwnedFd) {
            match self {
                Offer::Ext(offer) => offer.receive(mime, fd.as_fd()),
                Offer::Wlr(offer) => offer.receive(mime, fd.as_fd()),
            }
        }

        fn destroy(&self) {
            match self {
                Offer::Ext(offer) => offer.destroy(),
                Offer::Wlr(offer) => offer.destroy(),
            }
        }
    }

    #[derive(PartialEq)]
    enum Source {
        Ext(ExtDataControlSourceV1),
        Wlr(ZwlrDataControlSourceV1),
    }

    impl Source {
        fn destroy(&self) {
            match self {
                Source::Ext(source) => source.destroy(),
                Source::Wlr(source) => source.destroy(),
            }
        }
    }

    struct State {
        control: Control,
        /// The MIME types of each offer not yet taken or dropped.
        offers: HashMap<ObjectId, (Offer, Vec<String>)>,
        /// What we put on the clipboard, while it is still there.
        selection: Option<(Source, Arc<Vec<u8>>)>,
        /// Hands an offer's read end to the reading thread.
        reads: Sender<(String, OwnedFd)>,
        /// The compositor took the data device away.
        finished: bool,
    }

    impl State {
        fn offered(&mut self, offer: Offer) {
            self.offers.insert(offer.id(), (offer, Vec::new()));
        }

        fn mime(&mut self, offer: ObjectId, mime: String) {
            if let Some((_, mimes)) = self.offers.get_mut(&offer) {
                mimes.push(mime);
            }
        }

        /// A new selection: reads it unless it is sensitive or holds nothing
        /// worth keeping.
        fn selected(&mut self, offer: Option<ObjectId>) {
            let Some((offer, mimes)) = offer.and_then(|id| self.offers.remove(&id)) else {
                return;
            };
            match choose(mimes.iter().map(String::as_str)) {
                Choice::Read(mime) => match rustix::pipe::pipe_with(PipeFlags::CLOEXEC) {
                    Ok((read, write)) => {
                        offer.receive(mime.clone(), &write);
                        // The request carries its own copy of the write end;
                        // ours must close, or the read never ends.
                        drop(write);
                        let _ = self.reads.send((mime, read));
                    }
                    Err(err) => kotodama_extension::log!("no pipe to read through: {err}"),
                },
                Choice::Sensitive | Choice::Nothing => {}
            }
            offer.destroy();
        }

        /// Drops an offer that won't be read, such as the primary selection.
        fn dropped(&mut self, offer: Option<ObjectId>) {
            if let Some((offer, _)) = offer.and_then(|id| self.offers.remove(&id)) {
                offer.destroy();
            }
        }

        fn set_selection(&mut self, mimes: Vec<String>, data: Vec<u8>, qh: &QueueHandle<State>) {
            let source = match &self.control {
                Control::Ext(manager, device) => {
                    let source = manager.create_data_source(qh, ());
                    for mime in mimes {
                        source.offer(mime);
                    }
                    device.set_selection(Some(&source));
                    Source::Ext(source)
                }
                Control::Wlr(manager, device) => {
                    let source = manager.create_data_source(qh, ());
                    for mime in mimes {
                        source.offer(mime);
                    }
                    device.set_selection(Some(&source));
                    Source::Wlr(source)
                }
            };
            if let Some((old, _)) = self.selection.replace((source, Arc::new(data))) {
                old.destroy();
            }
        }

        /// A paste asks for what we put on the clipboard.
        fn send(&self, source: &Source, fd: OwnedFd) {
            if let Some((_, data)) = self.selection.as_ref().filter(|(ours, _)| ours == source) {
                let data = Arc::clone(data);
                // A slow reader mustn't hold up the clipboard.
                let _ = thread::Builder::new()
                    .name("paste".into())
                    .stack_size(STACK)
                    .spawn(move || {
                        let _ = File::from(fd).write_all(&data);
                    });
            }
        }

        fn cancelled(&mut self, source: Source) {
            if self
                .selection
                .as_ref()
                .is_some_and(|(ours, _)| *ours == source)
            {
                self.selection = None;
            }
            source.destroy();
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    delegate_noop!(State: ignore wl_seat::WlSeat);

    /// The Dispatch impls for one data-control protocol; ext and wlr are the
    /// same protocol by two names.
    macro_rules! data_control {
        ($variant:ident, $manager:ty, $device:ty, $device_mod:ident, $offer:ty, $offer_mod:ident, $source:ty, $source_mod:ident) => {
            delegate_noop!(State: $manager);

            impl Dispatch<$device, ()> for State {
                fn event(
                    state: &mut Self,
                    _: &$device,
                    event: $device_mod::Event,
                    _: &(),
                    _: &Connection,
                    _: &QueueHandle<Self>,
                ) {
                    match event {
                        $device_mod::Event::DataOffer { id } => state.offered(Offer::$variant(id)),
                        $device_mod::Event::Selection { id } => {
                            state.selected(id.map(|offer| offer.id()))
                        }
                        $device_mod::Event::PrimarySelection { id } => {
                            state.dropped(id.map(|offer| offer.id()))
                        }
                        $device_mod::Event::Finished => state.finished = true,
                        _ => {}
                    }
                }

                event_created_child!(State, $device, [
                    $device_mod::EVT_DATA_OFFER_OPCODE => ($offer, ()),
                ]);
            }

            impl Dispatch<$offer, ()> for State {
                fn event(
                    state: &mut Self,
                    offer: &$offer,
                    event: $offer_mod::Event,
                    _: &(),
                    _: &Connection,
                    _: &QueueHandle<Self>,
                ) {
                    if let $offer_mod::Event::Offer { mime_type } = event {
                        state.mime(offer.id(), mime_type);
                    }
                }
            }

            impl Dispatch<$source, ()> for State {
                fn event(
                    state: &mut Self,
                    source: &$source,
                    event: $source_mod::Event,
                    _: &(),
                    _: &Connection,
                    _: &QueueHandle<Self>,
                ) {
                    match event {
                        $source_mod::Event::Send { fd, .. } => {
                            state.send(&Source::$variant(source.clone()), fd)
                        }
                        $source_mod::Event::Cancelled => {
                            state.cancelled(Source::$variant(source.clone()))
                        }
                        _ => {}
                    }
                }
            }
        };
    }

    data_control!(
        Ext,
        ExtDataControlManagerV1,
        ExtDataControlDeviceV1,
        ext_device,
        ExtDataControlOfferV1,
        ext_offer,
        ExtDataControlSourceV1,
        ext_source
    );
    data_control!(
        Wlr,
        ZwlrDataControlManagerV1,
        ZwlrDataControlDeviceV1,
        wlr_device,
        ZwlrDataControlOfferV1,
        wlr_offer,
        ZwlrDataControlSourceV1,
        wlr_source
    );
}
