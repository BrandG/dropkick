//! The tab bar, the Esc ladder, and the routing between them.
//!
//! Keys bind to *tools*, never to tab positions. Inserting a tool later must not
//! shift anyone's fingers, so Calendar keeps `w` and ColorPicker keeps `r` even
//! though neither is built — the tabs render dimmed rather than being absent.
//! A retired tool's key stays retired.

use crate::calculator::Calculator;
use crate::key::{Key, KeyPress};
use crate::scratchpad::Scratchpad;

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
    Tool { id: ToolId::Calculator, name: "Calculator", key: 'q', built: true },
    Tool { id: ToolId::Scratchpad, name: "Scratchpad", key: 'w', built: true },
    Tool { id: ToolId::Clipboard, name: "Clipboard", key: 'e', built: false },
    Tool { id: ToolId::Calendar, name: "Calendar", key: 'r', built: false },
    Tool { id: ToolId::Timer, name: "Timer", key: 't', built: false },
    Tool { id: ToolId::Generate, name: "Generate", key: 'y', built: false },
    Tool { id: ToolId::Color, name: "Color", key: 'u', built: false },
];

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
    /// Transient one-line message shown in the hint bar.
    pub notice: Option<String>,
    last_esc_ms: Option<u64>,
}

impl Default for App {
    fn default() -> Self {
        App {
            active: ToolId::Calculator,
            calculator: Calculator::new(),
            scratchpad: Scratchpad::new(),
            notice: None,
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
                crate::scratchpad::Mode::Insert => "INSERT   esc command mode",
                crate::scratchpad::Mode::Command => "i insert  o line  x cut  u undo  hjkl move  alt+h/l tabs  esc x",
            },
            _ => "not built yet   alt+q calculator   alt+w scratchpad",
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
        self.notice = None;
        Response::Redraw
    }

    pub fn on_key(&mut self, press: KeyPress, now_ms: u64) -> Response {
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
                    // left alone.
                    self.active = tool.id;
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
                _ => false,
            };
            return if descended { Response::Redraw } else { Response::Dismiss };
        }

        self.notice = None;
        let consumed = match self.active {
            ToolId::Calculator => self.calculator.on_key(press),
            ToolId::Scratchpad => self.scratchpad.on_key(press),
            _ => false,
        };
        if consumed {
            Response::Redraw
        } else {
            Response::Ignored
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
        assert_eq!(app.active, ToolId::Calculator);
        assert_eq!(app.on_key(KeyPress::alt('w'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Scratchpad);
        assert_eq!(app.on_key(KeyPress::alt('q'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Calculator);
    }

    #[test]
    fn alt_switches_even_from_insert_mode() {
        // The property Shift could not deliver.
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.scratchpad.mode, Mode::Insert);
        app.on_key(KeyPress::alt('q'), 0);
        assert_eq!(app.active, ToolId::Calculator);
        // ...and the notepad is left exactly as it was.
        assert_eq!(app.scratchpad.mode, Mode::Insert);
    }

    #[test]
    fn shift_letters_are_text_not_tab_switches() {
        // Regression guard for the decision itself: typing a capital Q into a
        // note must never change tools.
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
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
        for c in "12*12".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        for c in "hello".chars() {
            app.on_key(KeyPress::ch(c), 0);
        }
        app.on_key(KeyPress::alt('q'), 0);
        assert_eq!(app.calculator.input, "12*12");
        assert_eq!(app.scratchpad.text(), "hello");
    }

    #[test]
    fn unbuilt_tools_keep_their_key_and_say_so() {
        let mut app = App::new();
        assert_eq!(app.on_key(KeyPress::alt('r'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Calculator);
        assert!(app.notice.as_deref().unwrap().contains("Calendar"));
    }

    #[test]
    fn esc_ladder_climbs_one_level_at_a_time() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
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
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1000), Response::Redraw);
        assert_eq!(app.on_key(KeyPress::plain(Key::Escape), 1100), Response::Dismiss);
    }

    #[test]
    fn slow_double_esc_is_two_separate_escapes() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
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
        assert_eq!(app.active, ToolId::Calculator);
        assert_eq!(app.on_key(KeyPress::alt('l'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Scratchpad);
        assert_eq!(app.on_key(KeyPress::alt('h'), 0), Response::Redraw);
        assert_eq!(app.active, ToolId::Calculator);
    }

    #[test]
    fn cycling_skips_unbuilt_tools_and_wraps() {
        // Calendar and ColorPicker sit between and after the built pair, so a
        // naive index step would land on a dead tab.
        let mut app = App::new();
        app.on_key(KeyPress::alt('l'), 0);
        assert_eq!(app.active, ToolId::Scratchpad);
        app.on_key(KeyPress::alt('l'), 0);
        assert_eq!(app.active, ToolId::Calculator, "should wrap past ColorPicker");
        app.on_key(KeyPress::alt('h'), 0);
        assert_eq!(app.active, ToolId::Scratchpad, "should wrap backwards past Calendar");
    }

    #[test]
    fn cycling_works_from_inside_insert_mode() {
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
        app.on_key(KeyPress::ch('i'), 0);
        app.on_key(KeyPress::alt('l'), 0);
        assert_eq!(app.active, ToolId::Calculator);
    }

    #[test]
    fn plain_h_and_l_still_move_the_cursor() {
        // Regression guard: cycling must not steal the Scratchpad's motions.
        let mut app = App::new();
        app.on_key(KeyPress::alt('w'), 0);
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
