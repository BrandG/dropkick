//! The tab bar, the Esc ladder, and the routing between them.
//!
//! Keys bind to *tools*, never to tab positions. Inserting a tool later must not
//! shift anyone's fingers, so Calendar keeps `w` and ColorPicker keeps `r` even
//! though neither is built — the tabs render dimmed rather than being absent.
//! A retired tool's key stays retired.

use crate::calculator::Calculator;
use crate::clipboard::{Clipboard, Outcome};
use crate::generate::{Generate, Outcome as GenOutcome};
use crate::key::{Key, KeyPress};
use crate::scratchpad::Scratchpad;
use crate::timer::Timer;

/// Two Escapes closer together than this dismiss from anywhere, regardless of
/// how many levels are open.
pub const DOUBLE_ESC_MS: u64 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolId {
    Calculator,
    Scratchpad,
    Clipboard,
    Calendar,
    Timer,
    Generate,
    Color,
}

pub struct Tool {
    pub id: ToolId,
    pub name: &'static str,
    /// Permanent. Never reassigned, never reused.
    pub key: char,
    pub built: bool,
}

/// The permanent roster. Ten slots exist in total (`q` through `p`); three are
/// deliberately left unspent. Keys never move and retired keys stay retired, so
/// this table is the one thing in Dropkick that must not be casually edited.
pub const TOOLS: [Tool; 7] = [
    Tool { id: ToolId::Scratchpad, name: "Scratchpad", key: 'q', built: true },
    Tool { id: ToolId::Clipboard, name: "Clipboard", key: 'w', built: true },
    Tool { id: ToolId::Calculator, name: "Calculator", key: 'e', built: true },
    Tool { id: ToolId::Calendar, name: "Calendar", key: 'r', built: false },
    Tool { id: ToolId::Timer, name: "Timer", key: 't', built: true },
    Tool { id: ToolId::Generate, name: "Generate", key: 'y', built: true },
    Tool { id: ToolId::Color, name: "Color", key: 'u', built: false },
];

/// Work the core wants done that only the platform can do. Core never touches
/// a system clipboard, a notification daemon or a file; it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Put this text on the system clipboard.
    SetClipboard(String),
    /// Put this on the clipboard as a **secret**: offered with the password
    /// manager hint, and never entered into clipboard history — not ours, and
    /// not any other manager on the system that honours the hint.
    ///
    /// A separate variant rather than a flag so that a call site cannot copy a
    /// secret by forgetting to say it was one.
    SetClipboardSecret(String),
    /// Top the Generate pool up with this many bytes of OS randomness. Raised
    /// by the core because the core has no way to read any.
    Entropy(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// State changed; the surface needs repainting.
    Redraw,
    /// Dropkick should disappear.
    Dismiss,
    /// Nothing happened; do not repaint.
    Ignored,
}

pub struct App {
    pub active: ToolId,
    pub calculator: Calculator,
    pub scratchpad: Scratchpad,
    pub clipboard: Clipboard,
    pub timer: Timer,
    pub generate: Generate,
    /// Transient one-line message shown in the hint bar.
    pub notice: Option<String>,
    /// Drained by the platform after every key.
    pub pending: Vec<Request>,
    last_esc_ms: Option<u64>,
}

