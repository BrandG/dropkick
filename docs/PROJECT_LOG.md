---
name: dropkick-project
description: "Goals, constraints, locked decisions, and measured platform findings for Dropkick — offline-only keyboard-complete desktop utility panel"
metadata:
  node_type: memory
  type: project
  originSessionId: ee9b8de3-8ea4-449e-ab3c-341b0af5b5a2
---

**Dropkick** is a new project (started 2026-08-13) at `/home/brandg/Documents/Dropkick`:
a small desktop utility panel — calculator, scratchpad, clipboard history,
calendar, unit conversion — summoned and dismissed by a global hotkey. It is not
a launcher clone; the distinguishing claims are architectural rather than
featural. Unrelated to the trading projects ([[opm-project]], [[carmen-project]],
[[glengarry-project]]).

## LOCKED DECISIONS (user, 2026-08-13)

- **Offline only, architecturally.** Not "works offline" — there is no
  networking code at all. No HTTP client, no update checking, no sockets except
  the AF_UNIX ones Wayland and the control channel require. Enforced by
  `deny.toml` (build fails if a network-capable crate enters the tree) and
  `#![forbid(unsafe_code)]` on the core. Planned: seccomp denying
  `AF_INET`/`AF_INET6` at startup.
- **No plugins, ever.** Plugins are the mechanism by which an offline utility
  acquires third-party code with clipboard access and a socket. New utilities
  get reviewed and compiled in instead.
- **Keyboard-complete.** The rule is "nothing *requires* the mouse", not "no
  mouse" — mouse input stays supported for discoverability and accessibility.
  vi-flavoured: command mode vs insert mode, home-row navigation.
- **Platform order:** Pop!_OS 24.04 / COSMIC first, Windows second.
- **Stack:** Rust + Cargo. libcosmic deferred — the probe uses
  smithay-client-toolkit directly, since the first milestone is interaction
  latency, not widgets.
- **Keep the dependency tree small**: no SQLite, no ORM, no logging framework
  until something actually demands one. The tree is audited on every change; the
  current count is recorded in the BUILT sections below.
  - **Correction 2026-08-15:** "no async runtime" was previously listed here as
    a locked user decision. It was not. It came from the planning document
    brought into the project, and the user has since said they did not write it
    and it was never explained to them. It is **downgraded to a preference**:
    do not take on a large async runtime without a reason, because for a program
    that sleeps until a keypress it is machinery with nothing to do. It is not a
    law, and it must not be cited to block a feature the user wants. See the
    D-Bus question.

## UI MODEL — LOCKED (user, 2026-08-13)

Dropkick has no root menu and no launcher screen. **It opens into whatever tool
was last used, with that tool's values intact.** Tools are shown as tabs across
the top; switching is a single chord. This is nearly free given the resident
daemon: "last tool with last values" is just not clearing memory. No persistence
layer, no serialisation, no disk.

This supersedes the earlier open question about modal-vs-type-ahead root
grammar. That question only existed because a root menu was assumed. With
resume, the common case carries **zero ceremony keystrokes** —
`Super+Space 437*1.18` goes straight into the calculator because that is where
you left it. The tool-switch key becomes a rare keystroke rather than a
per-invocation tax, which changes what it must optimise for: unambiguity over
speed.

**Tool switching is Alt+Q/W/E/R…, not Shift+Q/W/E/R.**

The user's first instinct was Shift. Shift+Q is not a free keystroke — it is the
letter `Q`. Harmless in the calculator (no letters accepted), fatal in Notepad,
where "Quick note…" would be eaten by the tab bar. The collision lands precisely
on the tool where text entry is the whole point. Modality could patch it (Shift+Q
switches only in command mode) but then the switch key behaves differently
depending on tool and mode, which is exactly what muscle memory cannot absorb.
Alt+letter is never text, in any tool, in any mode. Ctrl was rejected: Ctrl+W and
Ctrl+E already mean delete-word and end-of-line to anyone with readline habits.
Super was rejected: COSMIC has claimed much of it, including the summon key.

**Q/W/E/R is kept** despite being one row above strict home position, because the
top row of keys maps left-to-right onto the tabs at the top of the screen. That
spatial correspondence is why it will stick.

Rules that are cheap now and expensive later:

- **Keys bind to tools, not to tab positions.** If Alt+Q means "first tab",
  inserting a tool later shifts every binding at once. So each tool has its own
  key, tab order follows key order, and new tools take the next free letter.
  - **Scope of this rule, clarified 2026-08-15:** it protects the user's own
    fingers, nothing else. There is one user, and he is the person who feels the
    cost, so he is the person who decides when a reshuffle is worth it. Reorder
    when he asks; do not pre-emptively argue against it, and do not attach a
    warning about permanence to changes he requested. Flag real costs (work,
    risk, breakage) and leave the rest to him.
- **Esc is strictly "up one level"** — insert mode → command mode → dismiss —
  and **double-Esc always dismisses from anywhere**. Esc will be the most-pressed
  key in the product. If it sometimes dismisses and sometimes does not, the user
  must look at the screen before pressing it, which defeats the entire design.
- **Resume-safety is declared per tool.** This collides with the "design it as
  though it contains passwords" rule: if clipboard history was the last tool
  used, `Super+Space` renders the clipboard on screen instantly, in a meeting or
  in front of whoever is behind you. The calculator auto-resumes revealed; the
  clipboard resumes to the tool with entries **masked until a key is pressed**.
- **Switching tools preserves the other tools' state.** Alt+W then Alt+Q returns
  to the calculator with the expression still there.

Consequence of the measured latency (see finding 7): keyboard focus lands ~11 ms
before the first frame is presented, so **the user can type into a tool before
seeing which tool it is**. Harmless in the calculator, a mutation in Notepad.
Two mitigations, both adopted: (a) undo in any tool a stray keystroke can mutate;
(b) the summon may name a target — `dropkickctl show notes` bound to its own key
— to bypass resume when the user already knows where they are going. Still a
write-only verb that moves a window, so the socket's authority does not widen.

## MEASURED PLATFORM FINDINGS (2026-08-13, this machine)

Pop!_OS 24.04, Wayland, `cosmic-comp`. All measured, none assumed.

1. **`org.freedesktop.portal.GlobalShortcuts` is absent.** The session portal
   exports 20 interfaces; GlobalShortcuts is not among them. The plan's
   preferred hotkey mechanism is unavailable, so: user binds the key in COSMIC
   Settings → runs `dropkickctl` → Unix socket → resident daemon. Cost of that
   hop measured at **1.1 ms median / 1.4 ms p95** (fork+exec+connect+ack+exit).
   Do not build portal support. Bind to the binary directly, never via `sh -c`.
   Custom shortcuts live in
   `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom` (empty as
   of today — nothing to conflict with).

2. **Background clipboard capture works.** cosmic-comp advertises both
   `ext_data_control_manager_v1` v1 and `zwlr_data_control_manager_v1` v2, plus
   `zwlr_layer_shell_v1` v5. A client with no surface and no focus bound the
   device and read the live clipboard immediately. **Data-control is ungated —
   no prompt, no permission, no privilege.** Any program running as the user can
   tail the clipboard forever. This belongs in the README threat model (it does).

3. **The compositor pushes the current selection at bind time**, so Dropkick
   inherits whatever is already on the clipboard at startup whether it wants it
   or not. Decide deliberately.

4. **Primary selection is a separate, much noisier stream** — it updates on
   every text highlight, not every copy. Default it off.

5. **MIME types leak the source application** (a Chromium copy carried
   `chromium/x-internal-source-rfh-token`). Useful for source-based filtering
   policy; also means the MIME list is itself metadata worth treating as
   sensitive.

6. **Layer surfaces must be destroyed and recreated per summon on cosmic-comp.**
   The wlr-layer-shell spec says an unmapped surface re-maps by committing
   without a buffer and awaiting a configure. That works exactly **once** here;
   every later summon commits and no configure ever arrives (4/4 stalls after
   the first). Destroy-and-recreate works 20/20. Both paths kept behind
   `--strategy` for re-testing on other compositors.

7. **Summon latency, 20 consecutive summons (ms from daemon poke):**

   | metric | min | median | p95 | max |
   |---|---|---|---|---|
   | poke → configure | 0.29 | 0.50 | 0.74 | 1.32 |
   | poke → committed | 0.34 | 0.58 | 1.19 | 1.40 |
   | poke → presented | 1.67 | 11.42 | 36.62 | 36.88 |
   | **poke → keyboard focus** | **0.28** | **0.48** | **0.72** | **1.30** |

   **Keyboard focus consistently arrives BEFORE the first frame is committed** —
   ~0.5 ms after the poke, ~11 ms before anything is on screen. End-to-end
   keypress-to-input-live ≈ **1.6 ms** including the `dropkickctl` spawn,
   against a 50 ms budget.

   This settles the "accepts keystrokes before it finishes painting"
   requirement, and reframes it: on Wayland a client cannot buffer input it
   lacks focus for — before `wl_keyboard.enter` keystrokes go to the previously
   focused window. So the requirement is not buffering, it is *acquiring focus
   faster than the human's next keypress*. It does, by a factor of ~30.

## ARCHITECTURE CONSEQUENCE

