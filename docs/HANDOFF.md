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

## State as of 2026-08-22

**All seven tools built.** 298 tests, clippy clean at zero warnings.

| key | tool | state |
|---|---|---|
| `alt+q` | Scratchpad | built — vi-ish modes, word wrap, burst undo, `v`/`V` selection, `d`/`y` operators, persisted |
| `alt+w` | Clipboard | built — background capture, search, pinning, masked resume |
| `alt+e` | Calculator | built — evaluator, functions, `ans`, unit conversion, date maths |
| `alt+r` | Calendar | built — month grid, timed and all-day entries, saved to disk |
| `alt+t` | Timer | built — countdown + stopwatch, notification on expiry |
| `alt+y` | Generate | built — passwords + UUIDs; hashes deferred, input is the clipboard |
| `alt+u` | Color | built — hex/rgb/hsl/names; eyedropper written but **switched off**, COSMIC refuses it |
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

Shots are written as **`.pam`**, not PPM: the panel is no longer a rectangle and
now casts a shadow, and PPM has no alpha to represent either. Convert with
`pamtopng` to get a PNG that keeps the transparency. `pnmtopng` on a PPM writes
cut corners as black notches, which is how the committed screenshots carried two
of them for a while.

To see it against something, composite over a checkerboard:

```
cargo run --release --example preview -- 200 /tmp/dk.ppm
```

Other dev-only flags and env vars:

```
--bench 20                 summon latency distribution
--set-clipboard "text"     hold the selection, to exercise capture
--notify "text"            post a notification, to exercise D-Bus
DROPKICK_DEBUG=1           capture/notify logging: lengths only, never content
DROPKICK_TRACE=1           tick and focus tracing
DROPKICK_DEBUG_TIMER=3s    arm a countdown at startup, to test hidden behaviour
DROPKICK_DEBUG_SCRATCHPAD=. seed the scratchpad, to test save-on-quit
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

**Key repeat is the client's job on Wayland.** The compositor sends one press
and one release, never the repeats. The keyboard must be built with
`get_keyboard_with_repeat`; with plain `get_keyboard`, holding Backspace deletes
exactly one character. The `repeat_key` handler is a *different* path — native
`KeyState::Repeated`, wl_keyboard v10, which cosmic-comp does not send — so it
existing proves nothing about repeat working.

**Anything copied is recorded in clipboard history unless it says otherwise.**
`set_clipboard` records every copy, so a generated password would land in the
`alt+w` list. Secrets go out as `Request::SetClipboardSecret`, which offers
`x-kde-passwordManagerHint` — that both skips our history and asks other
clipboard managers to skip it. A separate variant, not a flag, so a call site
cannot leak one by forgetting.

**`now_ms` is not the wall clock.** It is `Instant::elapsed()` — milliseconds
since the daemon started. Anything needing a real date uses `tz::today_local()`,
which reads `/etc/localtime`. UTC is not good enough: local and UTC are on
different days for several hours every evening in this timezone.

**The surface is bigger than the panel.** `SURFACE_W`/`SURFACE_H` add room for
the shadow — 24px left, right and below, none above, since the panel is docked
to the top. Drawing code works in *panel* coordinates through `Canvas::view`,
so it never knows. **An input region is set to the panel rectangle**; without it
the shadow band swallows clicks meant for the window behind.

**Never write a file in place.** `storage::write_atomic` writes a temp file
beside the target and renames it over. The temp must be in the same directory
(rename is only atomic within a filesystem) and is created `0600` from the
start, not tightened afterwards. Saves happen on a ~2s debounce, on hide, and
on quit.

**The eyedropper is off behind one line.** `ColorTool::new` sets
`eyedropper: false`; everything behind it — the portal call, the match rule, the
signal decoding — is written and tested. Flip that field to try it again.

**An interface existing does not mean it works.** `PickColor` is present on
this system, introspectable, version 2, with a COSMIC backend that declares it —
and it returns response code 2 in ~1ms without showing anything, for any client.
Measured 2026-08-23; see the log. The eyedropper code is correct and stays.
`GlobalShortcuts` at least had the decency to be absent.

**`dropkick-core` has zero dependencies and must keep them.** Time enters as a
`now_ms` parameter, never read inside; that is why expiry and the Esc ladder are
testable without sleeping. Platform work goes in `dropkick-linux`; the core
raises an `app::Request` for anything only the platform can do.

**`deny.toml` fails the build if a network-capable crate appears**, and a
pre-commit hook now runs it. **The hook needs turning on once per clone** — git
will not enable a repository's hooks by itself:

```
cargo install cargo-deny
git config core.hooksPath .githooks
cargo deny check                      # advisories, bans, licenses, sources
```

If `core.hooksPath` is unset, nothing is being checked and it looks exactly like
everything passing. Verify with `git config core.hooksPath`.

The hook runs `--offline`: a check enforcing "this program cannot use the
network" must not itself need the network to let you commit. The cost is a
stale advisory database, so refresh it occasionally with `cargo deny fetch`.
It fails loudly if `cargo-deny` is missing rather than skipping — silently
passing when the tool is absent is how this went unenforced in the first place.

Exceptions live in the `ignore` list with a written reason and the conditions
that reopen them. Never silence a check to make it pass: a permanently red run
teaches everyone to skip it, which is the failure this file exists to prevent.
One advisory is currently accepted (`ttf-parser`, unmaintained, via `fontdue`).

## What is next, and what each needs

- **Hashes in Generate** (`alt+y`) — the deferred half. Needs a from-scratch
  SHA-256 in core, since it has zero dependencies and keeps them: ~80 lines,
  testable against published vectors. **Decided: the input is the current
  clipboard contents**, so it needs no new UI beyond a third kind alongside
  `p` and `u`.
- **Repeating calendar entries, and alarms for them** — deliberately left out.
  An entry with a time is inert; nothing notifies. The Timer already knows how
  to raise a notification, so wiring one to a calendar entry is small; recurrence
  is not, and should not be started casually.
- **Clickable notifications, and encrypted pinned clipboard entries** — both
  were blocked on D-Bus signal support, which the eyedropper has now added.
  `Bus::pick_color` is the worked example: install the match rule *before* the
  call, then read until the signal arrives.
- **The eyedropper, for real** (`alt+u`) — deferred to a later point release,
  not abandoned. The portal route is written and switched off because COSMIC
  refuses `PickColor`; see the log for the traced exchange. The route that would
  actually work here is native: cosmic-comp exports
  `ext_image_copy_capture_manager_v1`, and Dropkick already speaks Wayland, so
  capturing the screen and reading the pixel under the cursor needs no D-Bus and
  no dependency — the same call as hand-rolling the D-Bus client rather than
  taking one. Real work: the capture protocol, a full-screen picker surface, and
  cursor handling. Turning the existing portal path back on is one field
  (`ColorTool::new`), and is worth retrying whenever COSMIC updates.
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