impl Default for App {
    fn default() -> Self {
        App {
            active: ToolId::Scratchpad,
            calculator: Calculator::new(),
            scratchpad: Scratchpad::new(),
            clipboard: Clipboard::new(),
            timer: Timer::new(),
            generate: Generate::new(),
            notice: None,
            pending: Vec::new(),
            last_esc_ms: None,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_tool(&self) -> &'static Tool {
        TOOLS.iter().find(|t| t.id == self.active).expect("active tool is registered")
    }

    /// The hint line under the content, the "those aren't buttons, they're
    /// reminders" row.
    pub fn hints(&self) -> &'static str {
        match self.active {
            ToolId::Calculator => "expression + enter =   ans reuses last   alt+h/l tabs   esc x",
            ToolId::Scratchpad => match self.scratchpad.mode {
                crate::scratchpad::Mode::Insert => "INSERT   esc or ctrl-c leaves",
                crate::scratchpad::Mode::Visual => "VISUAL   d cut   y copy   hjkl gg G extend   esc back",
                crate::scratchpad::Mode::VisualLine => "VISUAL LINE   d cut   y copy   jk gg G extend   esc back",
                crate::scratchpad::Mode::Command => "i insert  v sel  dd cut  yy copy  Y copy all  u undo  hjkl move  esc x",
            },
            ToolId::Clipboard => {
                if !self.clipboard.revealed {
                    "hidden - press any key to reveal   esc x"
                } else {
                    match self.clipboard.mode {
                        crate::clipboard::Mode::Search => "SEARCH   enter accept   esc back",
                        crate::clipboard::Mode::List => {
                            "enter paste  / find  p pin  d del  x clear  c pause  m hide  esc x"
                        }
                    }
                }
            }
            ToolId::Generate => match self.generate.kind {
                crate::generate::Kind::Uuid => "p password   r new   y copy   esc x",
                crate::generate::Kind::Password => {
                    "u uuid  a A 0 ! types  x lookalikes  +- length  r new  y copy"
                }
            },
            ToolId::Timer => {
                if self.timer.is_running() {
                    "space pause   r reset   esc x   (it keeps running while hidden)"
                } else {
                    "type 25m + enter   space start/pause   w stopwatch   r reset   esc x"
                }
            }
            _ => "not built yet   alt+q scratchpad   alt+w clipboard   alt+e calculator",
        }
    }

    /// Step to the next or previous *built* tool, wrapping around. Unbuilt
    /// tools are skipped: cycling into a dead tab would make the convenience
    /// worse than the direct chord it exists to save.
    fn cycle(&mut self, forward: bool) -> Response {
        let built: Vec<ToolId> = TOOLS.iter().filter(|t| t.built).map(|t| t.id).collect();
        if built.len() < 2 {
            return Response::Ignored;
        }
        let at = built.iter().position(|id| *id == self.active).unwrap_or(0);
        let next = if forward {
            (at + 1) % built.len()
        } else {
            (at + built.len() - 1) % built.len()
        };
        self.active = built[next];
        self.on_enter_tool();
        self.notice = None;
        Response::Redraw
    }

    /// Called whenever a tool becomes active, and on every summon.
    /// Raise a refill if the pool is low. Cheap to call after every key: it is
    /// a length check, and it means a long password can never find itself
    /// half-generated and out of randomness.
    fn ask_for_entropy(&mut self) {
        if let Some(n) = self.generate.wants_entropy() {
            self.pending.push(Request::Entropy(n));
        }
    }

    fn on_enter_tool(&mut self) {
        if self.active == ToolId::Clipboard {
            self.clipboard.on_enter();
        }
        // Clearing the value is unconditional, like the Scratchpad below: a
        // generated password must not still be on screen when the panel comes
        // back, whichever tab it comes back to. Asking for randomness is not --
        // every other tool would otherwise queue a refill it has no use for.
        self.generate.on_enter();
        if self.active == ToolId::Generate {
            self.ask_for_entropy();
        }
        // Unconditional: a pending operator in the Scratchpad has to be cleared
        // even when the tool being entered is a different one, because that is
        // exactly the case that leaves it half-typed.
        self.scratchpad.on_enter();
    }

    /// The platform calls this when the panel is summoned. Resume must not mean
    /// "put whatever was on screen last time back on screen".
    pub fn on_summon(&mut self) {
        self.on_enter_tool();
    }