Dropkick is a **resident daemon with a pre-created Wayland connection**, not a
fast-launching application. A cold start pays for the connection, shm/GPU setup
and font loading, none of which fits in a keystroke. This decision drives
everything downstream — including that the "build Windows from the bottom up in
raw Win32 for performance" instinct is probably misdirected: once resident, the
toolkit stops dominating interaction latency. Revisit with a measurement, not a
prior.

The daemon's control socket is the one piece of attack surface Dropkick adds:
`$XDG_RUNTIME_DIR/dropkick.sock`, mode 0600, four verbs that move a window. No
verb reads clipboard history back out, and none may ever be added.

## OPEN QUESTIONS

- ~~Root interaction grammar~~ — RESOLVED 2026-08-13, see UI MODEL above.
- Which tool owns which permanent key, beyond the first four (Calculator,
  Calendar, Notepad, ColorPicker ⇒ Alt+Q/W/E/R). Cheap to decide, impossible to
  change later.
- In-tool keymaps are unspecified. They are free-form: tool switching lives on a
  modifier, so no in-tool binding can collide with it.
- ~~Does resume survive a daemon restart~~ — RESOLVED 2026-08-15, see STORAGE
  AND CAPABILITIES.
- ~~`Alt+N`/`Alt+P` worth a slot~~ — RESOLVED: no, `Alt+H`/`Alt+L` only.
- ~~Final key allocation and the rename~~ — RESOLVED, roster locked.
- **D-Bus client choice**, still open, and on the critical path for both
  notifications and the eyedropper. Previously framed as blocked by a "no async
  runtime" rule; that rule was inherited rather than chosen (see the correction
  above), so this is now an ordinary trade-off: `zbus` is the mainstream crate
  and brings an async runtime and a few dozen crates; the `dbus` crate wraps the
  libdbus C library instead; a hand-rolled client for the two or three calls
  actually needed is also viable. Decide on weight and maintenance, not on a
  rule nobody agreed to.
- Which tool, if any, takes a reserved slot (`i` `o` `p`). Character picker is
  the only candidate raised, and only weakly.
- Password-manager hint (`x-kde-passwordManagerHint: secret`) skip policy is
  designed but **untested** — needs a real password-manager copy to confirm.
- Accessibility: custom-drawn toolkits (Iced/libcosmic) have limited AT-SPI
  support. A manifesto about respecting users should say so out loud.
- Licence not yet chosen (workspace currently declares MIT).
- On-disk format stability and data portability unspecified.

## BUILT 2026-08-14 — tab bar, Calculator, Notepad

`dropkick-core` now holds the whole interaction grammar and both tools, with 29
unit tests and still **zero dependencies**. The platform crate provides only
rendering and a keysym translator; the tools take a `KeyPress`, never a keysym,
which is what makes the grammar testable without a compositor.

- **Calculator** — recursive-descent evaluator: `+ - * / % ^` (right-associative
  power), parentheses, unary minus, `sqrt abs ln log sin cos tan round floor
  ceil`, `pi`/`e`, and `ans` for the previous result. Errors are reported and the
  input is kept for repair rather than cleared.
- **Notepad** — command/insert modes, `i a A o` `hjkl` `0 $` `x` `u`. **Undo is
  per burst, not per keystroke**: a snapshot is taken when a mutation begins, so
  one `u` reverses a whole typing run. That is what makes the stray-keystroke
  problem (focus lands ~11 ms before paint) actually recoverable, and there is a
  test named for exactly that case.
- **Tab bar** — all four tools always visible; unbuilt ones dimmed and holding
  their permanent keys. Pressing `Alt+W` says "Calendar isn't built yet" rather
  than doing nothing, so the key never feels broken.
- Tests pin the decisions themselves, not just the code: `Alt` switches tools
  from inside insert mode; a typed capital `Q` stays text and never switches
  tabs; switching preserves both tools' state; the Esc ladder climbs one level
  per press and double-Esc dismisses from the deepest level.

**Rendering costs nothing that matters.** Measured live with the real UI:
`FOCUS` stayed at 0.45–0.55 ms, identical to the flat-fill probe, because
painting happens *after* focus is granted. First paint rose to 3.03 ms on a cold
glyph cache and settled at 0.92 ms once warm. The interaction budget is
untouched.

`dropkickd --shot <prefix>` renders frames to PPM with no compositor involved,
driving the same `App` through synthetic key presses. The UI can therefore be
reviewed as an image in CI or a code review rather than described in prose.

One dependency added: `fontdue` (glyph rasterisation) in the platform crate
only. Core stays at zero. Tree is 61 crates, none network-capable.

## FIXED 2026-08-15 — panel dismissed itself after 1.2s

First real interactive use found a bug the benchmarks could not: the panel
appeared and closed again about a second later, untouched.

Cause was the stall watchdog added alongside the `--strategy` work. It armed on
every summon and treated "cycle still open" as "summon stalled" — but an open
cycle is the *normal* state of a panel someone is typing into; it only closes on
hide. So it dismissed every working summon at 1.2s. The log said as much once
looked at directly: `[watchdog] summon 1 never completed (phase Visible)`
alongside `FOCUS 0.74ms`, which is a completed summon by any reading.

Now the watchdog checks whether focus ever arrived, which is what "stalled"
actually means. Verified both directions: `--strategy remap` is still detected
as stalling, `--strategy recreate` still benches clean, and an idle panel now
stays up indefinitely.

Worth noting as a pattern: bench mode hid this. Bench always hides after 90ms,
so the 1.2s timer never had a chance to fire. A harness that drives itself
faster than a person will not find bugs that only a person's pace exposes.

**Keybinding decided (user, 2026-08-15): `Super+D`.** `Super+Space` is left to
COSMIC's `System(InputSourceSwitch)`. A custom binding does override the default
— tested, both worked — but the user's call was that a chord the desktop already
defines should stay with the desktop. Free single-letter Super chords on COSMIC
are `c d e n p v z`.

Also fixed: `dropkickctl` now starts `dropkickd` on demand and retries for up to
3s. A keybinding runs with no terminal, so a ctl that could not reach a daemon
exited quietly and was indistinguishable from a dead key. Cold start 25ms, then
the ~1ms path. No autostart entry needed.

## TOOL ROSTER — decided 2026-08-15 (user)

**The top row is the cap: ten tools, ever.** Tool keys come from `q w e r t y u
i o p`, keys are permanent, and retired keys stay retired. The user's framing:
adding a tool should be a BIG decision. The ceiling is treated as a feature —
it is the "is this a small desktop utility?" rule made physical, and an eleventh
tool is a signal that two of the ten should have been one.

Dispositions:

| tool | decision |
|---|---|
| Calculator | built. Absorbs **unit conversion** and **date arithmetic** |
| Scratchpad | built (as "Notepad"). Absorbs **text transforms** |
| Clipboard history | the flagship; masked resume already specified |
| Calendar | event grid, not a viewer — see below |
| Timer/Stopwatch | approved **including notifications** |
| Generate | password + UUID + hash folded into one tool, one key |
| Color | converter **and** eyedropper, both approved |
| Character picker | optional; a free slot exists if wanted |

Folded in rather than given keys: unit conversion (into Calculator), date
arithmetic (into Calculator), text transforms (into Scratchpad), timezone
conversion (into Calendar). Rejected outright: a snippet expander — pinned
clipboard entries already are snippets.

Per-tool notes from the user:

- **Unit conversion needs discoverability.** With no network there is no LLM to
  interpret "12 furlongs in RPM", so the calculator must *show* its supported
  units and formats. This is the strongest argument for the help screen below.
- **Hash takes pasted text only.** No file picker — that is a whole subsystem
  for one feature.
- **Password generator must state plainly that nothing is stored.** Wording to
  the effect of: passwords are not retained in any fashion; saving it somewhere
  safe is the user's job. Settings needed: length, alpha, numeric,
  capitalisation, special characters, and exclusion of visually similar
  characters (`I` or `1`, not both).
- **Calendar** is a month grid with a dot on days holding events, arrow-key
  navigation, and the focused day's events listed at the bottom. Add/remove
  stays basic. Reminders are wanted but depend on the notification decision.
  Rationale for building it at all despite the clock applet having a calendar:
  *there is no keypress that brings the applet up*, so checking a date means
  reaching for the mouse and leaving the home keys.
- **Timezones**: a settable secondary time display, visible while using the tool.

## HELP SCREEN — required (user, 2026-08-15)

Every tool gets a one-page cheatsheet, flicked on and off, showing that tool's
keys and capabilities. It is the answer to unit-conversion discoverability and
to the general problem of a keyboard-only app being unguessable.

Design consequences:

- It is an **overlay, not a tab** — it costs no slot, keeps the tool's context
  behind it, and slots naturally into the Esc ladder as one more level.
- **`?` cannot be the binding.** `?` is Shift+/, which is *text* in the
  Scratchpad's insert mode — the identical collision that disqualified Shift for
  tab switching. Help is global, so by the established rule it belongs on Alt.
  `Alt+/` is the natural choice; `?` may additionally work where no text entry
  is active, but must never be the only way in.

## TAB CYCLING — added 2026-08-15

`Alt+H` previous, `Alt+L` next, skipping unbuilt tools so cycling never lands on
a dead tab.

The user asked for `Alt+P`/`Alt+N`. **`Alt+P` would permanently cost the tenth
tool slot**, since tool keys come from the top row and `p` is in it. `Alt+N` is
free forever (`n` is not a top-row key), but a half-implemented pair is worse
than a consistent one. `Alt+H`/`Alt+L` cost nothing, sit on the home row, and
match the directions the Scratchpad already uses for motion. Adding `Alt+N`/
`Alt+P` as aliases remains available at the price of one slot — user's call.

