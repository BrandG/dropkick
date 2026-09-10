//! dropkickd — the resident daemon, and the latency probe that justifies it.
//!
//! Dropkick is not a fast-launching application. It is a process that is
//! already running with a layer surface already created, and the hotkey only
//! maps it. That distinction is the whole product: a cold start pays for a
//! Wayland connection, a GPU/shm setup and a font load, none of which can be
//! made to fit in a keystroke's worth of time.
//!
//! What this probe measures, per summon:
//!   poke -> configure   compositor answered the map request
//!   poke -> committed   we attached a buffer and committed
//!   poke -> presented   compositor released our frame callback (visible)
//!   poke -> FOCUS       wl_keyboard.enter, i.e. keystrokes now reach us
//!
//! FOCUS is the number that matters, and it is worth being precise about why.
//! On Wayland a client cannot buffer input it does not have focus for: before
//! wl_keyboard.enter arrives, keystrokes are delivered to whatever the user was
//! typing into. "Accepts keystrokes before it finishes painting" therefore is
//! not implementable as buffering; it reduces to "acquire focus faster than the
//! human's next keypress". Presentation can lag focus without dropping input.
//!
//! Note the protocol constraint driving the design: per wlr-layer-shell, a
//! surface unmapped with a null buffer "returns to the state it had right after
//! layer_shell.get_layer_surface", and re-mapping means commit-with-no-buffer,
//! wait for a configure, then attach. So every summon costs one compositor
//! round trip, and size/anchor/interactivity must be re-applied each time.

use dropkick_core::app::{App, Request, Response};
use dropkick_core::key::{Key as DkKey, KeyPress, Mods};
use dropkick_linux::font::{Canvas, TextRenderer};
use dropkick_linux::render::{self, HEIGHT, SHADOW, SURFACE_H, SURFACE_W, WIDTH};
use dropkick_linux::ipc;
use dropkick_linux::{storage, tz};

use std::collections::HashMap;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::{
    channel, generic::Generic, timer::TimeoutAction, timer::Timer, EventLoop, Interest, LoopHandle,
    Mode as CalloopMode, PostAction, RegistrationToken,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    backend::ObjectId,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use smithay_client_toolkit::reexports::protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

/// How long the panel stays up in bench mode before being dismissed.
const VISIBLE_MS: u64 = 90;
/// Gap between bench cycles, so the compositor is not measured mid-animation.
const GAP_MS: u64 = 150;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Hidden,
    Mapping,
    Visible,
}

#[derive(Debug)]
struct Cycle {
    t0: Instant,
    configure: Option<Duration>,
    committed: Option<Duration>,
    presented: Option<Duration>,
    focused: Option<Duration>,
}

