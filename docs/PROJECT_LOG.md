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

## NEXT

The interaction grammar is settled, so: the tab bar plus two tools, far enough
to exercise the parts that are hard to retrofit — Alt-switching between them,
state preserved on both sides of the switch, the Esc ladder, and undo. Calculator
and Notepad are the right pair, because Notepad is the tool that disqualified
Shift and will stress the mode boundary.

Clipboard history stays last. It is the security-heavy one, it is the tool that
needs masked resume, and it benefits from every other decision being settled
first.