## FIRST REAL USE — 2026-08-15

The user wrote notes on a design discussion in the Scratchpad, and reported the
experience as better than opening a window over the thing being commented on:
instant, and out of the way. Two gaps found by use rather than by testing:

- **No word wrap.** Long lines ran off the right edge and the rest of the
  sentence was simply gone. **Fixed**: `dropkick_core::text` does word wrapping
  with mid-word breaks only for words longer than the panel, plus scrolling that
  keeps the cursor on screen and a `row/total` marker when content overflows.
  Wrapping is presentation only — the buffer keeps real lines, so nothing gains
  a hard break it did not have.
- **No way to save.** Still open, and it is the same question as "does resume
  survive a daemon restart". See OPEN QUESTIONS.

## PORTAL FINDINGS FOR THE APPROVED FEATURES (2026-08-15)

Both newly approved features land on D-Bus, and both are available here:

- **`org.freedesktop.portal.Screenshot.PickColor`** exists (version 2). This is
  purpose-built for eyedroppers: it returns a single colour rather than a screen
  capture, so the eyedropper does **not** need ongoing screen-capture
  permission. Much cheaper than assumed.
- **`org.freedesktop.portal.Notification.AddNotification`** exists (version 1),
  with an `ActionInvoked` signal for clickable actions.

**But both require a D-Bus client, and that is an unresolved dependency
decision.** D-Bus is AF_UNIX, so it does not touch the no-networking rule — the
planned seccomp filter blocks AF_INET/AF_INET6 only. The problem is that the
mainstream Rust D-Bus crate (`zbus`) brings an async runtime and a few dozen
crates with it. Options: `zbus`, the `dbus` crate (libdbus bindings), or a
hand-rolled minimal client for the two or three calls actually needed. Not yet
decided, and explicitly not blocked by the old "no async" wording.

Also note this makes the timer the **first tool that acts while Dropkick is
hidden**. Every other tool is inert when dismissed. That changes the daemon from
"a window that appears" into "a process that does things while you are not
looking", which is a real change in what Dropkick is, and deserves to be a
deliberate yes rather than a side effect of wanting a timer.

## STORAGE AND CAPABILITIES — LOCKED (user, 2026-08-15)

### Optional capabilities: only where a user actually gains something

- **Eyedropper gets no toggle.** `PickColor` runs only when the user presses the
  eyedropper key. A setting could disable nothing that pressing-the-key does not
  already control, so it would be config noise — a switch whose sole effect is
  breaking a feature you must deliberately invoke.
- **Notifications get a runtime preference**, because they are the one thing
  Dropkick does that *interrupts*. "Don't interrupt me" is a legitimate want,
  like muting sounds.
- **A runtime toggle is explicitly NOT a security boundary.** It is a promise,
  which is the category this project exists to avoid. Untrustworthy Dropkick
  ignores its own checkbox; a user who does not trust the binary gains nothing
  from the binary's own setting.
- **The security-grade version is a compile-time feature.** A `dbus` cargo
  feature, on by default; `--no-default-features` produces a binary with no
  D-Bus code linked at all — no notifications, no eyedropper — verifiable by
  inspecting the binary exactly the way the no-networking claim is. Property,
  not promise. Cheap to set up now while only two features depend on it.

### Where data lives

- **Durable:** `$XDG_DATA_HOME/dropkick/` (`~/.local/share/dropkick/`),
  directory `0700`, files `0600`. Calendar events, Scratchpad contents, pinned
  clipboard entries.
- **Ephemeral:** `$XDG_RUNTIME_DIR/dropkick/` — already `tmpfs, mode=700,
  uid=1000` on this machine, so RAM-backed, private, and wiped at logout.
- **`/tmp` rejected.** The user's instinct was ephemeral-and-safe, which is
  exactly `$XDG_RUNTIME_DIR`; `/tmp` is neither. It is mode `1777` and shared
  with every local user, so file *names* are visible even when contents are
  `0600`; `systemd-tmpfiles` ages contents out on its own schedule; and on this
  machine it is disk-backed, so it is not even ephemeral. Worst of both.

### Encryption: a narrower claim than it looks

`0600` in the XDG directories is the honest baseline. **Encryption is reserved
for pinned clipboard entries and anything explicitly marked secret**, keyed
through the OS keyring (Secret Service — the DPAPI analogue named in the
original plan).

What that encryption actually buys, and how it must be described: it protects
data **that leaves the machine** — a backup, a synced folder, a stolen or
resold disk. It does **not** protect a running machine compromised under the
user's account, which the threat model already concedes in writing. If Dropkick
can decrypt unattended, so can anything running as the user; anything stronger
than that claim would be obfuscation sold as encryption.

A typed passphrase would be genuinely strong and is **rejected**: unlocking the
scratchpad at every login is the "sign in to continue" this project exists to
refuse.

### When writes happen

**On change, debounced ~2s after the last edit, plus on hide and on clean
shutdown.** Writes are atomic — temp file then rename — so a crash mid-write
cannot corrupt the file.

