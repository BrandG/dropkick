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
use std::path::{Path, PathBuf};

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
/// Claim the control socket, refusing to steal it from a living daemon.
///
/// Split out from [`spawn_listener`] so the three cases can be tested: free
/// path, live owner, and the corpse a crashed daemon leaves behind.
pub fn bind_control_socket(path: &Path) -> std::io::Result<UnixListener> {
    // Never unlink the path blind. A running daemon owns it, and stealing it
    // leaves that daemon listening on an orphaned inode: alive, invisible, and
    // unreachable by `dropkickctl quit` forever. Ask first.
    let listener = match UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            match UnixStream::connect(path) {
                // Something answered: a live daemon owns this. Stand down.
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        "another dropkickd is already running",
                    ))
                }
                // Nothing listening, so the file is a corpse from a crash and
                // removing it is safe.
                Err(_) => {
                    std::fs::remove_file(path)?;
                    UnixListener::bind(path)?
                }
            }
        }
        Err(e) => return Err(e),
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub fn spawn_listener(tx: Sender<Cmd>) -> std::io::Result<PathBuf> {
    let path = socket_path();
    let listener = bind_control_socket(&path)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(dir).join(format!("dropkick-test-{tag}-{}.sock", std::process::id()))
    }

    #[test]
    fn binds_a_free_path() {
        let path = temp_path("free");
        let _ = std::fs::remove_file(&path);
        assert!(bind_control_socket(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refuses_to_steal_from_a_live_owner() {
        // The bug this pins: unlinking here leaves the live daemon listening on
        // an orphaned inode, alive but unreachable by `dropkickctl quit`.
        let path = temp_path("live");
        let _ = std::fs::remove_file(&path);
        let _owner = bind_control_socket(&path).expect("first bind");

        let err = bind_control_socket(&path).expect_err("second bind must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(path.exists(), "the live owner's socket must survive");

        // ...and the owner is still reachable.
        assert!(UnixStream::connect(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reclaims_the_socket_a_crash_left_behind() {
        let path = temp_path("stale");
        let _ = std::fs::remove_file(&path);
        // Dropping a listener does not unlink the path, which is exactly what a
        // crashed daemon leaves.
        drop(bind_control_socket(&path).expect("first bind"));
        assert!(path.exists());

        assert!(bind_control_socket(&path).is_ok(), "a corpse must not block startup");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn socket_is_owner_only() {
        let path = temp_path("perms");
        let _ = std::fs::remove_file(&path);
        let _l = bind_control_socket(&path).expect("bind");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the control socket must not be reachable by other users");
        let _ = std::fs::remove_file(&path);
    }
}
