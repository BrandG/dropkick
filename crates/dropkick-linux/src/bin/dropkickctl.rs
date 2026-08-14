//! dropkickctl — the thing a compositor keybinding actually runs.
//!
//! It exists because this system's xdg-desktop-portal does not export
//! GlobalShortcuts. Binding Super+Space in COSMIC Settings to this binary costs
//! roughly 1.1ms end to end (measured), which buys portability across every
//! desktop and needs no privileged input access.
//!
//! Bind it to the binary directly, not through `sh -c`: a shell would cost more
//! than everything this program does.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use dropkick_linux::ipc::{self, Cmd};

/// Launch `dropkickd` from alongside this binary. Same directory rather than
/// PATH: whichever dropkickctl you ran is the one whose daemon you get.
fn start_daemon() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let daemon = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("dropkickctl has no parent directory"))?
        .join("dropkickd");
    if !daemon.exists() {
        return Err(std::io::Error::other(format!("{} not found", daemon.display())));
    }
    Command::new(&daemon)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// Poll briefly for the socket to come up. A fresh daemon has to open a Wayland
/// connection and create its surface before it can answer.
fn retry_send(cmd: Cmd) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut last = std::io::Error::other("no attempt made");
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
        match ipc::send(cmd) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let verb = args.get(1).map(String::as_str).unwrap_or("toggle");
    let timed = args.iter().any(|a| a == "--time");

    let cmd = match verb {
        "show" => Cmd::Show,
        "hide" => Cmd::Hide,
        "toggle" => Cmd::Toggle,
        "quit" => Cmd::Quit,
        other => {
            eprintln!("dropkickctl: unknown verb {other:?} (show|hide|toggle|quit)");
            std::process::exit(2);
        }
    };

    let start = Instant::now();
    if let Err(first) = ipc::send(cmd) {
        // Launched from a keybinding there is no terminal to print to, so a
        // dead daemon would simply look like a broken key. Start one and retry
        // instead: the first summon of the session pays for it, and every
        // summon after that is the ~1ms path.
        if let Err(e) = start_daemon() {
            eprintln!("dropkickctl: {first}; could not start daemon: {e}");
            std::process::exit(1);
        }
        if let Err(e) = retry_send(cmd) {
            eprintln!("dropkickctl: daemon started but did not answer: {e}");
            std::process::exit(1);
        }
    }
    if timed {
        println!("{:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
    }
}