A periodic timer (the user's first suggestion, once per minute) is **rejected**:
it writes when nothing has changed, it still loses up to a minute of work, and
it adds another thing the daemon does while hidden.

### Per-tool persistence, mirroring the resume-safety rules

| tool | default |
|---|---|
| Scratchpad | persists |
| Calendar | must persist — pointless otherwise |
| Clipboard | session-only; pinned entries persist, encrypted |

Each is user-overridable, including turning persistence off entirely.

### Scratchpad export is the clipboard, not a file picker

"No way to save what I'd written" is two features. Automatic persistence means
*it is still there next time*. Getting text **out** of Dropkick is a separate
need, and the answer is to yank the buffer to the system clipboard: one key, no
dialog, paste it wherever you want. No file picker enters the project. Build it
alongside the Clipboard tool.

## BUILT 2026-08-15 — Clipboard

The flagship, and the first tool where a bug leaks secrets rather than losing a
note. The four rules went in with the first commit rather than being retrofitted:

1. **Masked on arrival.** `on_enter()` re-masks every time the tool becomes
   active *and* on every summon. The first keypress reveals and is consumed
   doing nothing else, so a masked list cannot be navigated, pasted from or
   deleted from blind. `Esc` is routed by the app layer and never reaches the
   tool, so it can never be the key that reveals.
2. **Password-manager hints obeyed.** Any offer carrying
   `x-kde-passwordManagerHint` is dropped, matched as a substring so a
   `: secret` suffix still counts.
3. **Session-only.** History is in memory. Nothing reaches disk.
4. **`c` pauses capture, `x` clears unpinned, `nuke()` takes everything.**

Verified live against cosmic-comp:

- Background capture works from Rust, not just the Python spike: three copies
  logged at exactly 34, 21 and 23 bytes.
- **The startup capture is real.** The daemon recorded a 295-byte entry at bind
  time with no copy involved — the compositor pushing the existing selection,
  as predicted in finding 3. It is kept: the user did copy it, it is on the
  clipboard now, and the password-hint check applies to it identically because
  the MIME list arrives with the offer.
- **The write path is verified by the read path.** `--set-clipboard` creates a
  real data source; the reading daemon got the exact byte counts back, which
  means the source served them correctly.
- **Self-capture guard works.** Taking ownership of the selection sends the
  selection event straight back; reading it would mean writing and reading one
  pipe on a single thread, which deadlocks once the text exceeds the pipe
  buffer. `skip_next_selection` suppresses it — verified by a process setting a
  30-byte selection and logging no capture of its own text.

Reads never block the event loop: the compositor gets a pipe, and the far end is
registered as a calloop source read to EOF, capped at 4MB against a source that
streams forever.

`DROPKICK_DEBUG=1` logs captures as **length and MIME count only, never
content** — a debug flag that printed clipboard text into a terminal scrollback
would undo the point of the tool.

Also built: **`y` in the Scratchpad yanks the buffer to the system clipboard**,
which is the no-file-picker answer to "no way to save what I'd written". Core
never touches a system clipboard itself; it raises an `app::Request` that the
platform carries out.

Deferred deliberately: **persistence**. Session-only is the default anyway, and
encrypted pinned entries depend on the unresolved keyring/D-Bus choice. Nothing
about the storage decisions changes; they are simply not implemented yet.

## FIXED 2026-08-15 — an unkillable daemon

Reported symptom: `dropkickctl quit` run several times, and `ps aux | grep
dropkick` still showed a daemon. Two bugs compounding, both mine.

**1. Non-daemon modes deleted a running daemon's socket.** `main` ended with an
unconditional `remove_file(socket_path())`, but `--bench`, `--shot` and
`--set-clipboard` never *create* a socket. Running `--bench` while a daemon was
up therefore unlinked that daemon's socket. Which is exactly what happened —
the socket was destroyed by a benchmark run during a debugging session, not by
anything the user did.

**2. Startup unlinked the socket path blind.** `spawn_listener` removed any
existing socket before binding, with a comment claiming this was safe because a
live daemon would have answered a connect first. It never attempted that
connect. The justification described code that did not exist.

Either bug alone produces the same end state: a daemon **alive, holding a
listening socket on an orphaned inode, and unreachable forever**. `dropkickctl
quit` then fails to connect, helpfully auto-starts a *fresh* daemon, quits that
one, and reports success while the real daemon survives untouched. Every quit
attempt spawned and killed a new process.

Fixes: `bind_control_socket` now tries `bind` first and only reclaims the path
if a `connect` proves nobody is listening; a second daemon refuses to start and
exits non-zero rather than stealing the socket. `main` removes the socket only
if this process created it.

Pinned by four tests in `ipc.rs` covering a free path, a live owner, the corpse
a crash leaves behind, and the `0600` mode. This class of bug is invisible from
the outside — the process list looks normal and the tool simply stops
responding — so it is worth the tests.

Lesson worth keeping: **a comment asserting an invariant is not the invariant.**
The reasoning in that comment was sound; the code just did not implement it, and
nothing checked.

## ROSTER REORDERED + CTRL+C ALIAS — 2026-08-15 (user)

**Order is now Scratchpad `q`, Clipboard `w`, Calculator `e`.** Calendar, Timer,
Generate and Color keep `r t y u`; `i o p` stay unspent. First launch lands on
Scratchpad rather than Calculator; after that, resume decides.

The alternative — keeping keys and only reordering the *display* — was rejected
because it breaks the reason the top row was chosen at all: the keyboard's
left-to-right order mapping onto the tabs. Tabs reading `w e q` would make the
keys arbitrary.

**`Ctrl+C` is a full alias for `Esc`**, requested because terminals that swallow
`Esc` make it a common habit. Implemented at the top of `App::on_key` by
rewriting the press, so it is genuinely the same key rather than a special case
in one tool: it climbs the same ladder, participates in the same double-tap, and
mixes freely with `Esc` (one of each in quick succession still dismisses). A
scratchpad-only alias was rejected — an alias that behaves differently depending
on where you are is worse than no alias.

Plain `c` is unaffected: it is text in insert mode and pauses capture in the
Clipboard, because `Mods::is_plain()` excludes ctrl.

## D-BUS DECIDED AND BUILT — 2026-08-16

**Hand-rolled minimal client.** `crates/dropkick-linux/src/dbus.rs`, ~500 lines,
behind a `dbus` cargo feature that is on by default.

Rejected alternatives and why:

- **`zbus`** — the mainstream choice, works fine, brings an async runtime and
  several dozen crates. Note the old "no async runtime" objection was never a
  real rule (see the correction above), so this was decided on weight, not
  dogma.
- **`dbus` crate** — wraps libdbus, so it adds a C library and a system package
  (`libdbus-1-dev`, needing root to install) to a project whose build currently
  needs nothing unusual.

Neither threatens the no-networking promise: D-Bus is an `AF_UNIX` socket and
the planned seccomp filter blocks `AF_INET`/`AF_INET6` only.

What decided it: Dropkick needs **three calls**, not a library — a notification
for the Timer, `PickColor` for the eyedropper, and a secret for encrypting
pinned clipboard entries. And hand-rolling makes the capability flag mean
something checkable. With `--no-default-features` the binary contains no D-Bus
code at all, verified by inspection:

```
--no-default-features:  strings | grep -c org.freedesktop.Notifications  ->  0
default:                                                                ->  1
```

**The dependency tree did not grow: still 61 crates.**

Verified end to end: `dropkickd --notify "..."` posted a real notification and
the server returned id 13.

The honest cost is that the marshalling is ours to get right. Alignment and
padding are where D-Bus implementations fail, so the encoder is tested against
byte layouts from the specification — 11 tests, including the one that matters
most: **an empty `a{sv}` still needs its 8-byte padding**, which is the single
most common D-Bus encoding bug and produces a message the bus rejects with no
useful diagnostic.

Deliberately absent: signal handling, properties, introspection, object server,
fd passing. `PickColor` and the secret portal both use the portal
Request/Response *signal* pattern and will need signal support added; the
notification path does not, which is why it went first and why the classic
`org.freedesktop.Notifications` interface was used rather than the portal.

## BUILT 2026-08-16 — Timer

Countdown and stopwatch on `alt+t`. Durations parse as `25m`, `90`, `1h30m`,
`2m30s`; a bare number is seconds. `Enter` arms, `space` starts/pauses, `w`
runs a stopwatch, `r` resets.

**The first tool that acts while Dropkick is hidden.** Time enters the core as a
parameter, exactly as `now_ms` already does for the Esc ladder, so expiry is
tested without sleeping — including the case that actually happens in
production, a tick arriving *long* after expiry.

The tick source lives exactly as long as the timer runs, at 100ms while the
panel is up so the readout moves, and once a second while hidden. That is enough
to notice a countdown has expired without waking ten times a second forever
behind a stopwatch someone left running. `Timer::tick` returns true exactly
once, so a late tick still fires and a slow one does not fire twice.

A running timer shows in accent on its tab from every other tool, since the
point of a countdown is that you stop thinking about it.

Notifications connect to the bus per notification rather than holding a socket
open for the life of the daemon; the handshake costs a millisecond or two and
these are rare. Without the `dbus` feature the timer still counts down, it just
cannot tell anyone who is not looking — verified in both builds.

### The bug the harness found

`s` was bound to "start stopwatch" while also being a duration unit, so typing
`30s` — the most natural way to write thirty seconds — silently discarded the
duration and switched modes. Nothing errored; the countdown just never counted.

It surfaced only because `DROPKICK_DEBUG_TIMER` drove the real keymap rather
than calling `arm()` directly, and then only because tracing showed
`remaining=0` on the first tick instead of the notification simply never
arriving. Absence of an error looks identical to never having run.

Stopwatch moved to `w`. The rule worth keeping: **a key that is both a unit and
a command will silently do the wrong one.**

## ALARMS MADE HARD TO MISS + SETTINGS — 2026-08-16 (user)

First real use of the timer: "a tiny box in the upper right corner that
disappeared before I could even read it... if I'm not watching for it, I will
miss it every time." Clicking it did nothing.

Cause: the first implementation passed **no hints at all** and `expire_timeout =
-1`, meaning "server decides", and the server decides on a few seconds. That is
right for *copied to clipboard* and useless for *your timer went off*.

`GetCapabilities` on `cosmic-notifications` reports `body, icon-static,
persistence, actions, action-icons, body-markup, body-hyperlinks, sound` — three
of which were going unused. Now:

- **urgency = 2 (critical)** and **expire_timeout = 0**, so it stays until
  dismissed. Confirmed by the user: "that one waited for closure".
- **`sound-name = alarm-clock-elapsed`**, the XDG sound naming spec's name for
  exactly this event.
- **The panel itself can appear** — `timer.show_panel`, **off by default since
  2026-08-16 at the user's request**: it covers the top of the screen, and a
  notification that waits for you is usually enough. Kept as a setting for
  anyone who wants an alarm that is unmissable rather than merely patient.

  **Correction.** The claim that `OnDemand` avoids disturbing focus was wrong,
  and the measurement that appeared to support it was too narrow. Dropkick never
  receives `wl_keyboard.enter` — that part is true and traced — but the user
  observed their focused window losing focus anyway when the overlay mapped.
  Both are consistent: the compositor deactivates the focused window without
  handing the keyboard to the layer surface, so keystrokes land nowhere at all.
  That is worse than taking focus outright, because nothing recoverable holds
  them.

  The alarm panel now maps with `KeyboardInteractivity::None`, which states
  outright that the surface must never be given the keyboard, rather than
  `OnDemand`, which merely declines to demand it. Lesson: "we did not receive
  X" is not evidence for "nothing was disturbed" — the user watching the screen
  saw something the client-side trace could not.

The panel maps with `KeyboardInteractivity::OnDemand`, not `Exclusive`. An alarm
that stole the keyboard mid-sentence would eat what you were typing into
something else — the alert must be impossible to miss and impossible to have
damaged anything.

Clicking the notification still does nothing: that needs declared actions and
the `ActionInvoked` signal, which the D-Bus client does not do yet. Same missing
piece as `PickColor`.

### Settings exist now

Sound is the second thing wanting a switch (notifications were the first), so
the config file is real:

`~/.config/dropkick/config`, plain `key = value`, written with every option and
an explanatory comment on **first run** — options discoverable by opening the
file rather than by reading documentation. Parsing lives in core and is tested;
only the path and the IO are in the platform crate. No TOML, no serde, no
dependency.

**Unrecognised keys are reported at startup, never ignored.** A settings file
that silently swallows `timer.sond = false` is worse than having no settings,
because it looks like it worked. There is a test asserting the shipped default
file itself parses clean, so the file written on first run cannot drift from
what the program does.

### And a protocol error, caught by the alarm path

Showing the panel from the tick handler killed the Wayland connection:
`must ack the initial configure before attaching buffer`. Three draw sites
guarded on `phase != Hidden`, but `Mapping` means the surface exists and has
*not* been configured yet; drawing then is a protocol violation. Now they guard
on `phase == Visible`. The clipboard capture path had the same latent bug — a
capture completing during the map window would have killed the connection the
same way, intermittently and for no visible reason.

## SETTINGS RELOAD ON SUMMON — 2026-08-16 (user)

Settings were read once, at daemon startup, and never again. Since
`dropkickctl` auto-starts the daemon and it can live for days, editing the file
did effectively nothing — the worst kind of bug, because it looks like the
setting does not work rather than like the program did not read it.

Now checked on **every summon**, which is the user's own suggestion and the
right hook: it is the moment the user is present, it needs no timer, no inotify
and no reload verb on the control socket, and there is nothing to remember.

A `stat` guards the read, so the file is only parsed when its mtime moves.
Measured against the previous build: `poke -> FOCUS` 0.56ms median versus
0.48ms, inside run-to-run noise on a budget of ~1.6ms end to end.

The reload also sets the panel's notice line rather than only printing to
stderr. Launched from a keybinding there is no terminal, and a setting that
applies silently is indistinguishable from one that did not.

## FOCUS: EXCLUSIVE TO ACQUIRE, ON-DEMAND TO HOLD — 2026-08-16

The user opened the panel and could not type into anything else until they
dismissed it: "no other program pulls focus constantly. Why are we the only
one?"

Fair, and the answer is that Dropkick is not an ordinary window. It is a
layer-shell surface on the Overlay layer, the protocol meant for panels, docks
and lock screens. `KeyboardInteractivity::Exclusive` tells the compositor to
give it the keyboard and *keep it there*, so clicking another window did
nothing. The programs that behave this way are the other layer-shell ones —
rofi, wofi, fuzzel. Ordinary apps are `xdg_toplevel` and get ordinary focus.

Exclusive was not gratuitous: it is what makes a summon 1.6ms end to end, with
focus landing before the first frame is even committed. But it was being held
for the whole session rather than just long enough to acquire.

**Now: map Exclusive, then downgrade to `OnDemand` the instant
`wl_keyboard.enter` arrives.** The fast summon is unchanged — measured 0.43ms
median to focus, against 0.48ms before — and ordinary behaviour is restored:
click away and focus leaves, click the panel and it returns. **Verified by the
user 2026-08-16**: clicking another window returns focus to it. cosmic-comp
honours the downgrade, so the fallback plan of moving to an `xdg_toplevel`
window is not needed.

The first instinct was to add a key that releases the keyboard. That was the
wrong shape of answer: the user was not asking for a new gesture, they were
asking why the obvious one did not work. A workaround for a behaviour nobody
else has is worse than not having the behaviour.

Consequence worth knowing: a panel that has lost focus cannot receive `Esc`, so
the summon key dismisses it. **Settled by the user 2026-08-16:** `Super+D`
toggling open and closed is fine, and he does not expect to use `Esc` for
dismissal anyway. So the panel stays visible after losing focus — no auto-hide,
which also suits keeping the Scratchpad up as a reference while typing
elsewhere.

## HELD KEYS DID NOT REPEAT — 2026-08-16 (user)

First sustained use: "keypress holds are not recognized. If I try to delete a
whole lot of characters in the scratchpad by holding down Backspace, it only
deletes one character." True of cursor movement too, and of every key.

**Cause: on Wayland, key repeat is the client's job.** The compositor sends one
press and one release and nothing in between — unlike X11, where the server
generates the repeats. `wl_keyboard.repeat_info` tells the client the user's rate
and delay and expects it to synthesise the rest.

`new_capability` built the keyboard with `SeatState::get_keyboard`, which does no
repeat. A `repeat_key` handler was already implemented, and commented as though
it did the job — it had never once run. **A handler existing is not evidence that
anything calls it.**

Now `get_keyboard_with_repeat`, driven by the calloop loop the daemon already
has for timer ticks, so no new dependency and nothing new to shut down. The
compositor's own rate and delay are used, so held keys feel like they do
everywhere else.

`KeyboardHandler::repeat_key` is kept and its comment corrected. It is a
genuinely different path: SCTK only calls it for `KeyState::Repeated`, which
wl_keyboard gained in v10 and cosmic-comp does not send. Client-side repeat goes
through the installed callback instead. Both end in `dispatch_key`, so a
compositor that grows native repeat needs no change here.

Repeat is bound to focus — SCTK drops the repeat timer when the surface dies or
`leave` arrives — which is correct and needs no work given the Exclusive-then-
OnDemand downgrade.

**Not verifiable by test.** This is compositor behaviour; `--shot` drives the
`App` directly and never touches the platform keyboard. It needs a human holding
a key — **verified by the user 2026-08-16**, so cosmic-comp does send
`repeat_info` and the client-side path is doing the right thing with it.

## dd DELETES A LINE — 2026-08-16 (user)

Asked for directly, after "I have no way of clearing out the whole scratchpad
other than hitting the delete key once for every key."

`d` was unbound, so there is no collision, and `dd` is what vi muscle memory
already expects. It needed the first operator-pending state in the Scratchpad:
`pending: Option<char>`, set by `d` and consumed by the next key.

Decisions inside it, each a place it could have gone wrong:

- **An aborting key does not also run.** `dj` is not an operator we implement; vi
  aborts on it. Aborting *and* moving would leave the cursor somewhere the user
  did not ask for, so the next `dd` would delete the wrong line. The key is
  swallowed.
- **Esc aborts the pending operator and nothing else.** This is a new innermost
  rung of the Esc ladder. It had to go in `leave_one_level`, not in the abort
  path in `command_key`, because Esc is intercepted at the app layer and never
  reaches `command_key` at all.
- **The buffer is never empty.** `dd` on the last line leaves one blank line,
  as vi does. Every other method indexes `lines[cy]` assuming a line exists.
- **`on_enter` clears a half-typed operator**, and is called unconditionally
  rather than only when the Scratchpad is the tool being entered — switching
  *away* mid-operator is exactly the case that strands one. A stranded `d` would
  silently eat the first `d` typed on the way back.
- Undo already covers it: `dd` snapshots like any other command-mode edit, so
  one `u` puts the line back.

10 tests, one per decision above. 129 total, clippy clean.

**Emergent, worth knowing:** now that repeat works, *holding* `d` alternates
pending/delete and so deletes a line every second repeat. It works as a crude
wipe. It is a side effect, not a design, and it is not what a whole-buffer wipe
should rest on.

## VISUAL SELECTION, AND y BECOMES A REAL OPERATOR — 2026-08-16 (user)

Asked for while `dd` was being built: "if we want to do the G→gg pattern, we
should implement a selection policy so that we could say G→gg→d to select all of
the text, and then delete the selection. We will want to have partial selection
in any case, right?"

Yes — and **the wipe was the weaker half of the argument.** `y` used to yank the
whole buffer, because that was the only way text got out of Dropkick without a
file picker. Partial *yank* is what selection really buys. Deleting a selection
falls out of the same machinery, and the whole-buffer wipe stops needing a
gesture of its own.

**The user chose vi mappings over the existing behaviour**, for a reason worth
recording: "I would try to stick with VI mapping for y, so that new users (who
know Vi) will be able to use it easier."

