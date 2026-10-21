//! The open windows of any compositor with wlr-foreign-toplevel-management
//! (sway, river, labwc, niri, Wayfire and others), followed on a thread of
//! its own, and the requests that focus, close, minimize, maximize or
//! fullscreen one.
//!
//! The thread waits for the compositor (see
//! `kotodama_extension::wayland::Display`), then blocks on the connection:
//! the compositor pushes every window opened, changed or closed, so nothing
//! is polled. On a compositor without the protocol, such as GNOME or KDE, it
//! ends there, and so does it on Hyprland, whose windows the Hyprland
//! Extension offers already: the same window from both would show twice.
//!
//! # Owns
//!
//! - The Wayland connection, binding the toplevel manager, and when to step aside.
//! - Each toplevel's handle, and the requests made on it.
//!
//! # Not here
//!
//! - The windows' Items: [`crate::windows`].

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use kotodama_extension::STACK_SIZE;
use kotodama_extension::log;
use kotodama_extension::wayland::Display;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::EventQueue;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::backend::ObjectId;
use wayland_client::delegate_noop;
use wayland_client::event_created_child;
use wayland_client::globals::GlobalList;
use wayland_client::globals::GlobalListContents;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1;
use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;

use crate::windows::Request;
use crate::windows::Window;

/// What every Hyprland-only global's interface starts with.
const HYPRLAND: &str = "hyprland_";
/// The `state` values of a toplevel, from the protocol.
const MAXIMIZED: u32 = 0;
const MINIMIZED: u32 = 1;
const FULLSCREEN: u32 = 3;
/// The first version of the protocol with fullscreen.
const FULLSCREEN_SINCE: u32 = 2;

/// What the thread reports.
#[derive(Debug)]
pub enum Change {
    /// A compositor with the protocol is there; its windows follow.
    Up(Compositor),
    /// Window `number` is new, or the compositor described it again.
    Window {
        number: u64,
        window: Window,
    },
    Closed {
        number: u64,
    },
    /// The connection is gone: the compositor exited.
    Lost,
}

/// Each open toplevel's handle, by number.
type Handles = Arc<Mutex<HashMap<u64, ZwlrForeignToplevelHandleV1>>>;

/// The compositor, to make requests of.
#[derive(Debug)]
pub struct Compositor {
    connection: Connection,
    /// The seat focusing a window activates it on; `None` without one.
    seat: Option<WlSeat>,
    handles: Handles,
}

impl Compositor {
    /// Asks the compositor to do `request` to window `number`.
    ///
    /// # Errors
    ///
    /// A message for the user when the window is gone, the compositor can't
    /// do it, or the connection is lost.
    ///
    /// # Panics
    ///
    /// When the handles' mutex is poisoned: the toplevels thread panicked.
    pub fn request(&self, number: u64, request: Request) -> Result<(), String> {
        let handle = self
            .handles
            .lock()
            .expect("toplevel handles mutex poisoned")
            .get(&number)
            .cloned()
            .ok_or("That window is closed")?;
        match request {
            Request::Activate => {
                let seat = self
                    .seat
                    .as_ref()
                    .ok_or("The compositor has no seat to focus the window on")?;
                handle.activate(seat);
            }
            Request::Close => handle.close(),
            Request::Minimize => handle.set_minimized(),
            Request::SetMaximized(true) => handle.set_maximized(),
            Request::SetMaximized(false) => handle.unset_maximized(),
            Request::SetFullscreen(_) if handle.version() < FULLSCREEN_SINCE => {
                return Err("The compositor can't make windows fullscreen".into());
            }
            Request::SetFullscreen(true) => handle.set_fullscreen(None),
            Request::SetFullscreen(false) => handle.unset_fullscreen(),
        }
        self.connection
            .flush()
            .map_err(|err| format!("Lost the compositor: {err}"))
    }
}

