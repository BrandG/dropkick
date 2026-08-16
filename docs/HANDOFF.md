# Handoff

For whoever picks this up next, including a future me with no memory of it.

**Read `docs/PROJECT_LOG.md` first.** It holds every decision, what was
rejected, and why. This file is orientation and traps; the log is the reasoning.

## What Dropkick is

A keyboard-summoned desktop utility panel with a hard philosophy: no networking
code at all, no accounts, no telemetry, no plugins, keyboard-complete, ten tools
maximum forever. `README.md` is the user-facing version.

One user (Brand), on Pop!_OS 24.04 / COSMIC / Wayland. Windows is a stated
future target; `dropkick-core` is kept free of anything OS-specific for it.

## State as of 2026-08-16

Four of seven tools built. 119 tests, clippy clean at zero warnings.

| key | tool | state |
|---|---|---|
| `alt+q` | Scratchpad | built — vi-ish modes, word wrap, burst undo, `y` yanks to clipboard |
| `alt+w` | Clipboard | built — background capture, search, pinning, masked resume |
| `alt+e` | Calculator | built — recursive-descent evaluator, functions, `ans` |
| `alt+r` | Calendar | not built — needs the storage layer |
| `alt+t` | Timer | built — countdown + stopwatch, notification on expiry |
| `alt+y` | Generate | not built — passwords, UUIDs, hashes |
| `alt+u` | Color | not built — converter + eyedropper (needs D-Bus signals) |
| `alt+i/o/p` | — | deliberately unspent |

Summon is `Super+D`, bound in
`~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`.

## Build and run

```
cargo build --release          # or plain `cargo build` for a debug binary
./target/release/dropkickd     # run in a terminal to see stderr
```

The keybinding runs the *release* `dropkickctl`, but both builds share one
control socket, so a release `dropkickctl` will poke a debug daemon happily.

**Only one daemon at a time.** `dropkickctl quit` before starting another. If
the socket is gone, `quit` cannot help — `pkill -f dropkickd`.

Debug builds paint ~30x slower (57ms to present vs 11ms). Fine for correctness,
useless for judging feel, never quote a latency number from one.

## How to see the UI without a compositor

```
./target/release/dropkickd --shot /tmp/dk
```

Renders every view to PPM by driving the real `App` through synthetic keys.
Convert with `pnmtopng` and look at them. This is how the UI gets reviewed —
use it rather than describing a screenshot in prose.

Other dev-only flags and env vars:

```
--bench 20                 summon latency distribution
--set-clipboard "text"     hold the selection, to exercise capture
--notify "text"            post a notification, to exercise D-Bus
DROPKICK_DEBUG=1           capture/notify logging: lengths only, never content
DROPKICK_TRACE=1           tick and focus tracing
DROPKICK_DEBUG_TIMER=3s    arm a countdown at startup, to test hidden behaviour
```

## Traps

Every one of these was a real bug that cost real time.

**Draw only when `phase == Visible`.** `Mapping` means the surface exists but
has not been configured; attaching a buffer then is a protocol error that kills
the connection.

**Layer surfaces must be destroyed and recreated per summon on cosmic-comp.**
The spec's null-buffer re-map path works exactly once, then never configures
again. Both paths remain behind `--strategy`.

**Focus: map `Exclusive`, downgrade to `OnDemand` once `wl_keyboard.enter`
arrives.** Exclusive is what gives focus in 0.43ms with no click; *holding* it
means the user cannot type anywhere else until they dismiss the panel.

**Never unlink the control socket unless this process created it.** A daemon
whose socket is stolen stays alive, listening on an orphaned inode, invisible
and unkillable by `dropkickctl`.

**The clipboard self-capture guard is load-bearing.** Taking ownership of the
selection sends the selection event straight back; reading it means writing and
reading one pipe on a single thread, which deadlocks past the pipe buffer.

**The stall watchdog's meaning depends on what was requested** — focus for a
panel the user summoned, a configure for one that asked for no keyboard.

**A key that is both a unit and a command will silently do the wrong one.**
`s` meant "stopwatch" and "seconds"; `30s` quietly did the wrong thing. Relevant
to Generate, where `p`/`u`/`h` could each mean a mode and a character class.

**`dropkick-core` has zero dependencies and must keep them.** Time enters as a
`now_ms` parameter, never read inside; that is why expiry and the Esc ladder are
testable without sleeping. Platform work goes in `dropkick-linux`; the core
raises an `app::Request` for anything only the platform can do.

**`deny.toml` fails the build if a network-capable crate appears.** The tree is
61 crates and none can open a socket. Check before claiming otherwise.

## What is next, and what each needs

- **Generate** (`alt+y`) — needs nothing new. Passwords, UUIDs, hashes are core
  logic plus the clipboard plumbing that exists. Shortest path to a fifth tool.
  Must state plainly that nothing is stored, and offer length / character-class
  / exclude-ambiguous settings.
- **Calendar** (`alt+r`) — forces the storage layer to become real: month grid,
  dots on days with events, arrow navigation, events listed for the focused day.
  Storage decisions are made and unimplemented (see the log): XDG dirs, `0600`,
  write on change with debounce, atomic rename.
- **Color** (`alt+u`) — converter is trivial; the eyedropper needs
  `org.freedesktop.portal.Screenshot.PickColor`, which uses the portal's
  Request/Response **signal** pattern. The D-Bus client has no signal support
  yet. Same missing piece as encrypted pins and as making notifications
  clickable.
- **Help overlay** — agreed, unbuilt. `Alt+/`, not `?`: `?` is Shift+/ and is
  text in insert mode. Overlay, not a tab, so it costs no slot and drops into
  the Esc ladder.
- **seccomp filter** denying `AF_INET`/`AF_INET6` — planned, would make the
  no-networking claim enforced rather than merely audited.

## Working with Brand

- Stop at decision points; do not decide product questions unilaterally.
- Explain jargon. "Used in anger", "masked resume" and "async runtime" all
  needed unpacking, and he was right to ask.
- He is the only user. His preference *is* the requirement. Do not cite rules
  back at him for changes he asked for, and do not warn about permanence — that
  was a real annoyance.
- Flag genuine costs (work, risk, breakage). Skip the rest.
- Verify rather than assert. Several claims here were wrong until measured, and
  one — "OnDemand does not disturb focus" — was wrong *because* the measurement
  was narrower than what he could see on screen. A client-side trace cannot see
  everything a person watching the screen can.
- He is fine dismissing with `Super+D` and does not expect to use `Esc` for it.