So `y` is an operator now. Yanking everything became `ggyG`, which is a real cost
on the action this tool is most used for — and is why `Y` was then spent on
"yank all" instead of on vi's meaning. See below.

### What exists

```
v / V        select by character / by whole line
gg / G       first / last line, also useful on their own
d y x        act on the selection
dd yy        the doubled operator forms
Y            yank the whole buffer -- NOT vi's Y, see below
d{motion}    dj dk dgg dG d0 d$ dh dl, and the same for y
esc          cancel the selection, then the mode, then the tool
```

### `Y` is the one deliberate departure from vi

`Y` yanks the **whole buffer**. In vi it is `yy` — a synonym for yanking the
line. (Trivia that cost a lookup: `Y` is where vi contradicts itself, since `D`
is `d$` and `C` is `c$` but `Y` takes the whole line; Vim's own manual admits it,
and Neovim changed the default to `y$` in 0.6.)

**Decided by the user, twice, after the collision was put in front of them.**
The reasoning is that `ggyG` is a lot of keys for the thing this tool is most
used for — writing something down and taking it somewhere else — and that was
a single `y` until this change.

The cost, stated and accepted: a vi user pressing `Y` expecting one line gets
the entire buffer, and does not find out until they paste. It is silent and it
surfaces somewhere else.

The mitigation is that **`Y` is in the hint line and the vi-standard keys are
not.** That is the rule for this keymap now: keys a vi user can guess do not need
advertising, and keys that would fool one do.

### Decisions inside it

- **One motion table.** `motion_for` is shared by plain movement, selection
  extension, and operators, so `l`, `dl` and `vl` cannot drift apart.
- **Operator ranges are written out, not derived.** `d$` takes the cursor
  character through the end; `d0` stops *before* the cursor. vi's
  inclusive/exclusive algebra is subtle enough that a clever general rule would
  be wrong in a way nobody notices until it eats a character. There are nine
  motions; they are each spelled out.
- **A motion that cannot move does nothing.** `dj` on the last line deletes
  nothing rather than falling back to deleting one line — the fallback is the
  silent-wrong-thing failure this project keeps running into.
- **`g` gets its own pending flag**, resolved before the operator, because `dgg`
  is an operator waiting on a motion that is itself two keys. `g` followed by
  anything else abandons the operator too.
- **The anchor is clamped like the cursor.** It is a buffer position and the
  buffer can shrink under it; an anchor past the end would panic on the slice.
- **Esc unwinds selection before mode before tool**, and `on_enter` drops a
  selection outright. Returning to a highlight you do not remember making, with
  `d` armed, is the worst of the available surprises.

### Rendering

The selection crosses to the platform as logical `(line, char)` positions, not
screen rows. `draw_scratchpad` already flattened wrapped lines into rows carrying
their logical line and character offsets, so a selection spanning a wrap comes
out right without either side knowing about the other. Checked with `--shot`,
including a deliberate across-the-wrap case.

