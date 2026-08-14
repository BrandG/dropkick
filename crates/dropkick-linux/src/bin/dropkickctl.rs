//! dropkickctl — the thing a compositor keybinding actually runs.
//!
//! It exists because this system's xdg-desktop-portal does not export
//! GlobalShortcuts. Binding Super+Space in COSMIC Settings to this binary costs
//! roughly 1.1ms end to end (measured), which buys portability across every
//! desktop and needs no privileged input access.
//!
//! Bind it to the binary directly, not through `sh -c`: a shell would cost more
//! than everything this program does.

use std::time::Instant;

use dropkick_linux::ipc::{self, Cmd};

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
    if let Err(e) = ipc::send(cmd) {
        eprintln!(
            "dropkickctl: no daemon at {}: {e}",
            ipc::socket_path().display()
        );
        std::process::exit(1);
    }
    if timed {
        println!("{:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
    }
}