impl Cycle {
    fn new() -> Self {
        Cycle {
            t0: Instant::now(),
            configure: None,
            committed: None,
            presented: None,
            focused: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strategy {
    /// Spec path: unmap with a null buffer, re-map by committing without one.
    Remap,
    /// Destroy the layer surface on hide, build a fresh one on show.
    Recreate,
}

/// What the eyedropper thread sends back: a colour, a cancellation, or why it
/// could not ask. Named because the raw type is unreadable inline.
type Picked = Result<dropkick_linux::dbus::PickResult, String>;

struct Dropkick {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    compositor: CompositorState,
    layer_shell: LayerShell,
    layer: Option<LayerSurface>,
    qh: QueueHandle<Dropkick>,
    strategy: Strategy,
    cycle_id: u64,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    handle: LoopHandle<'static, Dropkick>,

    app: App,
    settings: dropkick_linux::settings::Settings,
    text: TextRenderer,
    big: TextRenderer,
    /// Live while the timer runs, dropped when it stops.
    tick_token: Option<RegistrationToken>,
    /// The pending debounced calendar write, if one is armed.
    calendar_token: Option<RegistrationToken>,
    /// The pending debounced scratchpad write.
    scratchpad_token: Option<RegistrationToken>,
    /// What was last written to disk.
    ///
    /// The scratchpad is saved by comparing against this rather than by a dirty
    /// flag set at each mutation site. There are a dozen ways to change the
    /// buffer -- insert, backspace, `x`, `dd`, an operator over a motion, a
    /// visual delete, undo -- and a flag missed at any one of them loses work
    /// silently. Comparing the text cannot miss one.
    saved_scratchpad: String,
    /// Where the eyedropper thread posts its answer.
    picked: channel::Sender<Picked>,
    /// True between asking for exclusive focus and actually getting it.
    grabbed_exclusive: bool,
    mods: Mods,
    started: Instant,

    conn: Connection,
    /// wlr-data-control: the protocol that lets a client with no surface and no
    /// focus observe the clipboard. It is how every clipboard manager works,
    /// and it is ungated - see the threat model in the README.
    data_device: Option<ZwlrDataControlDeviceV1>,
    data_manager: Option<ZwlrDataControlManagerV1>,
    /// MIME types announced for each pending offer, keyed by object.
    offers: HashMap<ObjectId, Vec<String>>,
    /// Our own source, alive for as long as we own the selection.
    source: Option<ZwlrDataControlSourceV1>,
    source_text: String,
    /// Set when we take ownership of the selection, so the selection event that
    /// comes straight back is not re-recorded as a fresh copy - and, more
    /// importantly, so we never ask to read from our own pipe and deadlock.
    skip_next_selection: bool,

    width: u32,
    height: u32,
    phase: Phase,
    cycle: Option<Cycle>,
    done: Vec<Cycle>,
    bench_left: u32,
    bench: bool,
    exit: bool,
}

impl Dropkick {
    fn surface_matches(&self, surface: &wl_surface::WlSurface) -> bool {
        self.layer.as_ref().map(|l| l.wl_surface() == surface).unwrap_or(false)
    }

    /// Map the surface. Layer state resets on unmap, so it is re-applied here
    /// rather than once at startup - that is a protocol requirement, not
    /// defensiveness.
    fn show(&mut self) {
        self.show_with(KeyboardInteractivity::Exclusive);
    }

    /// Show the panel because a timer went off, rather than because the user
    /// asked.
    ///
    /// `None`, not `OnDemand`. Measured with `OnDemand`, Dropkick never
    /// received `wl_keyboard.enter` — and the user's focused window still lost
    /// focus when the overlay mapped. That combination is worse than taking the
    /// keyboard outright: keystrokes go nowhere instead of somewhere
    /// recoverable. `None` states outright that this surface must never be
    /// given the keyboard, which is the only honest request for an alert the
    /// user did not ask for. Dismiss it with the summon key.
    fn show_for_alarm(&mut self) {
        self.app.active = dropkick_core::app::ToolId::Timer;
        self.show_with(KeyboardInteractivity::None);
    }

    fn show_with(&mut self, interactivity: KeyboardInteractivity) {
        if self.phase != Phase::Hidden {
            return;
        }
        // Pick up an edited settings file. A stat, and a read only when the
        // file actually moved.
        if self.settings.reload_if_changed() {
            // Say so in the panel, not just on stderr: launched from a
            // keybinding there is no terminal to print to, and a setting that
            // applies silently is indistinguishable from one that did not.
            self.app.notice = Some("settings reloaded".to_string());
        }
        // If the file was topped up with settings it predated, say which. This
        // is the only way the user finds out without opening the file, and the
        // point of appending was that they should not have to be told twice.
        let added = self.settings.take_added();
        if !added.is_empty() {
            self.app.notice = Some(format!(
                "{} new setting(s) added to your config: {}",
                added.len(),
                added.join(", ")
            ));
        }
        // The daemon can be up for days, so the calendar's idea of "today" has
        // to be refreshed rather than fixed at startup. set_today only moves
        // the focus if it was already sitting on today.
        let (today, _) = tz::today_local();
        self.app.calendar.set_today(today);
        // The Calculator needs it too, for `today + 90 days`.
        self.app.calculator.today = Some(today);

        // Push the generator's settings across every summon, not only when the
        // file changed: it is the same "the moment the user is present" hook,
        // and it costs a struct copy.
        let c = &self.settings.config;
        self.app.generate.configure(
            c.generate_length,
            dropkick_core::generate::Classes {
                lower: c.generate_lower,
                upper: c.generate_upper,
                digits: c.generate_digits,
                symbols: c.generate_symbols,
            },
            c.generate_exclude_ambiguous,
        );

        self.cycle_id += 1;
        let id = self.cycle_id;
        self.cycle = Some(Cycle::new());
        // Resume must not mean "put the last screenful back on screen".
        self.app.on_summon();
        // A tick that was running at one second needs to speed up now the
        // readout is visible.
        self.ensure_ticking();

        if self.strategy == Strategy::Recreate || self.layer.is_none() {
            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("dropkick"),
                None,
            );
            self.layer = Some(layer);
        }

        if let Some(layer) = self.layer.as_ref() {
            layer.set_size(SURFACE_W, SURFACE_H);
            layer.set_anchor(Anchor::TOP);

            // The surface is bigger than the panel now, to make room for the
            // shadow. Without this, that extra band would swallow clicks meant
            // for whatever is behind it -- a translucent gutter that eats the
            // pointer is worse than having no shadow at all.
            //
            // The region is the panel rectangle only. Corners are left square
            // here: a pointer landing in the ten pixels a rounded corner cuts
            // away is not worth a second region, and the alternative is
            // clicks passing through a part of the panel that looks solid.
            match Region::new(&self.compositor) {
                Ok(region) => {
                    region.add(SHADOW as i32, 0, WIDTH as i32, HEIGHT as i32);
                    layer.wl_surface().set_input_region(Some(region.wl_region()));
                }
                Err(e) => eprintln!("dropkickd: no input region, the shadow will eat clicks: {e}"),
            }
            layer.set_keyboard_interactivity(interactivity);
            layer.commit();
        }
        self.phase = Phase::Mapping;
        self.grabbed_exclusive = interactivity == KeyboardInteractivity::Exclusive;

        // A summon that never configures must report, not hang.
        //
        // What counts as "stalled" depends on what was asked for. A panel the
        // user summoned should have the keyboard within a millisecond, so no
        // focus means something went wrong. An alarm panel deliberately asks
        // for *no* keyboard, so waiting for focus would condemn every one of
        // them -- which is exactly what happened: the alarm panel appeared and
        // was killed 1.2s later by its own watchdog.
        //
        // An open cycle is not evidence either way: it is the normal state of a
        // panel someone is typing into.
        let expects_focus = interactivity == KeyboardInteractivity::Exclusive;
        let h = self.handle.clone();
        let _ = h.insert_source(
            Timer::from_duration(Duration::from_millis(1200)),
            move |_, _, state: &mut Dropkick| {
                let stalled = state.cycle_id == id
                    && state.cycle.as_ref().is_some_and(|c| {
                        if expects_focus {
                            c.focused.is_none()
                        } else {
                            c.configure.is_none()
                        }
                    });
                if stalled {
                    eprintln!("  [watchdog] summon {id} never completed (phase {:?})", state.phase);
                    state.hide();
                }
                TimeoutAction::Drop
            },
        );
    }

    fn hide(&mut self) {
        if self.phase == Phase::Hidden {
            return;
        }
        // One of the three save triggers. Closing the panel is the moment the
        // user considers themselves done, so waiting out the debounce would be
        // gambling with work they think is already put away.
        self.save_calendar_now();
        self.save_scratchpad_now();
        match self.strategy {
            Strategy::Recreate => {
                // Dropping destroys zwlr_layer_surface_v1 and its wl_surface.
                self.layer = None;
            }
            Strategy::Remap => {
                if let Some(layer) = self.layer.as_ref() {
                    layer.wl_surface().attach(None, 0, 0);
                    layer.commit();
                }
            }
        }
        self.phase = Phase::Hidden;

        if let Some(c) = self.cycle.take() {
            self.report(&c);
            self.done.push(c);
        }

        if self.bench {
            if self.bench_left > 0 {
                self.bench_left -= 1;
                let h = self.handle.clone();
                let _ = h.insert_source(
                    Timer::from_duration(Duration::from_millis(GAP_MS)),
                    |_, _, state: &mut Dropkick| {
                        state.show();
                        TimeoutAction::Drop
                    },
                );
            } else {
                self.finish();
            }
        }
    }

    fn toggle(&mut self) {
        if self.phase == Phase::Hidden {
            self.show();
        } else {
            self.hide();
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let (width, height) = (self.width, self.height);
        let stride = width as i32 * 4;
        let (buffer, pixels) = self
            .pool
            .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
            .expect("create buffer");

        let mut canvas = Canvas::new(pixels, width as i32, height as i32);
        let now = self.started.elapsed().as_millis() as u64;
        let opacity = (self.settings.config.panel_opacity.clamp(20, 100) * 255 / 100) as u8;
        render::draw_surface(&self.app, &mut self.text, &mut self.big, now, &mut canvas, opacity);

        let Some(layer) = self.layer.as_ref() else { return };
        layer.wl_surface().damage_buffer(0, 0, width as i32, height as i32);
        layer.wl_surface().frame(qh, FrameCallbackData(layer.wl_surface().clone()));
        buffer.attach_to(layer.wl_surface()).expect("buffer attach");
        layer.commit();

        if let Some(c) = self.cycle.as_mut() {
            if c.committed.is_none() {
                c.committed = Some(c.t0.elapsed());
            }
        }
    }

    fn report(&self, c: &Cycle) {
        let ms = |d: Option<Duration>| match d {
            Some(d) => format!("{:7.2}", d.as_secs_f64() * 1000.0),
            None => "      -".to_string(),
        };
        println!(
            "  configure {}  committed {}  presented {}  FOCUS {}  (ms)",
            ms(c.configure),
            ms(c.committed),
            ms(c.presented),
            ms(c.focused)
        );
    }

    /// Bench mode drives itself: show, wait for focus, hold, hide, repeat.
    fn schedule_bench_hide(&mut self) {
        let handle = self.handle.clone();
        let _ = handle.insert_source(
            Timer::from_duration(Duration::from_millis(VISIBLE_MS)),
            |_, _, state: &mut Dropkick| {
                state.hide();
                TimeoutAction::Drop
            },
        );
    }

    fn init_clipboard(&mut self, qh: &QueueHandle<Self>) {
        let manager = match self
            .registry_state
            .bind_one::<ZwlrDataControlManagerV1, _, _>(qh, 1..=2, ())
        {
            Ok(m) => m,
            Err(e) => {
                eprintln!("dropkickd: no clipboard capture ({e})");
                return;
            }
        };
        let Some(seat) = self.seat_state.seats().next() else {
            eprintln!("dropkickd: no seat, clipboard capture disabled");
            return;
        };
        self.data_device = Some(manager.get_data_device(&seat, qh, ()));
        self.data_manager = Some(manager);
    }

    /// Pull the text out of an offer without blocking the event loop: hand the
    /// compositor a pipe, then read the far end as an event source.
    fn receive_offer(&mut self, offer: &ZwlrDataControlOfferV1, mimes: Vec<String>) {
        const PREFERRED: &[&str] = &[
            "text/plain;charset=utf-8",
            "UTF8_STRING",
            "text/plain",
            "STRING",
            "TEXT",
        ];
        let Some(mime) = PREFERRED
            .iter()
            .find(|m| mimes.iter().any(|o| o == *m))
            .map(|m| m.to_string())
        else {
            return; // not text; images and files are not this tool's business
        };

        let Ok((reader, writer)) = std::io::pipe() else { return };
        offer.receive(mime, writer.as_fd());
        drop(writer);
        // The request must reach the compositor before anything will arrive.
        let _ = self.conn.flush();

        let mut buf: Vec<u8> = Vec::new();
        let mimes_for_record = mimes;
        let source = Generic::new(reader, Interest::READ, CalloopMode::Level);
        let _ = self.handle.insert_source(source, move |_, file, state: &mut Dropkick| {
            let mut chunk = [0u8; 8192];
            // calloop lends the source out by shared reference; read through
            // the `&PipeReader` impl rather than needing &mut.
            let mut pipe: &std::io::PipeReader = file;
            // Safety net: a hostile or broken source could stream forever.
            const LIMIT: usize = 4 * 1024 * 1024;
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        state.finish_capture(&buf, &mimes_for_record);
                        return Ok(PostAction::Remove);
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > LIMIT {
                            return Ok(PostAction::Remove);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        return Ok(PostAction::Continue)
                    }
                    Err(_) => return Ok(PostAction::Remove),
                }
            }
        });
    }

    fn finish_capture(&mut self, bytes: &[u8], mimes: &[String]) {
        let Ok(text) = std::str::from_utf8(bytes) else { return };
        let kept = self.app.clipboard.record(text, mimes);
        if std::env::var_os("DROPKICK_DEBUG").is_some() {
            // Length and type list only. Never the content: a debug flag that
            // prints clipboard text into a terminal scrollback would undo the
            // point of the tool.
            eprintln!(
                "  [capture] {} bytes, {} mime types, kept={kept}",
                bytes.len(),
                mimes.len()
            );
        }
        if kept && self.phase == Phase::Visible {
            let qh = self.qh.clone();
            self.draw(&qh);
        }
    }

    /// Take ownership of the system selection so other applications can paste
    /// what the user picked.
    /// `secret` marks a generated password: it is offered with the password
    /// manager hint so other clipboard managers skip it, and it is deliberately
    /// not recorded in our own history.
    fn set_clipboard(&mut self, text: String, secret: bool, qh: &QueueHandle<Self>) {
        let (Some(manager), Some(device)) = (self.data_manager.as_ref(), self.data_device.as_ref())
        else {
            return;
        };
        let source = manager.create_data_source(qh, ());
        for mime in [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "STRING",
            "TEXT",
        ] {
            source.offer(mime.to_string());
        }
        if secret {
            source.offer(dropkick_core::clipboard::PASSWORD_HINT.to_string());
        }
        device.set_selection(Some(&source));
        // Record it here rather than reading our own selection back: the
        // self-capture guard suppresses that event, so without this a yank
        // would reach the system clipboard but never appear in the history.
        //
        // Except for a secret, which is the entire point: Generate promises
        // nothing is stored, and the Clipboard tab is two keys away.
        if !secret {
            self.app.clipboard.record(&text, &[]);
        }
        self.source_text = text;
        self.source = Some(source);
        self.skip_next_selection = true;
        let _ = self.conn.flush();
    }

    /// Load the calendar from disk, and tell the user today's date.
    ///
    /// Done once at startup rather than per summon: the file is ours, nothing
    /// else writes it, and re-reading on every summon would undo unsaved edits
    /// still sitting inside the debounce window.
    fn load_calendar(&mut self) {
        let (today, from_file) = tz::today_local();
        self.app.calendar.set_today(today);
        self.app.calculator.today = Some(today);
        if !from_file {
            eprintln!("dropkickd: could not read /etc/localtime; dates are UTC and may be a day out");
        }
        let Some(path) = storage::calendar_path() else { return };
        if let Some(text) = storage::read(&path) {
            let bad = self.app.calendar.load(&text);
            if bad > 0 {
                // Say so rather than swallowing it: those lines are being kept,
                // not understood, and the user is the only one who can fix them.
                eprintln!("dropkickd: {bad} calendar line(s) not understood; kept as-is");
                self.app.notice = Some(format!("{bad} calendar line(s) not understood"));
            }
        }
    }

    /// Read the scratchpad off disk at startup.
    fn load_scratchpad(&mut self) {
        if !self.settings.config.scratchpad_persist {
            return;
        }
        let Some(path) = storage::scratchpad_path() else { return };
        if let Some(text) = storage::read(&path) {
            self.app.scratchpad.load(&text);
            self.saved_scratchpad = self.app.scratchpad.text();
            println!("dropkickd: restored {} bytes of scratchpad", text.len());
        }
    }

    /// Write the scratchpad now, if it differs from what is on disk.
    fn save_scratchpad_now(&mut self) {
        if !self.settings.config.scratchpad_persist {
            return;
        }
        let current = self.app.scratchpad.text();
        if current == self.saved_scratchpad {
            return;
        }
        let Some(path) = storage::scratchpad_path() else { return };
        match storage::write_atomic(&path, &current) {
            Ok(()) => self.saved_scratchpad = current,
            // Leave `saved_scratchpad` alone so the next trigger tries again.
            Err(e) => eprintln!("dropkickd: could not save the scratchpad: {e}"),
        }
        self.scratchpad_token = None;
    }

    /// Save ~2s after the last edit, like the calendar.
    fn schedule_scratchpad_save(&mut self) {
        if let Some(tok) = self.scratchpad_token.take() {
            self.handle.remove(tok);
        }
        self.scratchpad_token = self
            .handle
            .insert_source(
                Timer::from_duration(Duration::from_secs(2)),
                |_, _, state: &mut Dropkick| {
                    state.save_scratchpad_now();
                    TimeoutAction::Drop
                },
            )
            .ok();
    }

    /// Write the calendar now, if it has unsaved changes.
    fn save_calendar_now(&mut self) {
        if !self.app.calendar.dirty {
            return;
        }
        let Some(path) = storage::calendar_path() else { return };
        match storage::write_atomic(&path, &self.app.calendar.serialise()) {
            Ok(()) => self.app.calendar.dirty = false,
            // Keep `dirty` set so the next trigger tries again rather than
            // dropping the change on the floor.
            Err(e) => eprintln!("dropkickd: could not save the calendar: {e}"),
        }
        self.calendar_token = None;
    }

    /// Save ~2s after the last edit. Each new edit pushes the write back, so a
    /// burst of typing costs one write rather than one per keystroke.
    fn schedule_calendar_save(&mut self) {
        if let Some(tok) = self.calendar_token.take() {
            self.handle.remove(tok);
        }
        self.calendar_token = self
            .handle
            .insert_source(
                Timer::from_duration(Duration::from_secs(2)),
                |_, _, state: &mut Dropkick| {
                    state.save_calendar_now();
                    TimeoutAction::Drop
                },
            )
            .ok();
    }

    /// Run the portal's colour picker on a thread and post the result back.
    ///
    /// Off-thread because `pick_color` blocks until the user clicks something,
    /// which may be tens of seconds or never. Running it inline would freeze
    /// the panel with no way to cancel -- not even Esc, since the event loop
    /// would not be reading keys.
    ///
    /// The thread posts to the same calloop channel the control socket uses,
    /// so there is no new machinery and still no async runtime.
    #[cfg(feature = "dbus")]
    fn start_eyedropper(&mut self) {
        // The panel is an overlay covering the top of the screen, and the
        // portal is about to ask the user to click a pixel. Get out of the way.
        self.hide();

        let tx = self.picked.clone();
        std::thread::spawn(move || {
            let result = dropkick_linux::dbus::Bus::session()
                .and_then(|mut bus| bus.pick_color())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    #[cfg(not(feature = "dbus"))]
    fn start_eyedropper(&mut self) {
        // Built with --no-default-features: there is no D-Bus code in the
        // binary at all, so say so rather than appearing to hang.
        self.app.color.picking_ended(Some("this build has no D-Bus support"));
    }

    /// Randomness for Generate, straight from the kernel CSPRNG.
    ///
    /// `/dev/urandom` rather than a crate: `dropkick-core` has no dependencies
    /// and this needs none either. It is the right source for keys on Linux --
    /// it does not block after boot and is not weaker than `/dev/random`.
    fn supply_entropy(&mut self, n: usize) {
        use std::io::Read;
        let mut buf = vec![0u8; n];
        match std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut buf).map(|_| ()))
        {
            Ok(()) => self.app.generate.supply_entropy(&buf),
            // Never fall back to a weaker source. Generate shows "out of
            // randomness" and produces nothing, which is the safe failure.
            Err(e) => eprintln!("dropkickd: no entropy: {e}"),
        }
    }

    fn drain_requests(&mut self, qh: &QueueHandle<Self>) {
        let pending: Vec<Request> = self.app.pending.drain(..).collect();
        for req in pending {
            match req {
                Request::SetClipboard(text) => self.set_clipboard(text, false, qh),
                Request::SetClipboardSecret(text) => self.set_clipboard(text, true, qh),
                Request::Entropy(n) => self.supply_entropy(n),
                Request::SaveCalendar => self.schedule_calendar_save(),
                Request::PickColor => self.start_eyedropper(),
            }
        }
    }

    /// Keep a tick source alive exactly while the timer is running.
    ///
    /// 100ms while the panel is up, so the readout moves; once a second while
    /// hidden, which is enough to notice a countdown has expired without waking
    /// ten times a second forever behind a stopwatch someone forgot about.
    fn ensure_ticking(&mut self) {
        if !self.app.timer.is_running() || self.tick_token.is_some() {
            return;
        }
        let handle = self.handle.clone();
        self.tick_token = handle
            .insert_source(
                Timer::from_duration(Duration::from_millis(100)),
                |_, _, state: &mut Dropkick| state.on_tick(),
            )
            .ok();
    }

    fn on_tick(&mut self) -> TimeoutAction {
        let now = self.started.elapsed().as_millis() as u64;
        if std::env::var_os("DROPKICK_TRACE").is_some() {
            eprintln!(
                "  [tick] now={now} running={} remaining={}",
                self.app.timer.is_running(),
                self.app.timer.remaining_ms(now)
            );
        }
        if self.app.timer.tick(now) {
            if self.settings.config.timer_notify {
                self.notify_timer_finished();
            }
            // A notification in the corner is missable by design; it is meant
            // to be glanceable, not insistent. For an alarm the panel itself
            // shows up too, which is the one alert Dropkick can make that does
            // not depend on the user happening to look at the right corner.
            if self.settings.config.timer_show_panel && self.phase == Phase::Hidden {
                self.show_for_alarm();
            }
        }
        if !self.app.timer.is_running() {
            self.tick_token = None;
            // One last paint so a visible panel shows 0:00 rather than freezing
            // a tenth short of it. Only when *Visible*: a surface still in
            // Mapping has not had its configure acked, and attaching a buffer
            // before that is a protocol error that kills the connection.
            if self.phase == Phase::Visible {
                let qh = self.qh.clone();
                self.draw(&qh);
            }
            return TimeoutAction::Drop;
        }
        if self.phase == Phase::Visible {
            let qh = self.qh.clone();
            self.draw(&qh);
            TimeoutAction::ToDuration(Duration::from_millis(100))
        } else {
            TimeoutAction::ToDuration(Duration::from_secs(1))
        }
    }

    #[cfg(feature = "dbus")]
    fn notify_timer_finished(&mut self) {
        let body = format!("{} elapsed", dropkick_core::timer::format_duration(self.app.timer.duration_ms()));
        // Connect per notification rather than holding a bus socket open for
        // the life of the daemon. The handshake costs a millisecond or two and
        // these are rare.
        match dropkick_linux::dbus::Bus::session()
            .and_then(|mut bus| {
                bus.notify(
                    "Dropkick timer finished",
                    &body,
                    dropkick_linux::dbus::Alert::Alarm { sound: self.settings.config.timer_sound },
                )
            })
        {
            Ok(id) => {
                if std::env::var_os("DROPKICK_DEBUG").is_some() {
                    eprintln!("  [timer] expired, notification {id} posted");
                }
            }
            Err(e) => eprintln!("dropkickd: could not post notification: {e}"),
        }
    }

    #[cfg(not(feature = "dbus"))]
    fn notify_timer_finished(&mut self) {
        // Built without the dbus feature: the timer still finishes, it just
        // cannot tell anyone who is not looking at the panel.
    }

    fn dispatch_key(&mut self, qh: &QueueHandle<Self>, event: KeyEvent) {
        let Some(press) = translate(&event, self.mods) else { return };
        let now = self.started.elapsed().as_millis() as u64;
        let response = self.app.on_key(press, now);
        self.drain_requests(qh);
        // Checked here rather than raised as a Request: the comparison is the
        // detection, and it has to happen after *any* key, not only the ones a
        // tool thought were edits.
        if self.app.scratchpad.text() != self.saved_scratchpad {
            self.schedule_scratchpad_save();
        }
        self.ensure_ticking();
        match response {
            Response::Redraw => self.draw(qh),
            Response::Dismiss => self.hide(),
            Response::Ignored => {}
        }
    }

    fn finish(&mut self) {
        summarise(&self.done);
        self.exit = true;
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

fn summarise(cycles: &[Cycle]) {
    let pick = |f: fn(&Cycle) -> Option<Duration>| -> Vec<f64> {
        let mut v: Vec<f64> = cycles
            .iter()
            .filter_map(f)
            .map(|d| d.as_secs_f64() * 1000.0)
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };

    println!("\n=== {} summons ===", cycles.len());
    println!("{:<22} {:>7} {:>8} {:>7} {:>7}  n", "metric", "min", "median", "p95", "max");
    for (name, f) in [
        ("poke -> configure", (|c: &Cycle| c.configure) as fn(&Cycle) -> Option<Duration>),
        ("poke -> committed", |c: &Cycle| c.committed),
        ("poke -> presented", |c: &Cycle| c.presented),
        ("poke -> FOCUS", |c: &Cycle| c.focused),
    ] {
        let v = pick(f);
        if v.is_empty() {
            println!("{name:<22}       (never observed)");
            continue;
        }
        println!(
            "{:<22} {:>7.2} {:>8.2} {:>7.2} {:>7.2}  {}",
            name,
            v[0],
            percentile(&v, 0.5),
            percentile(&v, 0.95),
            v[v.len() - 1],
            v.len()
        );
    }
    println!("\nFOCUS is the gate: before wl_keyboard.enter, keystrokes go to the");
    println!("previously focused window. Add ~1.1ms for the dropkickctl spawn.");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bench_n: Option<u32> = args.iter().position(|a| a == "--bench").map(|i| {
        args.get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(20)
    });

    if let Some(i) = args.iter().position(|a| a == "--shot") {
        let prefix = args.get(i + 1).cloned().unwrap_or_else(|| "dropkick".to_string());
        if let Err(e) = shots(&prefix) {
            eprintln!("dropkickd: {e}");
            std::process::exit(1);
        }
        return;
    }

    // Dev-only: prove the D-Bus client end to end.
    #[cfg(feature = "dbus")]
    if let Some(i) = args.iter().position(|a| a == "--notify") {
        let text = args.get(i + 1).cloned().unwrap_or_else(|| "test".to_string());
        match dropkick_linux::dbus::Bus::session().and_then(|mut b| b.notify("Dropkick", &text, dropkick_linux::dbus::Alert::Alarm { sound: true })) {
            Ok(id) => println!("dropkickd: notification {id} posted"),
            Err(e) => {
                eprintln!("dropkickd: notify failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Dev-only: take ownership of the selection and hold it, so clipboard
    // capture can be exercised without installing a clipboard utility.
    let set_clipboard: Option<String> = args
        .iter()
        .position(|a| a == "--set-clipboard")
        .and_then(|i| args.get(i + 1).cloned());

    let strategy = match args.iter().position(|a| a == "--strategy").and_then(|i| args.get(i + 1)) {
        Some(v) if v == "remap" => Strategy::Remap,
        _ => Strategy::Recreate,
    };

    let settings = dropkick_linux::settings::Settings::load();

    let (text, big) = match (TextRenderer::new(16.0), TextRenderer::new(58.0)) {
        (Ok(t), Ok(b)) => (t, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("dropkickd: {e}");
            std::process::exit(1);
        }
    };

    let mut owns_socket = false;
    let conn = Connection::connect_to_env().expect("no Wayland compositor");
    let (globals, event_queue) = registry_queue_init(&conn).expect("registry init");
    let qh: QueueHandle<Dropkick> = event_queue.handle();

    let mut event_loop: EventLoop<'static, Dropkick> =
        EventLoop::try_new().expect("event loop");
    let handle = event_loop.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("zwlr_layer_shell_v1 missing");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let pool = SlotPool::new((WIDTH * HEIGHT * 4) as usize, &shm).expect("shm pool");

    // The eyedropper's answer comes back on its own channel, from a thread.
    let (picked_tx, picked_rx) = channel::channel::<Picked>();

    let (tx, rx) = channel::channel::<ipc::Cmd>();
    handle
        .insert_source(rx, |ev, _, state: &mut Dropkick| {
            if let channel::Event::Msg(cmd) = ev {
                match cmd {
                    ipc::Cmd::Show => state.show(),
                    ipc::Cmd::Hide => state.hide(),
                    ipc::Cmd::Toggle => state.toggle(),
                    ipc::Cmd::Quit => {
                        state.save_calendar_now();
                        // This is the one that lost a set of notes: quit used
                        // to save the calendar and nothing else.
                        state.save_scratchpad_now();
                        state.exit = true;
                    }
                }
            }
        })
        .expect("insert control channel");

    handle
        .insert_source(picked_rx, |ev, _, state: &mut Dropkick| {
            let channel::Event::Msg(result) = ev else { return };
            use dropkick_linux::dbus::PickResult;
            match result {
                Ok(PickResult::Picked(r, g, b)) => {
                    // The portal reports 0..=1 doubles; the tool works in 8-bit
                    // sRGB. Round rather than truncate, or 1.0 lands on 254.
                    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    state.app.color.set(dropkick_core::color::Color::new(
                        to8(r),
                        to8(g),
                        to8(b),
                    ));
                    state.app.active = dropkick_core::ToolId::Color;
                }
                // Cancelling is not a failure, and must not leave the panel
                // saying "pick a colour" forever.
                Ok(PickResult::Cancelled(1)) => state.app.color.picking_ended(None),
                // Any other code is the portal refusing rather than the user
                // changing their mind, and saying so beats a silent shrug.
                Ok(PickResult::Cancelled(code)) => {
                    eprintln!("dropkickd: eyedropper: portal returned response code {code}");
                    state.app.color.picking_ended(Some("the desktop refused the colour picker"));
                }
                Ok(PickResult::TimedOut) => {
                    eprintln!("dropkickd: eyedropper: the portal never sent a Response");
                    state.app.color.picking_ended(Some("the picker never answered"));
                }
                Ok(PickResult::Undecodable) => {
                    eprintln!("dropkickd: eyedropper: could not read the portal's reply");
                    state.app.color.picking_ended(Some("could not read the picker's answer"));
                }
                Err(e) => {
                    eprintln!("dropkickd: eyedropper: {e}");
                    state.app.color.picking_ended(Some("the colour picker did not answer"));
                }
            }
            // Come back so the answer is visible. The panel hid itself to get
            // out of the picker's way.
            state.show();
        })
        .expect("insert eyedropper channel");

    let mut state = Dropkick {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        compositor,
        layer_shell,
        layer: None,
        qh: qh.clone(),
        strategy,
        cycle_id: 0,
        keyboard: None,
        handle: handle.clone(),
        app: App::new(),
        settings,
        text,
        big,
        tick_token: None,
        calendar_token: None,
        scratchpad_token: None,
        saved_scratchpad: String::new(),
        picked: picked_tx,
        grabbed_exclusive: false,
        mods: Mods::default(),
        started: Instant::now(),
        conn: conn.clone(),
        data_device: None,
        data_manager: None,
        offers: HashMap::new(),
        source: None,
        source_text: String::new(),
        skip_next_selection: false,
        width: WIDTH,
        height: HEIGHT,
        phase: Phase::Hidden,
        cycle: None,
        done: Vec::new(),
        bench_left: bench_n.unwrap_or(0).saturating_sub(1),
        bench: bench_n.is_some(),
        exit: false,
    };

    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .expect("insert wayland source");

    // Clipboard capture starts with the daemon, not with the panel: history you
    // only collect while the window is open is not history.
    state.init_clipboard(&qh);

    // The config directory predates the storage module and was created with
    // whatever the umask gave -- 0775/0664 on this machine, so world-readable.
    // Nothing secret is in it, but the project claims its files are private.
    if let Some(dir) = dropkick_linux::settings::config_path().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        if let Err(e) = storage::tighten(&dir) {
            eprintln!("dropkickd: could not tighten {}: {e}", dir.display());
        }
    }

    // Read the calendar and scratchpad off disk before the first summon.
    state.load_calendar();
    state.load_scratchpad();

    // Dev-only: seed the scratchpad, so the save-on-quit path can be exercised
    // without a compositor and without typing. This is the path that lost a
    // set of notes; it is worth being able to test end to end.
    if let Ok(seed) = std::env::var("DROPKICK_DEBUG_SCRATCHPAD") {
        state.app.scratchpad.load(&seed);
        println!("dropkickd: seeded scratchpad with {} bytes", seed.len());
    }

    // Seed the generator's pool now rather than on first use, so the first
    // password costs no file open at the moment someone is waiting for it.
    if let Some(n) = state.app.generate.wants_entropy() {
        state.supply_entropy(n);
    }

    // Dev-only: arm a countdown at startup, so the one behaviour that only
    // happens with the panel hidden can actually be exercised.
    if let Some(spec) = std::env::var_os("DROPKICK_DEBUG_TIMER") {
        let spec = spec.to_string_lossy().to_string();
        for c in spec.chars() {
            state.app.timer.on_key(dropkick_core::key::KeyPress::ch(c), 0);
        }
        state.app.timer.on_key(dropkick_core::key::KeyPress::plain(DkKey::Enter), 0);
        state.ensure_ticking();
        println!(
            "dropkickd: debug countdown armed ({spec}) running={} token={}",
            state.app.timer.is_running(),
            state.tick_token.is_some()
        );
    }

    if state.bench {
        println!(
            "dropkickd: benching {} summons, strategy {:?}\n",
            bench_n.unwrap_or(0),
            strategy
        );
        // The seat/keyboard arrives via the registry roundtrip; give the loop a
        // moment before the first summon or FOCUS is unmeasurable.
        let _ = handle.insert_source(
            Timer::from_duration(Duration::from_millis(300)),
            |_, _, state: &mut Dropkick| {
                state.show();
                TimeoutAction::Drop
            },
        );
    } else if let Some(text) = set_clipboard {
        println!("dropkickd: holding the selection ({} bytes); ctrl-c to release", text.len());
        state.set_clipboard(text, false, &qh);
    } else {
        match ipc::spawn_listener(tx) {
            Ok(path) => {
                println!("dropkickd: listening on {} (0600)", path.display());
                owns_socket = true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                eprintln!("dropkickd: {e}. Stop it first:  dropkickctl quit");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("dropkickd: cannot bind control socket: {e}");
                return;
            }
        }
    }

    loop {
        if state.exit {
            break;
        }
        if event_loop
            .dispatch(Duration::from_millis(200), &mut state)
            .is_err()
        {
            break;
        }
    }

    if !state.bench && !state.done.is_empty() {
        summarise(&state.done);
    }
    // Only clean up a socket this process actually created. --bench, --shot and
    // --set-clipboard never bind one, and deleting the path anyway unlinks a
    // *running* daemon's socket, leaving it alive but unreachable.
    if owns_socket {
        let _ = std::fs::remove_file(ipc::socket_path());
    }
}

/// Wayland keysyms -> Dropkick's platform-independent key model. The tools
/// never see a keysym; this is the only place that knows what one is.
fn translate(event: &KeyEvent, mods: Mods) -> Option<KeyPress> {
    let key = match event.keysym {
        Keysym::Escape => DkKey::Escape,
        Keysym::Return | Keysym::KP_Enter => DkKey::Enter,
        Keysym::BackSpace => DkKey::Backspace,
        Keysym::Delete => DkKey::Delete,
        Keysym::Tab => DkKey::Tab,
        Keysym::Left => DkKey::Left,
        Keysym::Right => DkKey::Right,
        Keysym::Up => DkKey::Up,
        Keysym::Down => DkKey::Down,
        Keysym::Home => DkKey::Home,
        Keysym::End => DkKey::End,
        _ => {
            // Alt suppresses the utf8 interpretation on most layouts, so fall
            // back to the keysym's own character. That fallback is what makes
            // Alt+Q work while Q still types a Q.
            let c = event
                .utf8
                .as_ref()
                .and_then(|s| s.chars().next())
                .filter(|c| !c.is_control())
                .or_else(|| event.keysym.key_char())?;
            DkKey::Char(c)
        }
    };
    Some(KeyPress { key, mods })
}

/// Plausible clipboard history for the rendered frames.
fn seed_clipboard(app: &mut App) {
    for text in [
        "cargo build --release",
        "postgres://localhost:5432/dropkick",
        "https://wayland.app/protocols/wlr-data-control-unstable-v1",
        "don't forget migration 37",
        "postgres backup command: pg_dump -Fc dropkick > dk.dump",
        "0x6FB3F2",
    ] {
        app.clipboard.record(text, &["text/plain;charset=utf-8".to_string()]);
    }
}

/// Render frames to PPM without a compositor, so the UI can be inspected in
/// review and in CI rather than only by looking at a running desktop.
fn shots(prefix: &str) -> std::io::Result<()> {
    let (mut text, mut big) = match (TextRenderer::new(16.0), TextRenderer::new(58.0)) {
        (Ok(t), Ok(b)) => (t, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("dropkickd: {e}");
            std::process::exit(1);
        }
    };
    // Frames render at a fixed instant so they are byte-identical run to run;
    // setup drives keys at t=0, so the timer shows a minute of progress.
    const SHOT_NOW_MS: u64 = 63_000;

    type Frame = (&'static str, Box<dyn Fn(&mut App)>);
    let frames: Vec<Frame> = vec![
        (
            "calculator",
            Box::new(|app: &mut App| {
                // Switch to the tab explicitly. This scene used to rely on the
                // Calculator being the default, and silently became a picture
                // of an empty Scratchpad the day the default changed -- with
                // the broken image committed and shown in the README.
                app.on_key(KeyPress::alt('e'), 0);
                app.calculator.today = Some(dropkick_core::date::Date::new(2026, 8, 22));
                for (expr, enter) in [
                    ("437*1.18", true),
                    ("25 feet in furlongs", true),
                    ("98.6 F in C", true),
                    ("today + 90 days", true),
                    ("2026-12-25 - today", true),
                    ("453/17", false),
                ] {
                    for c in expr.chars() {
                        app.on_key(KeyPress::ch(c), 0);
                    }
                    if enter {
                        app.on_key(KeyPress::plain(DkKey::Enter), 0);
                    }
                }
            }),
        ),
        (
            "scratchpad-insert",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                for c in "call Frank re: staging server".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
                for c in "don't forget migration 37".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
        (
            "scratchpad-command",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                for c in "Quick note: Shift+Q types a Q".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Escape), 0);
                for _ in 0..8 {
                    app.on_key(KeyPress::ch('h'), 0);
                }
            }),
        ),
        (
            "scratchpad-wrap",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                let para = "Word wrap matters more than it sounds: without it a long \
thought runs off the right edge and the rest of the sentence is simply gone, which \
is exactly what happened the first time this scratchpad was used in anger. Wrapping \
is presentation only - the buffer still holds real lines, so nothing gains a hard \
break it did not have.";
                for c in para.chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
        (
            "scratchpad-visual",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                for c in "call Frank re: staging server".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
                for c in "don't forget migration 37".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Escape), 0);
                // Character-wise, starting mid-word on the second line and
                // running back up into the first: the case where the highlight
                // has to stop partway along one row and fill the rest of another.
                for c in "0lllllllllllvkllll".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
        (
            "scratchpad-visual-line",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                for (i, line) in
                    ["milk", "bread", "coffee beans", "something for Sunday"].iter().enumerate()
                {
                    if i > 0 {
                        app.on_key(KeyPress::plain(DkKey::Enter), 0);
                    }
                    for c in line.chars() {
                        app.on_key(KeyPress::ch(c), 0);
                    }
                }
                app.on_key(KeyPress::plain(DkKey::Escape), 0);
                for c in "ggjVj".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
        (
            "scratchpad-visual-wrapped",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::ch('i'), 0);
                let para = "A selection that crosses a wrap is the case worth looking at, \
because the buffer knows nothing about display rows and the renderer knows nothing \
about the buffer's idea of a line. If the highlight is right here it is right \
everywhere.";
                for c in para.chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Escape), 0);
                // One logical line, selected across the wrap boundary.
                app.on_key(KeyPress::ch('0'), 0);
                for _ in 0..40 {
                    app.on_key(KeyPress::ch('l'), 0);
                }
                app.on_key(KeyPress::ch('v'), 0);
                for _ in 0..80 {
                    app.on_key(KeyPress::ch('l'), 0);
                }
            }),
        ),
        (
            "calculator-error",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('e'), 0);
                for c in "25 feet in furlongs".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
            }),
        ),
        (
            "help-scratchpad",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('q'), 0);
                app.on_key(KeyPress::alt('/'), 0);
            }),
        ),
        (
            "help-calendar",
            Box::new(|app: &mut App| {
                // The longest key column of the seven, so it is the one that
                // shows whether the two columns collide.
                app.on_key(KeyPress::alt('r'), 0);
                app.on_key(KeyPress::alt('/'), 0);
            }),
        ),
        (
            "color-opening",
            Box::new(|app: &mut App| {
                // Nothing typed: what someone sees the first time they land on
                // the tab. It used to be an empty pane, which read as a tool
                // with nothing in it.
                app.on_key(KeyPress::alt('u'), 0);
            }),
        ),
        (
            "color",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('u'), 0);
                for c in "tomato".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
            }),
        ),
        (
            "color-dark",
            Box::new(|app: &mut App| {
                // A dark colour, to check the label on the swatch flips to
                // white. One fixed label colour is unreadable on half the
                // spectrum, so this is the case worth looking at.
                app.on_key(KeyPress::alt('u'), 0);
                for c in "#1B2A4A".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
            }),
        ),
        (
            "calendar",
            Box::new(|app: &mut App| {
                use dropkick_core::date::Date;
                app.on_key(KeyPress::alt('r'), 0);
                // A fixed date, so the frame is identical run to run.
                app.calendar.set_today(Date::new(2026, 8, 21));
                app.calendar.focus = Date::new(2026, 8, 21);
                app.calendar.add(Date::new(2026, 8, 21), Some(9 * 60), "Standup");
                app.calendar.add(Date::new(2026, 8, 21), Some(15 * 60), "Dentist");
                app.calendar.add(Date::new(2026, 8, 21), None, "Bin day");
                app.calendar.add(Date::new(2026, 8, 4), Some(12 * 60), "Lunch with Dana");
                app.calendar.add(Date::new(2026, 8, 27), None, "Rent due");
                app.calendar.add(Date::new(2026, 8, 28), Some(18 * 60 + 30), "Flight");
            }),
        ),
        (
            "calendar-ambiguous-time",
            Box::new(|app: &mut App| {
                use dropkick_core::date::Date;
                app.on_key(KeyPress::alt('r'), 0);
                app.calendar.set_today(Date::new(2026, 8, 21));
                app.calendar.focus = Date::new(2026, 8, 21);
                for c in "aDentist".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
                app.on_key(KeyPress::ch('3'), 0);
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
            }),
        ),
        (
            "generate-password",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('y'), 0);
                // Fixed bytes, so the frame is byte-identical run to run like
                // every other shot -- never how the real tool gets randomness.
                // Scattered rather than 0,1,2,...: sequential bytes map to
                // sequential alphabet indices and produce "abcdefg...", which
                // is correct and looks broken.
                let seed: Vec<u8> =
                    (0..252u32).map(|i| (i.wrapping_mul(97).wrapping_add(41) % 256) as u8).collect();
                app.generate.supply_entropy(&seed);
                app.on_key(KeyPress::ch('r'), 0);
            }),
        ),
        (
            "generate-uuid",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('y'), 0);
                let seed: Vec<u8> = (0..=251u8).rev().collect();
                app.generate.supply_entropy(&seed);
                app.on_key(KeyPress::ch('u'), 0);
            }),
        ),
        (
            "clipboard-masked",
            Box::new(|app: &mut App| {
                seed_clipboard(app);
                app.on_key(KeyPress::alt('w'), 0);
            }),
        ),
        (
            "clipboard-revealed",
            Box::new(|app: &mut App| {
                seed_clipboard(app);
                app.on_key(KeyPress::alt('w'), 0);
                app.on_key(KeyPress::ch('j'), 0); // first key only reveals
                app.on_key(KeyPress::ch('j'), 0);
                app.on_key(KeyPress::ch('p'), 0); // pin the selection
            }),
        ),
        (
            "clipboard-search",
            Box::new(|app: &mut App| {
                seed_clipboard(app);
                app.on_key(KeyPress::alt('w'), 0);
                app.on_key(KeyPress::ch('j'), 0);
                app.on_key(KeyPress::ch('/'), 0);
                for c in "postgres".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
        (
            "timer-countdown",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('t'), 0);
                for c in "25m".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
                app.on_key(KeyPress::plain(DkKey::Enter), 0);
            }),
        ),
        (
            "timer-entry",
            Box::new(|app: &mut App| {
                app.on_key(KeyPress::alt('t'), 0);
                for c in "1h30m".chars() {
                    app.on_key(KeyPress::ch(c), 0);
                }
            }),
        ),
    ];

    for (name, setup) in frames {
        let mut app = App::new();
        setup(&mut app);
        let mut buf = vec![0u8; (SURFACE_W * SURFACE_H * 4) as usize];
        let mut canvas = Canvas::new(&mut buf, SURFACE_W as i32, SURFACE_H as i32);
        // Shots are rendered solid: an opacity setting would make every frame
        // depend on the machine's config file rather than on the code.
        render::draw_surface(&app, &mut text, &mut big, SHOT_NOW_MS, &mut canvas, 255);
        // PAM, not PPM: the panel is no longer a rectangle and now casts a
        // shadow, and PPM has no alpha to represent either. `pamtopng` keeps it.
        let path = format!("{prefix}-{name}.pam");
        std::fs::write(&path, canvas.to_pam())?;
        println!("wrote {path}");
    }
    Ok(())
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for Dropkick {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for Dropkick {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                state.offers.insert(id.id(), Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else { return };
                let mimes = state.offers.remove(&offer.id()).unwrap_or_default();
                // Our own selection comes straight back to us. Reading it would
                // mean writing and reading the same pipe on one thread, which
                // deadlocks as soon as the text exceeds the pipe buffer.
                if state.skip_next_selection {
                    state.skip_next_selection = false;
                    return;
                }
                state.receive_offer(&offer, mimes);
            }
            // Primary selection changes on every text *highlight*, not every
            // copy. Capturing it would mean recording everything the user drags
            // a cursor over, so it stays off.
            zwlr_data_control_device_v1::Event::PrimarySelection { .. } => {}
            zwlr_data_control_device_v1::Event::Finished => {
                state.data_device = None;
            }
            _ => {}
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(Dropkick, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for Dropkick {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.offers.entry(offer.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for Dropkick {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type: _, fd } => {
                // Small texts and a reader that is already waiting, so a
                // blocking write is fine here.
                let mut file = std::fs::File::from(fd);
                let _ = file.write_all(state.source_text.as_bytes());
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                // Someone else took the selection; we are no longer the owner.
                if let Some(source) = state.source.take() {
                    source.destroy();
                }
                state.source_text.clear();
            }
            _ => {}
        }
    }
}

impl CompositorHandler for Dropkick {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    /// Frame callback: the compositor has taken our buffer. This is the closest
    /// honest proxy for "the user can see it".
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if let Some(c) = self.cycle.as_mut() {
            if c.presented.is_none() {
                c.presented = Some(c.t0.elapsed());
            }
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Dropkick {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        eprintln!("  [trace] layer surface closed by compositor (phase {:?})", self.phase);
        self.layer = None;
        self.phase = Phase::Hidden;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.width = NonZeroU32::new(configure.new_size.0).map_or(WIDTH, NonZeroU32::get);
        self.height = NonZeroU32::new(configure.new_size.1).map_or(HEIGHT, NonZeroU32::get);

        if let Some(c) = self.cycle.as_mut() {
            if c.configure.is_none() {
                c.configure = Some(c.t0.elapsed());
            }
        }

        if self.phase == Phase::Mapping {
            self.phase = Phase::Visible;
            self.draw(qh);
        }
    }
}

impl KeyboardHandler for Dropkick {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if !self.surface_matches(surface) {
            return;
        }
        if std::env::var_os("DROPKICK_TRACE").is_some() {
            eprintln!("  [focus] enter - the panel has the keyboard");
        }
        if let Some(c) = self.cycle.as_mut() {
            if c.focused.is_none() {
                c.focused = Some(c.t0.elapsed());
            }
        }

        // Now that we HAVE the keyboard, stop demanding it.
        //
        // Exclusive is how a layer surface gets focus the instant it maps, with
        // no click and no window management -- that is what makes a summon
        // 1.6ms. But exclusive also means the compositor will not let anything
        // else have the keyboard until we go away, so clicking back to your
        // editor did nothing. Downgrading to OnDemand the moment focus lands
        // keeps the fast summon and restores ordinary behaviour: click away and
        // focus leaves, click the panel and it comes back.
        if self.grabbed_exclusive {
            self.grabbed_exclusive = false;
            if let Some(layer) = self.layer.as_ref() {
                layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
                layer.commit();
            }
        }

        if self.bench {
            self.schedule_bench_hide();
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.surface_matches(surface) && std::env::var_os("DROPKICK_TRACE").is_some() {
            eprintln!("  [focus] leave - the keyboard went back to something else");
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.dispatch_key(qh, event);
    }

    /// The *compositor-driven* repeat path: only ever called when the server
    /// sends `KeyState::Repeated` itself, which wl_keyboard gained in v10.
    /// cosmic-comp does not, which is why implementing this alone did nothing
    /// and holding Backspace deleted one character. The repeat that actually
    /// fires here is client-side, synthesised by the callback installed in
    /// `new_capability`. Both land in `dispatch_key`, so a compositor that
    /// grows native repeat needs no change.
    fn repeat_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.dispatch_key(qh, event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = Mods {
            alt: modifiers.alt,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
        };
    }
}

impl SeatHandler for Dropkick {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            // `get_keyboard` -- no repeat. Wayland sends one press and one
            // release and nothing in between: unlike X11, synthesising the
            // repeats from `wl_keyboard.repeat_info` is the *client's* job. So
            // holding Backspace deleted exactly one character, and `repeat_key`
            // below was dead code that had never once run.
            //
            // `get_keyboard_with_repeat` does that synthesis, driven by the
            // calloop loop this daemon already has. The compositor's own rate
            // and delay are used, so held keys match every other application.
            let handle = self.handle.clone();
            self.keyboard = self
                .seat_state
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    handle,
                    Box::new(|state: &mut Dropkick, _kbd, event| {
                        let qh = state.qh.clone();
                        state.dispatch_key(&qh, event);
                    }),
                )
                .ok();
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kbd) = self.keyboard.take() {
                kbd.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl OutputHandler for Dropkick {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Dropkick {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Dropkick);

impl ProvidesRegistryState for Dropkick {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Dropkick);
