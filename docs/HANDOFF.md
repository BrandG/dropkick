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

## State as of 2026-08-25

**All seven tools built.** 451 tests, clippy clean at zero warnings.

| key | tool | state |
|---|---|---|
| `alt+q` | Scratchpad | built — vi-ish modes, word wrap, burst undo, `v`/`V` selection, `d`/`y` operators, paste, persisted |
| `alt+w` | Clipboard | built — background capture, search, pinning, masked resume |
| `alt+e` | Calculator | built — evaluator, functions, `ans`, unit conversion, date maths |
| `alt+r` | Calendar | built — month grid, timed and all-day entries, repeats, alarms |
| `alt+t` | Timer | built — countdown + stopwatch, notification on expiry |
| `alt+y` | Generate | built — passwords, UUIDs, and sha-256/sha-1/md5 of the clipboard |
| `alt+u` | Color | built — hex/rgb/hsl/names; eyedropper written but **switched off**, COSMIC refuses it |
| `alt+i/o/p` | — | deliberately unspent |
| `alt+/` | Help | built — an overlay, not a tool; the active tool's keys plus the global ones |
| `alt+-` `alt+=` | — | live panel opacity, 5% a press, session only |

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

**Calendar alarms are the only thing Dropkick does while idle.** Everything
else sleeps on the event loop; the Timer's tick exists only while a timer
runs. `start_alarm_poll` is armed once at startup and never dropped, at
`ALARM_POLL_SECS` (30s, and it must divide a minute or an entry's minute could
be stepped over). That is a change in what kind of program this is — do not
add a second always-on poll without the same deliberation.

**Alarms poll the wall clock; they never schedule.** `tz::now_local()`, never
`now_ms` — `now_ms` is milliseconds since the daemon started and an alarm keyed
off it drifts by however long Dropkick has been up. Polling is also what makes
suspend, a manually changed clock and DST all work with no code: nothing is
scheduled in advance, so a machine that wakes at 10:00 simply sees 10:00.

**Alarms only fire for times that pass while the daemon is running.**
`started_local` is the floor. This is why nothing has to be persisted to stop a
restart re-announcing the whole day, and why the `fired` set can be in memory.
The honest cost is that a one-off missed while Dropkick was off is gone;
recurring entries absorb it, since tomorrow's still arrives. Said plainly in
the README rather than hidden.

**A recurrence marker rides in the *time* field** (`09:00/weekly`), not beside
the text. Put it before the text and an entry called "daily standup" parses as
a daily recurrence of "standup" — the same collision class as a key that is
both a unit and a command. The time field is already strictly parsed, so
nothing free-form can wander into it, and old files without a marker read
unchanged.

**Monthly recurrence needs no exceptions table.** `occurs_on` matches the day
number, and there is no 31st of February for it to match — so those months are
skipped, the 29th and 30th skip February except in leap years, and 29 February
returns every four. One rule. The panel says so under a monthly entry dated
after the 28th.

**`hash.rs` is hand-rolled, and that is only safe because of the vectors.**
Three algorithms, no dependencies, ~250 lines. Every one has published test
vectors and they are all in the tests, plus a million-byte message (which is
where a length-encoding mistake shows) and the 56-byte case checked against
coreutils rather than against this code. **MD5 is little-endian where SHA is
big-endian** — get that backwards and you get a confident wrong answer that
looks entirely reasonable, which is the whole reason `padded` takes a flag and
the vectors are not optional.

**A digest without its input is unverifiable.** The panel shows
`8 characters of "0x6FB3F2"` above every hash. The count is not decoration: a
trailing newline changes a digest completely, is invisible in a preview, and is
the most common reason a checksum does not match. `hashed_from` in `render.rs`
also replaces control characters with `·`, so the preview cannot lie about what
it hashed.

**The opacity floor is load-bearing, and it is checked in three places.**
`config::OPACITY_MIN` is 20 and the bounds live with the setting, because
there are two ways in — the file at startup and `Alt+-` while running — and a
second opinion about the floor would let one of them go under it. A panel
faded to nothing still holds the keyboard and still swallows what you type;
the only way out would be the summon key, and key repeat on a held `Alt+-`
gets there in well under a second. `stepped_opacity` is a free function
specifically so that can be tested without a compositor.

**`help::VISIBLE_ROWS` is a promise core makes about a panel it cannot see.**
Core owns the help scroll position and has no idea how tall anything is, so it
states how many rows are on screen and a render test proves the panel has room
for exactly that many — and that one more would not fit, so the constant is the
real maximum rather than a number someone rounded down. Claim one row too many
and the last row draws under the hint row, where scrolling can never reach it.
Tables longer than this scroll; the global column never does, and a test keeps
it short enough not to need to.

**`w`/`b` are exclusive and `e` is inclusive, and that is the whole reason
both exist.** `dw` stops *at* the next word, so `operator_span` ends one
character before `motion_target` says; `de` ends *on* the target. `dw` leaves
`quick brown`, `de` leaves ` quick brown`. Get it backwards and `de` strands
the last letter — a one-character bug that reads as a typo, not a motion error.

`e` also **skips blank lines where `w` stops on them**, since an empty line has
no word to be at the end of. That asymmetry is real vi behaviour, measured
against `vim -u NONE -es` rather than recalled — do the same before changing
any of it. `dw` also never joins lines — vi's own special
case, and without it `dw` on the last word of a line silently pulls the next
line up. Every arm of `operator_span` is written out for this reason; do not
try to derive them.

**`help.rs` is a claim about the other files, and claims go stale silently.**
Every row carries a `probe`: the plain characters it says work, which a test
presses and requires the tool to consume. It earned itself on the first run —
it caught `y` in Generate, though the help was right and the test's tool had a
dry entropy pool. Two render tests police the geometry: every table fits on one
screen, and the columns do not collide. The probe cannot catch the *reverse* —
a key that exists and is undocumented — so adding a key means adding its row.

**Anything the panel displays, its keys must act on.** The Color tool opens on
`color::SEED` so the tab is not blank, and `Ctrl+Y` copies it — a key that
refuses what is visibly on screen is worse than one that is not offered. The
same rule the other way round: the pane's empty-state line advertised `tab` for
the eyedropper long after `Tab` had started answering "this desktop refuses
colour picking". Both the hint row and that line now follow
`ColorTool::eyedropper`.

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

- **Tab fills the last result in the Calculator** (user, 2026-08-25) — pressing
  Tab inserts the previous line's *value* at the cursor. Distinct from `ans`,
  which is a symbol that re-evaluates: Tab would paste `515.66` as literal
  text you can then edit, where `ans` stays a reference. Both are wanted; they
  are not the same thing. Tab is unbound in the Calculator today.
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
- **Tab fills the last result in the Calculator** (user, 2026-08-25) — pressing
  Tab inserts the previous line's *value* at the cursor. Distinct from `ans`,
  which is a symbol that re-evaluates: Tab would paste `515.66` as literal
  text you can then edit, where `ans` stays a reference. Both are wanted; they
  are not the same thing. Tab is unbound in the Calculator today.
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
- **`e` in the Scratchpad, if anyone misses it** — `w`, `b` and `O` are built;
  `e` (end of word) is the obvious remaining vi motion and was not asked for.
  It goes in `motion_for` like the others, and its operator span is
  *inclusive*, unlike `w`'s — which is the whole subtlety, so read
  `operator_span` before starting.
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