    pub fn on_key(&mut self, press: KeyPress, now_ms: u64) -> Response {
        // Ctrl+C is a full alias for Esc, not a special case in one tool.
        // Plenty of terminals swallow Esc, so the habit is common, and an alias
        // that behaved differently depending on where you were would be worse
        // than no alias at all. It climbs the same ladder and double-taps the
        // same way.
        let press = if press.mods.ctrl && press.key == Key::Char('c') {
            KeyPress::plain(Key::Escape)
        } else {
            press
        };

        // Tab switching is checked before anything else and works identically in
        // every tool and every mode. That uniformity is the whole reason it
        // lives on Alt rather than Shift.
        if press.mods.alt {
            if let Key::Char(c) = press.key {
                let c = c.to_ascii_lowercase();
                // Cycling lives on h/l rather than p/n on purpose: tool keys
                // come from the top row, so Alt+P would permanently cost the
                // tenth tool slot while Alt+H and Alt+L cost nothing. They are
                // also the directions the Scratchpad already uses.
                if c == 'h' || c == 'l' {
                    return self.cycle(c == 'l');
                }
                if let Some(tool) = TOOLS.iter().find(|t| t.key == c) {
                    self.notice = None;
                    if !tool.built {
                        self.notice = Some(format!("{} isn't built yet", tool.name));
                        return Response::Redraw;
                    }
                    if self.active == tool.id {
                        return Response::Ignored;
                    }
                    // Switching preserves the other tool's state: it is simply
                    // left alone. The clipboard is the exception - it re-masks,
                    // because arriving at it is exactly when its contents should
                    // not already be on screen.
                    self.active = tool.id;
                    self.on_enter_tool();
                    return Response::Redraw;
                }
            }
            return Response::Ignored;
        }

        if press.key == Key::Escape {
            if let Some(prev) = self.last_esc_ms {
                if now_ms.saturating_sub(prev) <= DOUBLE_ESC_MS {
                    self.last_esc_ms = None;
                    return Response::Dismiss;
                }
            }
            self.last_esc_ms = Some(now_ms);
            self.notice = None;
            // Up one level, or out.
            let descended = match self.active {
                ToolId::Scratchpad => self.scratchpad.leave_one_level(),
                ToolId::Clipboard => self.clipboard.leave_one_level(),
                _ => false,
            };
            return if descended { Response::Redraw } else { Response::Dismiss };
        }

        self.notice = None;
        match self.active {
            ToolId::Calculator => {
                if self.calculator.on_key(press) {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            ToolId::Scratchpad => {
                let consumed = self.scratchpad.on_key(press);
                if let Some(text) = self.scratchpad.take_yank() {
                    // Say how much, not just that it happened: a yank whose
                    // range was not what you meant looks identical otherwise.
                    let lines = text.lines().count().max(1);
                    self.notice = Some(if lines == 1 {
                        format!("copied {} characters", text.chars().count())
                    } else {
                        format!("copied {lines} lines")
                    });
                    self.pending.push(Request::SetClipboard(text));
                }
                if consumed {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            ToolId::Timer => {
                if self.timer.on_key(press, now_ms) {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            ToolId::Generate => match self.generate.on_key(press) {
                GenOutcome::CopySecret(text) => {
                    self.pending.push(Request::SetClipboardSecret(text));
                    // Say that it is not in the history, because the Clipboard
                    // tab is right there and its absence would otherwise look
                    // like a bug.
                    self.notice = Some("copied - not saved to clipboard history".to_string());
                    self.ask_for_entropy();
                    Response::Redraw
                }
                GenOutcome::Redraw => {
                    self.ask_for_entropy();
                    Response::Redraw
                }
                GenOutcome::Ignored => Response::Ignored,
            },
            ToolId::Clipboard => match self.clipboard.on_key(press) {
                Outcome::Paste(text) => {
                    self.pending.push(Request::SetClipboard(text));
                    // Pick, vanish, paste. Staying open would just be in the way.
                    Response::Dismiss
                }
                Outcome::Redraw => Response::Redraw,
                Outcome::Ignored => Response::Ignored,
            },
            _ => Response::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::Mode;

    #[test]
    fn alt_switches_tools_from_anywhere() {
        let mut app = App::new();
        assert_eq!(app.active, ToolId::Scratchpad, "first launch lands on the leftmost tool");
        assert_eq!(app.on_key(KeyPress::alt('e'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Calculator);
        assert_eq!(app.on_key(KeyPress::alt('q'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Scratchpad);
    }

    #[test]
    fn alt_switches_even_from_insert_mode() {
        // The property Shift could not deliver.
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.scratchpad.mode, Mode::Insert);
        app.on_key(KeyPress::alt('e'), 0);
        assert_eq!(app.active, ToolId::Calculator);
        // ...and the notepad is left exactly as it was.
        assert_eq!(app.scratchpad.mode, Mode::Insert);
    }

    #[test]
    fn shift_letters_are_text_not_tab_switches() {
        // Regression guard for the decision itself: typing a capital Q into a
        // note must never change tools.
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        for c in "Quick note".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        assert_eq!(app.active, ToolId::Scratchpad);
        assert_eq!(app.scratchpad.text(), "Quick note");
    }

    #[test]
    fn switching_preserves_both_tools_state() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('e'), 0);
        for c in "12*12".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        for c in "hello".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::alt('e'), 0);
        assert_eq!(app.calculator.input, "12*12");
        assert_eq!(app.scratchpad.text(), "hello");
    }

    #[test]
    fn unbuilt_tools_keep_their_key_and_say_so() {
        let mut app = App::new();
        assert_eq!(app.on_key(KeyPress::alt('r'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Scratchpad, "an unbuilt tab must not steal focus");
        assert!(app.notice.as_deref().unwrap().contains("Calendar"));
    }

    #[test]
    fn esc_ladder_climbs_one_level_at_a_time() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        // insert -> command
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1000), Response::Redraw);
        assert_eq!(app.scratchpad.mode, Mode::Command);
        // command -> dismiss
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 5000), Response::Dismiss);
    }

    #[test]
    fn double_esc_dismisses_from_the_deepest_level() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1000), Response::Redraw);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1100), Response::Dismiss);
    }

