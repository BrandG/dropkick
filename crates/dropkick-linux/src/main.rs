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

use dropkick_linux::ipc;

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::{
    channel, timer::TimeoutAction, timer::Timer, EventLoop, LoopHandle,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
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

const WIDTH: u32 = 520;
const HEIGHT: u32 = 220;

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
        if self.phase != Phase::Hidden {
            return;
        }
        self.cycle_id += 1;
        let id = self.cycle_id;
        self.cycle = Some(Cycle::new());

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
            layer.set_size(WIDTH, HEIGHT);
            layer.set_anchor(Anchor::TOP);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.commit();
        }
        self.phase = Phase::Mapping;

        // A summon that never configures must report, not hang.
        let h = self.handle.clone();
        let _ = h.insert_source(
            Timer::from_duration(Duration::from_millis(1200)),
            move |_, _, state: &mut Dropkick| {
                if state.cycle_id == id && state.cycle.is_some() {
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
        let (buffer, canvas) = self
            .pool
            .create_buffer(width as i32, height as i32, stride, wl_shm::Format::Argb8888)
            .expect("create buffer");

        // Flat fill. Anything prettier would be measuring the painter, not the
        // compositor.
        canvas.chunks_exact_mut(4).for_each(|chunk| {
            chunk.copy_from_slice(&0xFF10_1418u32.to_le_bytes());
        });

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

    let strategy = match args.iter().position(|a| a == "--strategy").and_then(|i| args.get(i + 1)) {
        Some(v) if v == "remap" => Strategy::Remap,
        _ => Strategy::Recreate,
    };

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

    let (tx, rx) = channel::channel::<ipc::Cmd>();
    handle
        .insert_source(rx, |ev, _, state: &mut Dropkick| {
            if let channel::Event::Msg(cmd) = ev {
                match cmd {
                    ipc::Cmd::Show => state.show(),
                    ipc::Cmd::Hide => state.hide(),
                    ipc::Cmd::Toggle => state.toggle(),
                    ipc::Cmd::Quit => state.exit = true,
                }
            }
        })
        .expect("insert control channel");

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
    } else {
        match ipc::spawn_listener(tx) {
            Ok(path) => println!("dropkickd: listening on {} (0600)", path.display()),
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
    let _ = std::fs::remove_file(ipc::socket_path());
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
        if let Some(c) = self.cycle.as_mut() {
            if c.focused.is_none() {
                c.focused = Some(c.t0.elapsed());
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
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            self.hide();
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
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
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
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
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
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
