//! The clipboard through data-control: ext-data-control, the standard one,
//! or wlr-data-control on a compositor that has only that. The two are the
//! same protocol by two names.
//!
//! [`DataControl`] holds the bound manager and device. What each object
//! needs lives in its own user data: an offer's MIME types in
//! [`OfferMimes`], what a source serves in [`SourceData`]. So the state an
//! event queue dispatches to only implements [`DataControlHandler`] and
//! names itself in [`delegate_data_control!`](crate::delegate_data_control).

use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Mutex};
use std::thread;

use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self as ext_device, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::{self as ext_manager, ExtDataControlManagerV1},
    ext_data_control_offer_v1::{self as ext_offer, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self as ext_source, ExtDataControlSourceV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self as wlr_device, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::{self as wlr_manager, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self as wlr_offer, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self as wlr_source, ZwlrDataControlSourceV1},
};

/// A thread that writes the clipboard to a paste needs little stack.
const STACK: usize = 64 * 1024;

/// What an event queue's state does with the clipboard's events.
pub trait DataControlHandler: Sized + 'static {
    /// The clipboard now holds `offer`, offered as `mimes`. The offer is
    /// destroyed once this returns; read it with [`Offer::receive`] before.
    fn selected(&mut self, _offer: &Offer, _mimes: Vec<String>) {}

    /// The compositor took the data device away: the clipboard can no longer
    /// be watched or set on this connection.
    fn finished(&mut self) {}
}

/// The bound data-control manager and the seat's data device.
pub enum DataControl {
    Ext(ExtDataControlManagerV1, ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlManagerV1, ZwlrDataControlDeviceV1),
}

impl DataControl {
    /// Binds ext-data-control, or wlr-data-control if the compositor has
    /// only that, for `seat`.
    pub fn bind<S>(globals: &GlobalList, seat: &WlSeat, qh: &QueueHandle<S>) -> Result<Self, String>
    where
        S: DataControlHandler
            + Dispatch<ExtDataControlManagerV1, ()>
            + Dispatch<ExtDataControlDeviceV1, ()>
            + Dispatch<ZwlrDataControlManagerV1, ()>
            + Dispatch<ZwlrDataControlDeviceV1, ()>,
    {
        if let Ok(manager) = globals.bind::<ExtDataControlManagerV1, _, _>(qh, 1..=1, ()) {
            let device = manager.get_data_device(seat, qh, ());
            return Ok(DataControl::Ext(manager, device));
        }
        let manager = globals
            .bind::<ZwlrDataControlManagerV1, _, _>(qh, 1..=2, ())
            .map_err(|_| "the compositor has neither ext-data-control nor wlr-data-control")?;
        let device = manager.get_data_device(seat, qh, ());
        Ok(DataControl::Wlr(manager, device))
    }

    /// The protocol bound, for logs.
    pub fn protocol(&self) -> &'static str {
        match self {
            DataControl::Ext(..) => "ext-data-control",
            DataControl::Wlr(..) => "wlr-data-control",
        }
    }

    /// Puts `data` on the clipboard, offered as each of `mimes`, and serves
    /// it to every paste until something else is copied. The compositor has
    /// it once the next round trip is done.
    pub fn set_selection<S>(&self, mimes: Vec<String>, data: Vec<u8>, qh: &QueueHandle<S>)
    where
        S: Dispatch<ExtDataControlSourceV1, SourceData>
            + Dispatch<ZwlrDataControlSourceV1, SourceData>
            + 'static,
    {
        let data = SourceData(Arc::new(data));
        match self {
            DataControl::Ext(manager, device) => {
                let source = manager.create_data_source(qh, data);
                for mime in mimes {
                    source.offer(mime);
                }
                device.set_selection(Some(&source));
            }
            DataControl::Wlr(manager, device) => {
                let source = manager.create_data_source(qh, data);
                for mime in mimes {
                    source.offer(mime);
                }
                device.set_selection(Some(&source));
            }
        }
    }
}

/// Something on the clipboard, offered by another client.
pub enum Offer {
    Ext(ExtDataControlOfferV1),
    Wlr(ZwlrDataControlOfferV1),
}

impl Offer {
    /// Asks the source to write the clipboard as `mime` into `fd`.
    pub fn receive(&self, mime: String, fd: &OwnedFd) {
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

/// The MIME types an offer has announced so far. Public only for
/// [`delegate_data_control!`](crate::delegate_data_control).
#[doc(hidden)]
#[derive(Default)]
pub struct OfferMimes(Mutex<Vec<String>>);

impl OfferMimes {
    fn add(&self, mime: String) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(mime);
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|poison| poison.into_inner()))
    }
}

/// What a source we made serves. Public only for
/// [`delegate_data_control!`](crate::delegate_data_control).
#[doc(hidden)]
pub struct SourceData(Arc<Vec<u8>>);