/// Starts the thread that waits for the compositor and reports its windows
/// to `report`, until it returns false. Once the compositor exits, it waits
/// for the next one.
///
/// # Errors
///
/// When the thread can't be started.
pub fn follow(report: impl Fn(Change) -> bool + Send + Sync + 'static) -> io::Result<()> {
    thread::Builder::new()
        .name("toplevels".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let report: Report = Arc::new(report);
            // Counted on across compositors, so a window after a restart never
            // takes an earlier one's Item id.
            let mut next = 0;
            loop {
                let bound = Display::from_env().wait(bind, |err| log!("waiting for the compositor: {err}"));
                let (mut queue, compositor) = match bound {
                    Bound::Ready(queue, compositor) => (queue, compositor),
                    Bound::Hyprland => {
                        log!("leaving the windows to the Hyprland Extension");
                        return;
                    }
                    Bound::Absent => {
                        log!("no windows: the compositor has no wlr-foreign-toplevel-management");
                        return;
                    }
                };
                let mut state = State {
                    report: Arc::clone(&report),
                    handles: Arc::clone(&compositor.handles),
                    pending: HashMap::new(),
                    next,
                    live: true,
                };
                if !report(Change::Up(compositor)) {
                    return;
                }
                while state.live {
                    if let Err(err) = queue.blocking_dispatch(&mut state) {
                        log!("lost the compositor: {err}");
                        break;
                    }
                }
                next = state.next;
                if !state.live || !report(Change::Lost) {
                    return;
                }
            }
        })?;
    Ok(())
}

/// What binding on one connection found.
enum Bound {
    Ready(EventQueue<State>, Compositor),
    /// Hyprland, whose own Extension offers its windows.
    Hyprland,
    /// A compositor without the protocol.
    Absent,
}

/// Binds the toplevel manager on `connection`. The error is worth another
/// try; [`Bound::Hyprland`] and [`Bound::Absent`] are not.
fn bind(connection: Connection) -> Result<Bound, String> {
    let (globals, queue) = registry_queue_init::<State>(&connection).map_err(|err| err.to_string())?;
    if is_hyprland(&globals) {
        return Ok(Bound::Hyprland);
    }
    let qh = queue.handle();
    if globals
        .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
        .is_err()
    {
        return Ok(Bound::Absent);
    }
    let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=1, ()).ok();
    let compositor = Compositor {
        connection,
        seat,
        handles: Handles::default(),
    };
    Ok(Bound::Ready(queue, compositor))
}

fn is_hyprland(globals: &GlobalList) -> bool {
    globals
        .contents()
        .with_list(|list| list.iter().any(|global| global.interface.starts_with(HYPRLAND)))
}

type Report = Arc<dyn Fn(Change) -> bool + Send + Sync>;

/// What the event queue dispatches to.
struct State {
    report: Report,
    handles: Handles,
    /// Each open toplevel's number and what the compositor has said of it,
    /// applied at its `done`.
    pending: HashMap<ObjectId, (u64, Window)>,
    /// The number of the last toplevel.
    next: u64,
    /// False once `report` returned false: the Extension is exiting.
    live: bool,
}

impl State {
    fn report(&mut self, change: Change) {
        if !(self.report)(change) {
            self.live = false;
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.next += 1;
                state.pending.insert(toplevel.id(), (state.next, Window::default()));
                state
                    .handles
                    .lock()
                    .expect("toplevel handles mutex poisoned")
                    .insert(state.next, toplevel);
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                log!("the compositor stopped reporting windows");
            }
            _ => {}
        }
    }

    event_created_child!(State, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = handle.id();
        let Some((number, window)) = state.pending.get_mut(&id) else {
            return;
        };
        let number = *number;
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => window.title = title,
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => window.app_id = app_id,
            zwlr_foreign_toplevel_handle_v1::Event::State { state: states } => {
                let states: Vec<u32> = states
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|&bytes| u32::from_ne_bytes(bytes))
                    .collect();
                window.maximized = states.contains(&MAXIMIZED);
                window.minimized = states.contains(&MINIMIZED);
                window.fullscreen = states.contains(&FULLSCREEN);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                let window = window.clone();
                state.report(Change::Window { number, window });
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.pending.remove(&id);
                state
                    .handles
                    .lock()
                    .expect("toplevel handles mutex poisoned")
                    .remove(&number);
                handle.destroy();
                state.report(Change::Closed { number });
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlSeat);
