//! Control channel between `dropkickctl` and the resident daemon.
//!
//! The global-shortcuts portal is not available on this system (COSMIC's
//! xdg-desktop-portal does not export `org.freedesktop.portal.GlobalShortcuts`),
//! so the summon path is: compositor keybinding -> `dropkickctl` -> this socket.
//!
//! SECURITY: a resident daemon with a socket is attack surface Dropkick would
//! not otherwise have. Two mitigations, both deliberate:
//!   * the socket lives in $XDG_RUNTIME_DIR at mode 0600, so it is reachable
//!     only by this user;
//!   * the vocabulary is four verbs that move a window. There is no verb that
//!     reads clipboard history back out, and there must never be one. A local
//!     attacker who can talk to this socket can make a window appear. That is
//!     the whole of the granted authority.

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use smithay_client_toolkit::reexports::calloop::channel::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Show,
    Hide,
    Toggle,
    Quit,
}

impl Cmd {
    pub fn as_byte(self) -> u8 {
        match self {
            Cmd::Show => b's',
            Cmd::Hide => b'h',
            Cmd::Toggle => b't',
            Cmd::Quit => b'q',
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b's' => Some(Cmd::Show),
            b'h' => Some(Cmd::Hide),
            b't' => Some(Cmd::Toggle),
            b'q' => Some(Cmd::Quit),
            _ => None,
        }
    }
}

pub fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(dir).join("dropkick.sock")
}

/// Bind the control socket and pump commands into the event loop.
pub fn spawn_listener(tx: Sender<Cmd>) -> std::io::Result<PathBuf> {
    let path = socket_path();
    // A stale socket from a crashed daemon would block bind(). Removing it is
    // safe only because a live daemon holds the same path: if one is running,
    // connect() below would have succeeded and we would not be here.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).is_err() {
                continue;
            }
            if let Some(cmd) = Cmd::from_byte(byte[0]) {
                // Ack before dispatching so ctl's exit is not gated on the
                // compositor round trip. ctl's latency is the keybinding's
                // latency; the window work happens after it has gone.
                let _ = stream.write_all(b"k");
                if tx.send(cmd).is_err() {
                    break;
                }
            }
        }
    });

    Ok(path)
}

/// Client side: send one command, wait for the ack, return.
pub fn send(cmd: Cmd) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.write_all(&[cmd.as_byte()])?;
    let mut ack = [0u8; 1];
    stream.read_exact(&mut ack)?;
    Ok(())
}