impl SourceData {
    /// Writes it to a paste, on a thread of its own: a slow reader mustn't
    /// hold up the event queue.
    fn send(&self, fd: OwnedFd) {
        let data = Arc::clone(&self.0);
        if let Err(err) = thread::Builder::new()
            .name("paste".into())
            .stack_size(STACK)
            .spawn(move || {
                let _ = File::from(fd).write_all(&data);
            })
        {
            crate::log!("couldn't start a thread to paste from: {err}");
        }
    }
}

/// The Dispatch impls for one protocol, on [`DataControl`] for any handler.
macro_rules! dispatch {
    ($variant:ident, $manager:ty, $manager_mod:ident, $device:ty, $device_mod:ident, $offer:ty, $offer_mod:ident, $source:ty, $source_mod:ident) => {
        impl<S: DataControlHandler + Dispatch<$manager, ()>> Dispatch<$manager, (), S> for DataControl {
            fn event(
                _: &mut S,
                _: &$manager,
                _: $manager_mod::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<S>,
            ) {
            }
        }

        impl<S> Dispatch<$device, (), S> for DataControl
        where
            S: DataControlHandler + Dispatch<$device, ()> + Dispatch<$offer, OfferMimes>,
        {
            fn event(
                state: &mut S,
                _: &$device,
                event: $device_mod::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<S>,
            ) {
                match event {
                    $device_mod::Event::Selection { id: Some(offer) } => {
                        let mimes = offer.data::<OfferMimes>().map_or_else(Vec::new, OfferMimes::take);
                        let offer = Offer::$variant(offer);
                        state.selected(&offer, mimes);
                        offer.destroy();
                    }
                    // The primary selection is never read.
                    $device_mod::Event::PrimarySelection { id: Some(offer) } => offer.destroy(),
                    $device_mod::Event::Finished => state.finished(),
                    _ => {}
                }
            }

            wayland_client::event_created_child!(S, $device, [
                $device_mod::EVT_DATA_OFFER_OPCODE => ($offer, OfferMimes::default()),
            ]);
        }

        impl<S: DataControlHandler + Dispatch<$offer, OfferMimes>> Dispatch<$offer, OfferMimes, S> for DataControl {
            fn event(
                _: &mut S,
                _: &$offer,
                event: $offer_mod::Event,
                mimes: &OfferMimes,
                _: &Connection,
                _: &QueueHandle<S>,
            ) {
                if let $offer_mod::Event::Offer { mime_type } = event {
                    mimes.add(mime_type);
                }
            }
        }

        impl<S: DataControlHandler + Dispatch<$source, SourceData>> Dispatch<$source, SourceData, S> for DataControl {
            fn event(
                _: &mut S,
                source: &$source,
                event: $source_mod::Event,
                data: &SourceData,
                _: &Connection,
                _: &QueueHandle<S>,
            ) {
                match event {
                    $source_mod::Event::Send { fd, .. } => data.send(fd),
                    // Something else was copied.
                    $source_mod::Event::Cancelled => source.destroy(),
                    _ => {}
                }
            }
        }
    };
}

dispatch!(
    Ext,
    ExtDataControlManagerV1,
    ext_manager,
    ExtDataControlDeviceV1,
    ext_device,
    ExtDataControlOfferV1,
    ext_offer,
    ExtDataControlSourceV1,
    ext_source
);
dispatch!(
    Wlr,
    ZwlrDataControlManagerV1,
    wlr_manager,
    ZwlrDataControlDeviceV1,
    wlr_device,
    ZwlrDataControlOfferV1,
    wlr_offer,
    ZwlrDataControlSourceV1,
    wlr_source
);

/// Makes a state that implements [`DataControlHandler`] dispatch every
/// data-control event to [`DataControl`].
#[macro_export]
macro_rules! delegate_data_control {
    ($state:ty) => {
        $crate::delegate_data_control!(@ $state,
            $crate::wayland::reexports::wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1,
            $crate::wayland::reexports::wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::ExtDataControlDeviceV1,
            $crate::wayland::reexports::wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::ExtDataControlOfferV1,
            $crate::wayland::reexports::wayland_protocols::ext::data_control::v1::client::ext_data_control_source_v1::ExtDataControlSourceV1);
        $crate::delegate_data_control!(@ $state,
            $crate::wayland::reexports::wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
            $crate::wayland::reexports::wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::ZwlrDataControlDeviceV1,
            $crate::wayland::reexports::wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
            $crate::wayland::reexports::wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_source_v1::ZwlrDataControlSourceV1);
    };
    (@ $state:ty, $manager:ty, $device:ty, $offer:ty, $source:ty) => {
        $crate::wayland::reexports::wayland_client::delegate_dispatch!($state:
            [$manager: ()] => $crate::wayland::DataControl);
        $crate::wayland::reexports::wayland_client::delegate_dispatch!($state:
            [$device: ()] => $crate::wayland::DataControl);
        $crate::wayland::reexports::wayland_client::delegate_dispatch!($state:
            [$offer: $crate::wayland::OfferMimes] => $crate::wayland::DataControl);
        $crate::wayland::reexports::wayland_client::delegate_dispatch!($state:
            [$source: $crate::wayland::SourceData] => $crate::wayland::DataControl);
    };
}
