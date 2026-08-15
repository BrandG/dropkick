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
- **No SQLite, no ORM, no async runtime, no logging framework** until something
  actually demands one. The dependency tree is audited on every change; the
  current count is recorded in the BUILT sections below.

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

- **Keys bind to tools, never to tab positions.** If Alt+Q means "first tab",
  inserting a tool later shifts every binding and destroys months of muscle
  memory in one release. Each tool gets a permanent key; tab order follows key
  order; new tools take T, Y, U…; a retired tool's key stays retired.
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
- Does resume survive a daemon restart or reboot, or only sleep between
  summons? In-memory resume is free; on-disk resume is a per-tool retention
  decision and interacts with the clipboard retention tiers. **Raised by real
  use 2026-08-15**: the Scratchpad has no way to save, which is this question
  wearing a different hat. Calendar events force it too — they are the first
  data that is meaningless unless it persists.
- D-Bus client choice (see PORTAL FINDINGS): needed for both notifications and
  the eyedropper, and the obvious crate conflicts with "no async runtime".
- Whether `Alt+N`/`Alt+P` are worth one permanent tool slot.
- Final key allocation and the Notepad -> Scratchpad rename. Cheap now,
  permanent later; nothing has muscle memory yet.
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
mainstream Rust D-Bus crate (`zbus`) brings an async runtime, which collides
with the locked "no async framework" decision. Options: the `dbus` crate
(libdbus bindings, no async), a hand-rolled minimal client for the two or three
calls actually needed, or relaxing the rule. Not yet decided.

Also note this makes the timer the **first tool that acts while Dropkick is
hidden**. Every other tool is inert when dismissed. That changes the daemon from
"a window that appears" into "a process that does things while you are not
looking", which is a real change in what Dropkick is, and deserves to be a
deliberate yes rather than a side effect of wanting a timer.

## NEXT

The interaction grammar is settled, so: the tab bar plus two tools, far enough
to exercise the parts that are hard to retrofit — Alt-switching between them,
state preserved on both sides of the switch, the Esc ladder, and undo. Calculator
and Notepad are the right pair, because Notepad is the tool that disqualified
Shift and will stress the mode boundary.

Clipboard history stays last. It is the security-heavy one, it is the tool that
needs masked resume, and it benefits from every other decision being settled
first.
