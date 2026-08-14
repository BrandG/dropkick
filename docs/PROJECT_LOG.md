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
  actually demands one. Dependency tree today: 53 crates, none network-capable.

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

- Root interaction grammar: strict vi-modal (`c` = calculator) vs type-ahead
  with sigils (bare digits → calculator). The plan's own example,
  `Super+Space c 437*1.18`, has a ceremonial `c`. Decide before the second
  utility exists.
- Password-manager hint (`x-kde-passwordManagerHint: secret`) skip policy is
  designed but **untested** — needs a real password-manager copy to confirm.
- Accessibility: custom-drawn toolkits (Iced/libcosmic) have limited AT-SPI
  support. A manifesto about respecting users should say so out loud.
- Licence not yet chosen (workspace currently declares MIT).
- On-disk format stability and data portability unspecified.

## NEXT

Interaction grammar decision, then the first utility (calculator) end-to-end to
prove the grammar before four more are built on it. Clipboard history last: it
is the security-heavy one and benefits from everything else being settled.