**Linewise fills the row edge to edge**, decided by looking at it: highlighting
only as far as the text made `V` indistinguishable from `v` over the same
characters, and made an empty line in the range invisible.

`SELECT` is `0x2E3A4B`, dark enough to read unmodified `FG` text through. An
accent wash bright enough to notice made the glyphs on top of it worse.

The yank notice now says how much was copied rather than "scratchpad copied to
clipboard" — a yank whose range was not what you meant looks identical otherwise.

153 tests, clippy clean.

**Not built:** word motions. There is no `w`/`b`/`e`, so `dw` is not a thing.
Nothing depends on it; it is just absent, and `dw` will silently abort like any
other non-motion until it exists.

## AUTO-HIDE ON FOCUS LOST — DEFERRED, NOT REJECTED — 2026-08-16 (user)

The user, on the panel staying visible after focus leaves: "I don't personally
have a problem with it... I understand that this usage is not necessarily normal
for others, so we could make it a setting if we wanted to."

**Decided: not building it now.** Recorded so it is not re-argued from scratch.

The usual reason to build early does not apply. `wl_keyboard.leave` is a stable
Wayland event and the handler already exists and is already surface-matched — it
only prints under `DROPKICK_TRACE` today. This is not one of the retrofit-hostile
things like Alt-switching or the Esc ladder where deciding late means unpicking
something. It is the same fifteen lines whenever it is wanted.

Against building it: it would be a default-off branch that the only user never
runs, so it rots silently and nothing notices — the same shape of problem as a
settings file that swallows `timer.sond = false`.

The `timer.show_panel` precedent is real but different in kind. That panel was
already built and working when it was turned off by default, so the setting
preserved work rather than creating it. This would be new code for a
hypothetical person.

**If it is ever wanted**, the whole change is:

- `Dropkick::leave` in `dropkick-linux/src/main.rs` — already surface-matched,
  call the hide path there.
- one `key = value` bool in the config, parsed and tested in core, plus its line
  and comment in the default file written on first run.

**Checked, so it does not have to be checked again:** the alarm path cannot
auto-hide out from under you. `show_for_alarm` reuses the same layer surface but
maps it `KeyboardInteractivity::None`, and a surface that is never given the
keyboard never receives `enter`, so it never receives `leave` either.

**Open question if it is ever built**, and the thing that would decide whether it
is pleasant: whether transient surfaces — notification popups, OSD, menus —
produce spurious `leave`/`enter` pairs on cosmic-comp. If they do, the panel
would vanish for reasons invisible to the user. `DROPKICK_TRACE=1` already prints
every leave, so this is answerable by running the daemon in a terminal and using
the machine normally. Not measured yet.

## BUILT 2026-08-16 — Generate (passwords and UUIDs)

The fifth tool. Chosen because it needed no new platform capability, which held
up: **no dependency was added** — the randomness comes from `std::fs` reading
`/dev/urandom`. (The crate count in the handoff was not re-verified here;
`cargo-deny` is not installed on this machine, so `deny.toml` was not run.)

**Scope was cut deliberately, by the user: passwords and UUIDs now, hashes
later.** Hashes need a from-scratch SHA-256 in core, which is real but not risky
work; what made them wait is that "hash what?" is a product question. **Answered
and recorded so it need not be asked again: the input is the current clipboard
contents.**

### The trap here was the Clipboard, not the keymap

`set_clipboard` recorded *every* copy into clipboard history. A generated
password would therefore have landed in the `alt+w` list — flatly contradicting
the promise this tool makes, and doing it silently.

The mechanism to stop it already existed: `Clipboard::record` refuses anything
carrying `x-kde-passwordManagerHint`. So a generated secret is now offered with
that mime, which **both** keeps it out of our history and asks every other
clipboard manager on the system to skip it.

That is a **separate `Request` variant**, `SetClipboardSecret`, rather than a
boolean on the existing one. A flag can be forgotten at a call site; a variant
cannot be passed by accident. There is a test asserting the secret path never
emits a plain `SetClipboard`, because that single substitution is the whole
difference between this tool being safe and being theatre.

### Randomness

Enters as a parameter and is never read inside core, exactly as time does. The
platform fills a pool from `/dev/urandom` — the right source for keys on Linux,
no dependency, `std::fs` only. **If it cannot be read, nothing is generated**;
there is no weaker fallback, because a password generator that quietly degrades
is worse than one that fails.

The pool is topped up whenever it drops below 256 bytes, so a 128-character
password can never find itself half-generated and out of randomness. Requests
for a refill are raised only when Generate is the active tool — the first
version asked on every tool switch, which a Calculator test caught by finding an
`Entropy` request in its way.

**Rejection sampling, not modulo.** With 62 characters, `byte % 62` makes the
first eight ~1.6% likelier than the rest. Small, but it is a bias in a password
generator and costs four lines to remove. There is a test that feeds only the
bytes which must be rejected and proves they were skipped rather than folded
down.

### Keymap, and the `s` lesson applied

The handoff warned that `p`/`u`/`h` could each be a mode *and* a character
class. Splitting them by case and symbol keeps every key meaning one thing:

```
p u        password / UUID
a A 0 !    toggle lowercase, uppercase, digits, symbols
x          toggle "no lookalikes" (l I 1 O 0)
+ -        length
r space    regenerate
y enter    copy
```

The password-only keys return `Ignored` in UUID mode rather than doing something
approximate, and there is a test for it.

### Decided while building

- **Nothing survives a summon.** `on_enter` clears the value unconditionally,
  whichever tab the panel returns to. A password on screen from twenty minutes
  ago is a hazard, not a convenience.
- **Never show a short password.** Running out of randomness clears the value
  and says so. A truncated password that looks fine is the worst outcome
  available.
- **An empty panel always says why** — no classes selected, or everything
  excluded by `x`. Silence would read as a crash.