    #[test]
    fn slow_double_esc_is_two_separate_escapes() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1000), Response::Redraw);
        // Well beyond the window: this is "leave the tool", which from command
        // mode means dismiss anyway.
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 9000), Response::Dismiss);
    }

    #[test]
    fn esc_in_calculator_dismisses_immediately() {
        let mut app = App::new();
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 0), Response::Dismiss);
    }

    #[test]
    fn cycling_steps_between_built_tools() {
        let mut app = App::new();
        assert_eq!(app.active, ToolId::Scratchpad);
        assert_eq!(app.on_key(KeyPress::alt('l'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Clipboard);
        assert_eq!(app.on_key(KeyPress::alt('h'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Scratchpad);
    }

    #[test]
    fn cycling_skips_unbuilt_tools_and_wraps() {
        // Derived from TOOLS rather than hard-coded, so building a new tool
        // does not require editing this test to keep it honest. Landing on an
        // unbuilt tool fails, because `built` does not contain one.
        let built: Vec<ToolId> = TOOLS.iter().filter(|t| t.built).map(|t| t.id).collect();
        assert!(built.len() >= 2);

        let mut app = App::new();
        assert_eq!(app.active, built[0], "first launch lands on the leftmost tool");
        for expected in built.iter().skip(1).chain(built.iter().take(1)) {
            app.on_key(KeyPress::alt('l'), 0);
            assert_eq!(app.active, *expected);
        }
        app.on_key(KeyPress::alt('h'), 0);
        assert_eq!(app.active, built[built.len() - 1], "backwards wraps too");
    }

    #[test]
    fn cycling_works_from_inside_insert_mode() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        app.on_key(KeyPress::alt('l'), 0);
        assert_eq!(app.active, ToolId::Clipboard);
    }

    #[test]
    fn plain_h_and_l_still_move_the_cursor() {
        // Regression guard: cycling must not steal the Scratchpad's motions.
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        for c in "hello".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::plain(Key::Escape), 0);
        app.on_key(KeyPress::ch('h'), 0);
        assert_eq!(app.active, ToolId::Scratchpad);
        assert_eq!(app.scratchpad.text(), "hello");
    }

    #[test]
    fn arriving_at_the_clipboard_always_masks_it() {
        let mut app = App::new();
        app.clipboard.record("secret token", &[]);
        app.on_key(KeyPress::alt('w'), 0);
        assert_eq!(app.active, ToolId::Clipboard);
        assert!(!app.clipboard.revealed);
        // reveal, leave, come back
        app.on_key(KeyPress::ch('j'), 0);
        assert!(app.clipboard.revealed);
        app.on_key(KeyPress::alt('e'), 0);
        app.on_key(KeyPress::alt('w'), 0);
        assert!(!app.clipboard.revealed, "returning must re-mask");
    }

    #[test]
    fn a_summon_re_masks_even_without_switching() {
        let mut app = App::new();
        app.clipboard.record("secret token", &[]);
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('j'), 0);
        assert!(app.clipboard.revealed);
        app.on_summon();
        assert!(!app.clipboard.revealed, "resume must not put secrets back on screen");
    }

    #[test]
    fn esc_dismisses_a_masked_clipboard_without_revealing_it() {
        let mut app = App::new();
        app.clipboard.record("secret token", &[]);
        app.on_key(KeyPress::alt('w'), 0);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 0), Response::Dismiss);
        assert!(!app.clipboard.revealed, "Esc must never be the key that reveals");
    }

    #[test]
    fn pasting_asks_the_platform_and_dismisses() {
        let mut app = App::new();
        app.clipboard.record("postgres://localhost", &[]);
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('j'), 0); // reveal
        assert_eq!(app.on_key(KeyPress::plain(Key::Enter), 0), Response::Dismiss);
        assert_eq!(
            app.pending,
            vec![Request::SetClipboard("postgres://localhost".to_string())]
        );
    }

    #[test]
    fn scratchpad_yank_asks_the_platform() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('q'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        for c in "note to self".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::plain(Key::Escape), 0);
        // `yy`, not `y`: yank is a vi operator and needs its motion.
        app.on_key(KeyPress::ch('y'), 0);
        app.on_key(KeyPress::ch('y'), 0);
        assert_eq!(app.pending, vec![Request::SetClipboard("note to self".to_string())]);
    }

    #[test]
    fn generate_copies_as_a_secret_not_as_ordinary_text() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('y'), 0);
        app.generate.supply_entropy(&vec![42u8; 1024]);
        app.pending.clear();
        app.on_key(KeyPress::ch('r'), 0);
        let value = app.generate.value.clone();
        assert!(!value.is_empty());
        app.pending.clear();
        app.on_key(KeyPress::ch('y'), 0);
        // The distinction the whole tool rests on. `SetClipboard` here would put
        // the password into clipboard history.
        assert!(
            app.pending.contains(&Request::SetClipboardSecret(value)),
            "got {:?}",
            app.pending
        );
        assert!(!app
            .pending
            .iter()
            .any(|r| matches!(r, Request::SetClipboard(_))));
    }

    #[test]
    fn a_generated_password_does_not_survive_a_summon() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('y'), 0);
        app.generate.supply_entropy(&vec![7u8; 1024]);
        app.on_key(KeyPress::ch('r'), 0);
        assert!(!app.generate.value.is_empty());
        app.on_summon();
        assert!(app.generate.value.is_empty(), "still on screen next time it opens");
    }

    #[test]
    fn switching_to_another_tool_does_not_ask_for_randomness() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('e'), 0);
        assert!(
            !app.pending.iter().any(|r| matches!(r, Request::Entropy(_))),
            "only Generate needs entropy; {:?}",
            app.pending
        );
    }

    #[test]
    fn ctrl_c_leaves_insert_mode_like_esc() {
        let mut app = App::new();
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.scratchpad.mode, crate::scratchpad::Mode::Insert);
        let ctrl_c = KeyPress::ctrl('c');
        assert_eq!(app.on_key(ctrl_c, 0), Response::Redraw);
        assert_eq!(app.scratchpad.mode, crate::scratchpad::Mode::Command);
    }

    #[test]
    fn ctrl_c_climbs_the_whole_ladder_and_double_taps() {
        let mut app = App::new();
        let ctrl_c = KeyPress::ctrl('c');
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.on_key(ctrl_c, 1000), Response::Redraw); // insert -> command
        assert_eq!(app.on_key(ctrl_c, 1100), Response::Dismiss); // double tap
        // ...and mixing the two works, because they are the same key.
        app.on_key(KeyPress::ch('i'), 2000);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 3000), Response::Redraw);
        assert_eq!(app.on_key(ctrl_c, 3100), Response::Dismiss);
    }

    #[test]
    fn plain_c_is_still_text_in_insert_mode() {
        let mut app = App::new();
        app.on_key(KeyPress::ch('i'), 0);
        for c in "code".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        assert_eq!(app.scratchpad.text(), "code");
        assert_eq!(app.scratchpad.mode, crate::scratchpad::Mode::Insert);
    }

    #[test]
    fn tool_keys_are_unique() {
        for (i, a) in TOOLS.iter().enumerate() {
            for b in TOOLS.iter().skip(i + 1) {
                assert_ne!(a.key, b.key, "{} and {} share a key", a.name, b.name);
            }
        }
    }

    #[test]
    fn tool_keys_never_collide_with_cycling() {
        for t in TOOLS.iter() {
            assert!(t.key != 'h' && t.key != 'l', "{} collides with cycling", t.name);
        }
    }
}
