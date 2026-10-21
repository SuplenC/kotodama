// A virtual pointer that stays until it is killed, so headless sway's seat has
// a pointer and `swaymsg seat - cursor …` moves it over windows. It speaks the
// Wayland wire format itself, so it needs nothing beyond std.
// zwlr_virtual_pointer_manager_v1 (wlr-protocols):
// https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-virtual-pointer-unstable-v1.xml
use std::env;
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process;

const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;
const MANAGER: u32 = 3;
const POINTER: u32 = 4;

const DISPLAY_GET_REGISTRY: u16 = 1;
const DISPLAY_ERROR: u16 = 0;
const REGISTRY_GLOBAL: u16 = 0;
const REGISTRY_BIND: u16 = 0;
const MANAGER_CREATE_VIRTUAL_POINTER: u16 = 0;

const MANAGER_INTERFACE: &str = "zwlr_virtual_pointer_manager_v1";

fn main() {
    let dir = env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR is set");
    let display = env::var_os("WAYLAND_DISPLAY").expect("WAYLAND_DISPLAY is set");
    let mut socket = UnixStream::connect(PathBuf::from(dir).join(display)).expect("connect to the compositor");

    send(&mut socket, DISPLAY, DISPLAY_GET_REGISTRY, &Args::new().uint(REGISTRY));
    loop {
        let mut header = [0; 8];
        if socket.read_exact(&mut header).is_err() {
            return;
        }
        let object = u32::from_ne_bytes(header[0..4].try_into().unwrap());
        let word = u32::from_ne_bytes(header[4..8].try_into().unwrap());
        let (size, opcode) = ((word >> 16) as usize, word as u16);
        let mut body = vec![0; size - 8];
        socket.read_exact(&mut body).expect("read an event");

        if object == DISPLAY && opcode == DISPLAY_ERROR {
            eprintln!("the compositor sent an error: {:?}", String::from_utf8_lossy(&body[12..]));
            process::exit(1);
        }
        if object == REGISTRY && opcode == REGISTRY_GLOBAL {
            let name = u32::from_ne_bytes(body[0..4].try_into().unwrap());
            let length = u32::from_ne_bytes(body[4..8].try_into().unwrap()) as usize;
            if &body[8..8 + length - 1] == MANAGER_INTERFACE.as_bytes() {
                let bind = Args::new().uint(name).string(MANAGER_INTERFACE).uint(1).uint(MANAGER);
                send(&mut socket, REGISTRY, REGISTRY_BIND, &bind);
                // No seat: the compositor gives the pointer to its default one.
                let create = Args::new().uint(0).uint(POINTER);
                send(&mut socket, MANAGER, MANAGER_CREATE_VIRTUAL_POINTER, &create);
                println!("pointer ready");
            }
        }
    }
}

fn send(socket: &mut UnixStream, object: u32, opcode: u16, args: &Args) {
    let size = 8 + args.0.len() as u32;
    let mut message = Vec::with_capacity(size as usize);
    message.extend_from_slice(&object.to_ne_bytes());
    message.extend_from_slice(&(size << 16 | u32::from(opcode)).to_ne_bytes());
    message.extend_from_slice(&args.0);
    socket.write_all(&message).expect("send a request");
}

/// A request's arguments in the wire format.
struct Args(Vec<u8>);

impl Args {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn uint(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_ne_bytes());
        self
    }

    /// The length with its NUL, the bytes and the NUL, padded to 32 bits.
    fn string(mut self, value: &str) -> Self {
        self = self.uint(value.len() as u32 + 1);
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
        while self.0.len() % 4 != 0 {
            self.0.push(0);
        }
        self
    }
}