- **Settings are spelled out under the value** ("20 characters   a-z A-Z 0-9
  symbols") rather than shown as toggles, so the state of the thing on screen is
  readable without knowing the keymap.
- The symbol set omits quotes, backslashes and backticks. They are what shells,
  CSV and web input validation mangle.
- Config parses its first non-boolean value here. A `generate.length` that is
  not a number is **reported like an unknown key**, not silently defaulted —
  same rule and same reason as `timer.sond`.

173 tests, clippy clean. Checked with `--shot`; the sample seed is scattered
rather than `0,1,2,...`, because sequential bytes map to sequential alphabet
indices and produce `abcdefghij...`, which is correct and looks broken.

## deny.toml WAS NOT BEING RUN — 2026-08-16 (user)

`cargo-deny` was not installed on this machine. The file that mechanically
enforces "Dropkick has no networking code" had therefore never been executed
here — it was documentation wearing enforcement's clothes. Installed and run;
three things fell out.

**1. The `mio` ban had a hole.** The entry was
`{ name = "mio", wrappers = ["calloop"] }` — banned *unless* it arrived via
calloop, which needed it for its event loop. mio speaks TCP and UDP, so the
central claim had a carve-out for exactly the kind of crate the file exists to
keep out.

calloop 0.14 no longer uses mio and it is gone from the tree entirely, which is
what surfaced this: cargo-deny warned that the wrapper matched nothing. **The
exception is removed and the ban is now absolute.** Strictly stronger than
before, and the reason it was safe to tighten is that nothing depends on it any
more.

**2. `ttf-parser` is unmaintained** (RUSTSEC-2026-0192), via `fontdue`, which is
still on 0.9.4 and still depends on it. No upgrade resolves it; only replacing
the rasteriser would.

**Accepted knowingly by the user**, recorded in `deny.toml` with the reasoning
and with the conditions that should reopen it. The argument: it is a font
parser, fonts come from the OS, and an attacker who can write to font
directories is already past everything in the README's threat model.
"Unmaintained" is a maintenance notice, not a vulnerability — what it actually
means is that this one would not be fixed upstream if something were found.

Exceptions go in the `ignore` list with a reason. The check is never switched
off: a permanently red `cargo deny check` trains everyone to ignore it, which is
the exact failure this file exists to prevent. All four checks — advisories,
bans, licenses, sources — pass.

**3. Nothing ran this automatically — now something does.** A pre-commit hook
lives in `.githooks/pre-commit`, in the repository rather than in `.git/hooks`,
so it survives a clone and can be read before it is trusted. Enabled with
`git config core.hooksPath .githooks`.

Decisions in the hook, each of them a way it could have been useless:

- **It fails when `cargo-deny` is missing**, rather than skipping. A hook that
  quietly passes when its tool is absent reproduces exactly the state this whole
  entry is about.
- **It runs `--offline`.** A check that enforces "this program cannot use the
  network" must not itself require the network before you can commit, and
  commits have to work on a train. `bans`, `licenses` and `sources` only ever
  read `Cargo.lock`; only `advisories` wanted the database. The cost is that the
  database is as fresh as the last `cargo deny fetch`, which is written down in
  the hook.
- **It has to be turned on per clone.** Git does not enable a repository's own
  hooks, and should not — a repo that ran code on checkout would be a hazard.
  The consequence worth knowing: an unset `core.hooksPath` looks identical to
  everything passing.
- Measured: 0.27s offline, 0.57s online. Not enough to make anyone want to
  bypass it, which is the other way hooks die.

**And a fourth thing, found by running it.** The hook put five
`license-not-encountered` warnings in front of every commit — the allowlist
names licences that would be acceptable if they turned up, not only those
present. Left alone it would have trained the eye to skip the hook's output,
which is the failure this file exists to prevent. `unused-allowed-license =
"allow"` silences that class specifically, keeping the allowlist
forward-looking. This reverses a decision made an hour earlier to leave the
warnings be: harmless in a manual run, corrosive in a per-commit gate.

Verified rather than assumed: the clean path exits 0, a missing `cargo-deny`
exits 1 with instructions, and a banned crate makes cargo-deny exit 2 — which
the hook's `if !` catches.

A harmless leftover, deliberately not changed: `MPL-2.0` and `Unicode-3.0` are
allowed licenses that nothing in the tree currently uses. cargo-deny warns about
each. Removing them would silence the warning and cost a re-add the first time
such a crate legitimately appears, so they stay.

## LICENSING MADE EXPLICIT — 2026-08-18 (user)

`Cargo.toml` said `license = "MIT"` and there was no `LICENSE` file, and nothing
recorded what the 59 dependencies oblige. For a project whose README invites a
stranger to check its claims, both were conspicuous gaps.

Added: `LICENSE` (MIT, **copyright 2026 Brand Gamblin**) and
`THIRD-PARTY-NOTICES.md`, generated from `cargo metadata` so it can be rebuilt
rather than maintained by hand. The command is in the file's own header; a
notices file that drifts from the tree is worse than none.

### What the tree actually obliges

Read from the SPDX expressions, not the grouped summary — the grouping loses the
distinction that matters.

- **28 crates offer `MIT OR Apache-2.0`** (plus variants with Zlib or the LLVM
  exception). `OR` means the distributor elects. **Dropkick elects MIT**, and the
  original expression is preserved in the table so a redistributor can elect
  differently — Apache-2.0 would give them its express patent grant at the cost
  of its notice requirements.
- **Three cannot be elected away:** MIT (21 crates offer no alternative), Zlib
  (`foldhash`), and Unicode-3.0 (`unicode-ident` is
  `(MIT OR Apache-2.0) **AND** Unicode-3.0` — the `AND` means it applies *on top
  of* whichever is elected, not instead of one). That `AND` is the thing a
  glance at the licence list misses.
- **Nothing is copyleft.** No GPL, LGPL, AGPL or MPL, so distributing Dropkick
  carries no source-publication obligation and no share-alike on its own code.

Apache-2.0's text is deliberately **not** reproduced: every Apache crate here is
dual-licensed and MIT is elected, so its text is not in force. The notices file
says so and links it, rather than padding itself with 11kB nobody needs.

Counts in the file were checked against `cargo metadata` rather than asserted:
59 dependencies, 21 MIT-only, 1 Zlib-only.

**Not legal advice, and the log should not pretend otherwise** — this is a
reading of well-known licence terms, recorded so the reasoning is visible if it
ever needs revisiting by someone qualified.

## BUILT 2026-08-22 — Calendar, and the storage layer it forced

The sixth tool, and the first thing in Dropkick that writes a file.

### `now_ms` was never wall-clock time

Worth stating because it is not obvious from the name: `now_ms` is
`Instant::elapsed()` — milliseconds since the **daemon started**. Right for the
Timer, useless for a calendar. This needed a genuinely new time source rather
than a reuse of the existing one.

### Local time, and why it could not be skipped

`std::time` gives UTC. Measured on this machine while building this: it was
20:04 Friday locally and 00:04 **Saturday** in UTC. A UTC calendar would have
been showing the wrong day for four hours every evening.

**Decided by the user 2026-08-22, from three options put to them**: read
`/etc/localtime` directly rather than take a dependency or reach for `libc` and
an `unsafe` block. Same reasoning as the hand-written D-Bus client. The rejected
alternatives are worth recording: `libc::localtime_r` is ten lines and always
right but would have been the first `unsafe` in `dropkick-linux`, which has none;
a hand-typed offset in the config is trivial and silently wrong for half the year.

`tz.rs` parses TZif (RFC 8536) and reads the 64-bit block when the file is
version 2 or later — the 32-bit block cannot represent anything past 2038 and
exists only for old readers. Cross-checked against `date`: offset `-14400`,
matching `-0400`. Falls back to UTC if the file cannot be read, and says so on
stderr rather than being quietly a day out.

### Time entry: never guess

**The rule is the user's**, and it is the right one: look for `am`/`pm`, else a
colon means the 24-hour clock, else **ask** rather than pick.

One gap in it, found before building and agreed: `3:30pm` contains *both*, so
checking the colon first would silently produce 03:30 — the exact failure the
rule exists to prevent. `am`/`pm` is therefore checked first. Two refinements
folded in: `12am` is midnight and `12pm` is noon, and a bare **13–23 is not
ambiguous**, so the prompt appears less often than the rule implies. `930` is
refused rather than read as 9:30, because that reading is a guess.

### The file

```
2026-08-21 15:00 Dentist
2026-08-21 --:-- Bin day
```

Zero-padded ISO, so sorting the file as text sorts it chronologically and `sort`
cannot break it. **Lines that do not parse are kept, not dropped**, and written
back out untouched — a parse bug or a hand-edit typo must never cost somebody
their appointments. The count of unreadable lines is reported rather than
swallowed.

### Storage, implementing what was locked on 2026-08-15

`~/.local/share/dropkick/`, directory `0700`, files `0600`, written to a
temporary file and renamed into place so a crash mid-write cannot leave a
half-file. The temporary file is **created** `0600` rather than tightened
afterwards, which would leave a window where another account could open it, and
it lives beside the target because rename is only atomic within one filesystem.
`sync_all` before the rename, so the rename cannot land over contents that are
still only in the page cache.

Saves fire ~2s after the last edit, **and on hide, and on quit**. Closing the
panel is when the user considers themselves done, so waiting out the debounce
there would gamble with work they think is already put away. A failed write
leaves the dirty flag set so the next trigger retries.

### A real permissions bug, found by being asked a direct question

The user asked what permissions had to do with anything and whether something
was actually wrong. Checking rather than answering in the abstract: **yes**.
`~/.config/dropkick/` and its `config` file were `0775`/`0664` — world-readable,
from the default umask. Nothing secret is in there, but the project claims its
own files are private. `storage::tighten` now fixes both at startup.

The lesson is about the exchange, not the bug: the question was "is something
broken", and it was answered as though it had been "what does this word mean".
Check first.

### Decided while building

- **Re-summoning does not yank the view back to today** if you were browsing
  another month, but does follow the date if you were sitting on today.
- **The calendar is read once at startup**, not per summon: re-reading would
  discard edits still inside the debounce window.
- **Esc unwinds one step at a time** — the time prompt returns to the text, so a
  rejected time does not throw away what was typed.
- **`[` and `]` step months, not `H`/`L`.** `Alt+H`/`Alt+L` already cycle tabs,
  and plain `H` meaning something different is the sort of near-collision that
  trips fingers.
- Deleting uses the entry's identity, not its index: the list is filtered to one
  day, and an index into a filtered view addresses the wrong row in the backing
  vector. There is a test with entries on two days for exactly this.

233 tests, clippy clean. Checked with `--shot`, including the ambiguity prompt.

**Not built:** repeating entries, and any link to the Timer's notifications. An
entry with a time is inert — nothing will alarm.

## THREE BUGS FROM ONE SCREENSHOT — 2026-08-22 (user)

The user tried `25 feet in furlongs` in the Calculator and sent a photo of the
result. Three separate faults fell out of it, none of which any test could have
caught, because all three are about what ends up on screen.

**1. The error message was drawn off the panel.** `draw_calculator` put the
error at `input_baseline + lh`, and the input line already sits at the bottom of
the content area — so the message landed under the hint row and was clipped by
the panel edge. What the user saw was two red pixels and no text.

Fixed by reserving a line: with an error present the input moves up by one line
height and the error takes the space below it. The arithmetic is now in
`calculator_lines`, split out from the drawing **so it can be tested** — the
whole class of bug here is invisible to tests that only inspect state, and three
now pin it.

**2. The README promised something that does not exist.** It described the
Calculator as "expressions, units, date maths". There are no units and no date
maths; `docs/HANDOFF.md` had it right all along as "recursive-descent evaluator,
functions, `ans`". So this was not a user poking at nonsense — **the
documentation told them it would work.** Corrected to "arithmetic, functions,
`ans`".

**3. The first screenshot in the README was a picture of the wrong tool.**
`docs/img/calculator.png`, captioned "Calculator", showed an empty Scratchpad.

The `--shot` scene for the calculator never switched tabs; it relied on the
Calculator being the tool `App::new()` starts in. Commit `48e9547` ("Reorder the
roster and alias Ctrl+C to Esc") changed that default to the Scratchpad, and the
scene silently began rendering a different tool. The image was regenerated and
committed afterwards without anyone looking at it.

Every shot scene now switches tabs explicitly. All six committed images were
regenerated from the current build — they were stale as well, still showing the
hint row from before `dd` and visual selection existed.

**The pattern worth keeping.** `--shot` exists so the UI gets reviewed by looking
at it, and the handoff says so. All three of these survived because the output
was generated and not examined. Generating an image is not review; opening it is.

## UNITS AND DATE MATHS — 2026-08-22 (user)

Asked for after the README was found promising them. Rather than delete the
claim, make it true.

### Where they sit

**In front of the evaluator, not inside it.** The recursive-descent parser deals
in `f64` and knows nothing about metres or Tuesdays. Giving it units would mean
every value carrying a dimension through every operator, for a feature used on
a minority of lines. Instead `evaluate_line` dispatches on the shape of the
input: conversion, then date, then plain arithmetic. **`2+2` takes exactly the
path it always took**, and there is a test that says so, because that was the
whole risk of this change.

`eval` still returns `f64`. The new `Evaluated { display, ans }` carries what to
show and what `ans` should become — `None` for a date, since there is no sensible
number for one and quietly setting `ans` to a day count would make the next
`ans` mean something the user never saw.

### Traps that were designed around rather than discovered

- **Temperature is not a factor.** It has an offset, and a factor-only table
  reports 0 °C as 0 °F. Handled separately; tested at 0, 100, 98.6 and -40.
- **An ISO date is three numbers and two minus signs** to an arithmetic parser,
  which would quietly evaluate `2026-08-22` to 1996. The date path runs first,
  and splits only on a `+`/`-` that has a space before it.
- **`in` is both the keyword and a unit.** `25 in in cm` is 25 inches. The split
  takes the **last** ` in `, so the abbreviation cannot be eaten as the keyword.
- **`gal` is the US gallon**, 20% off the imperial one; `impgal` is separate.
  **`kb` is 1000 bytes**; `kib` is 1024. Both are cases where one name for two
  meanings would give a confident wrong answer.
- **There is no `month` or `year` unit.** Neither is a fixed number of seconds.
  They exist only in date arithmetic, where a calendar makes them well-defined:
  `2026-01-31 + 1 month` is 28 February, via the clamping already in
  `Date::add_months`.

### Found while testing

A bare unit on the left — `ft in cm`, meaning one foot — had no separator to
split on, so it fell through to the arithmetic parser and died on `unknown name
ft`. Fixed by checking whether the whole left side is a unit before trying to
split it.

Two test failures were **my expectations being wrong, not the code**: `1 ft in
in` is 12.000000000000002 because 0.3048/0.0254 has no exact binary form (the
display rounds; `ans` keeps the real value), and the arithmetic parser says
`unknown name` rather than `unexpected` for a bare word.

`today`, `tomorrow` and `yesterday` come from the platform on summon, like the
Calendar's. Before the platform supplies one they **fail rather than resolve**,
which is better than inventing a day.

268 tests, clippy clean. The README now describes what exists, and its
screenshot shows it.

## BUILT 2026-08-22 — Colour, and D-Bus signals at last

The seventh tool. **All seven roster slots are now built**; three keys
(`alt+i/o/p`) stay deliberately unspent.

### The eyedropper needed the thing that had been missing all along

`PickColor` was checked before anything was planned, because the last portal
this project wanted — `GlobalShortcuts` — turned out not to exist on this
machine. This one does: `org.freedesktop.portal.Screenshot`, version 2.

It does not return a colour. It returns an object path for a pending Request,
and the answer arrives later as a `Response` **signal** on that path. The
hand-written D-Bus client had no signal support, which is precisely why the
Color tool was last. Added:

- `MSG_SIGNAL`, and the member name threaded out of the header parser so a
  signal can be recognised at all.
- The unique bus name, captured from the `Hello` reply and previously thrown
  away. The portal builds the Request path out of it, and **the match rule has
  to be installed before the call** — installing it afterwards is a race the
  user loses by picking quickly.
- A narrow decoder for `(u response, a{sv})`, finding the `color` key and the
  three doubles after it. Deliberately not a general variant decoder: that is a
  lot of surface for one message shape.

**It runs on a thread.** `pick_color` blocks until the user clicks something,
which might be never. Inline, that would freeze the panel with no way out — not
even Esc, because the event loop would not be reading keys. The thread posts
back over a `calloop::channel`, the same shape `ipc::spawn_listener` already
uses, so no new machinery and still no async runtime. The panel hides itself
first, since the portal is about to ask for a pixel that Dropkick is covering.

Cancelling reports `Ok(None)` and is **not** an error: leaving `picking` set
would strand the panel saying "pick a colour" forever. There is a test.

### The tool

Free text like the Calculator, so every printable key belongs to the input —
which is why the eyedropper is **Tab** and copying is **Ctrl+Y**. `t` has to be
able to type a `t`, and there is a test asserting exactly that.

Accepts `#6FB3F2`, `#abc`, bare hex, `rgb(...)`, `hsl(...)`, and the CSS colour
names. Shows hex, rgb, hsl and the name — or the nearest name, labelled as
nearest, when it is not exact. Out-of-range input like `rgb(300, 0, 0)` is
**refused rather than clamped**: clamping would turn a mistake into a confident
red.

No alpha. This answers "what is that colour in hex", and a channel that silently
round-trips through HSL and back is a way to lose information.

### Found by looking at it

Two rendering faults, both invisible to the tests:

1. **The hex label overflowed the swatch.** Seven characters at 58px are wider
   than 220px, so it ran off the left edge of the panel. The swatch is now 300
   wide and the label drops to the small font if it still will not fit.
2. **White text on tomato.** The threshold for black-vs-white on the swatch was
   a guessed 0.35. The real crossover is where the two WCAG contrasts are equal:
   `sqrt(1.05 * 0.05) - 0.05`, about **0.179**. Tomato is 0.309, so black is
   right — 7:1 rather than 3:1. Now derived in `prefers_dark_text`, with the
   derivation written out and a test that checks the two contrasts agree at the
   crossover.

### And a decoder bug the tests caught

The scan for the doubles used an exclusive upper bound, so it missed the last
valid offset — which is the common case, since the doubles are the final thing
in the body. Every real response would have failed to decode.

298 tests, clippy clean.

### MEASURED 2026-08-23: COSMIC's portal refuses PickColor

It was verified, and it does not work — **for reasons outside Dropkick**. Traced
on the bus, with `dbus-monitor` watching the whole exchange:

```
-> PickColor("", {handle_token: "dropkick_pick"})
-> impl.portal.Screenshot.PickColor(/…/request/1_1581/dropkick_pick, …)
<- Request.Response  uint32 2   (1.3 ms later, no UI shown)
```

Response code 2 is "ended for other reasons" — not a cancellation, which is 1.
No picker appeared, and `xdg-desktop-portal-cosmic` logged nothing about it.
Reproduced from `gdbus`, an entirely unrelated client, with the same result, so
it is not something about how Dropkick asks.

**What this proves about our side:** the plumbing is right. The portal built
exactly the Request path the match rule predicted, the signal was received, and
the 8-byte body decoded correctly into response code 2. Every part of the new
signal support works; the desktop simply declines.

The backend declares the method and carries a `PickColorResult` type with a
`color` field, so it is wired up at the interface level and errors somewhere
inside. Version `0.1.0pop1~1785249581~24.04~f211aa3`.

**This is the GlobalShortcuts lesson again, one layer deeper.** That interface
was absent and could be detected by introspection. This one is present,
introspectable, version 2, with a backend that claims it — and still does not
work. Introspection is not evidence of function; only calling it is.

**Switched off, not deleted** (user, 2026-08-23). `ColorTool::new` sets
`eyedropper: false`, so `Tab` explains itself instead of hiding the panel and
waiting on a refusal. Everything behind the flag is intact and still tested; the
tests that exercise the picking path set it true. Flipping that one field is the
whole of turning it back on.

`Tab` says why rather than doing nothing. A key that is documented and then
silently inert is worse than one that admits it cannot work.

**A native alternative exists if the eyedropper is wanted for real.** cosmic-comp
exports `ext_image_copy_capture_manager_v1`, and Dropkick already speaks
Wayland. Capturing the screen and reading the pixel under the cursor would need
no D-Bus and no dependency — the same shape as hand-rolling the D-Bus client
rather than taking one. It is real work: the capture protocol, a full-screen
picker surface, and cursor handling. Not started.

## A BORDER — 2026-08-23 (user)

"The window doesn't blend with similarly-colored nearby windows."

Dropkick is a layer surface: no decoration, no title bar, no shadow. Against a
window of a similar colour its edge simply is not there, and the panel reads as
part of whatever is behind it.

A one-pixel hairline in `0x5A6B82`, drawn **last** so nothing paints over it —
the tab bar and hint row both run to the panel edge.

Checked against the worst case rather than a convenient one: the panel
composited onto a background of `0x14171C`, which is its own background colour
exactly. If the edge shows there it shows anywhere. It does.

`BORDER` and `BORDER_W` are constants, so the weight and colour are one line
each to change.

**Rounded corners are possible but not done.** The buffer is `Argb8888` and no
opaque region is declared, so the compositor would honour transparent corner
pixels. `Canvas::set` forces alpha to `0xFF`, so it would need a way to write a
transparent pixel first.

## NEXT

The interaction grammar is settled, so: the tab bar plus two tools, far enough
to exercise the parts that are hard to retrofit — Alt-switching between them,
state preserved on both sides of the switch, the Esc ladder, and undo. Calculator
and Notepad are the right pair, because Notepad is the tool that disqualified
Shift and will stress the mode boundary.

Clipboard history stays last. It is the security-heavy one, it is the tool that
needs masked resume, and it benefits from every other decision being settled
first.
