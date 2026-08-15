//! Scratchpad: the tool that disqualified Shift, and the one that needs undo.
//!
//! Named for what it is. "Notepad" implies documents you keep; "scratchpad"
//! implies things you throw away, and the second is the tool.
//!
//! vi-flavoured but not a vi clone. Command mode navigates; insert mode types.
//! The lesson borrowed from vi is the mode boundary, not the keymap.
//!
//! Undo granularity is deliberate: a snapshot is pushed when a mutation *begins*
//! — entering insert mode, or a command-mode edit — not per character. So one
//! `u` undoes a burst of typing, which is what a person means by "undo", and it
//! is what makes a stray keystroke typed before the panel paints recoverable.

use crate::key::{Key, KeyPress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Command,
    Insert,
}

#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    lines: Vec<String>,
    cx: usize,
    cy: usize,
}

#[derive(Debug)]
pub struct Scratchpad {
    pub lines: Vec<String>,
    pub cx: usize,
    pub cy: usize,
    pub mode: Mode,
    undo: Vec<Snapshot>,
    yank: bool,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Scratchpad {
            lines: vec![String::new()],
            cx: 0,
            cy: 0,
            mode: Mode::Command,
            undo: Vec::new(),
            yank: false,
        }
    }
}

impl Scratchpad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn snapshot(&mut self) {
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cx: self.cx,
            cy: self.cy,
        });
        if self.undo.len() > 128 {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(s) => {
                self.lines = s.lines;
                self.cx = s.cx;
                self.cy = s.cy;
                self.clamp();
                true
            }
            None => false,
        }
    }

    fn line_len(&self) -> usize {
        self.lines.get(self.cy).map_or(0, |l| l.chars().count())
    }

    fn clamp(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cy = self.cy.min(self.lines.len() - 1);
        let max = match self.mode {
            // Command mode sits *on* a character; insert mode sits after it.
            Mode::Command => self.line_len().saturating_sub(1),
            Mode::Insert => self.line_len(),
        };
        self.cx = self.cx.min(max);
    }

    fn byte_at(&self, cx: usize) -> usize {
        self.lines[self.cy]
            .char_indices()
            .nth(cx)
            .map_or(self.lines[self.cy].len(), |(i, _)| i)
    }

    /// Returns true when the key was consumed.
    pub fn on_key(&mut self, press: KeyPress) -> bool {
        if !press.mods.is_plain() {
            return false;
        }
        match self.mode {
            Mode::Insert => self.insert_key(press),
            Mode::Command => self.command_key(press),
        }
    }

    fn insert_key(&mut self, press: KeyPress) -> bool {
        match press.key {
            Key::Char(c) => {
                let at = self.byte_at(self.cx);
                self.lines[self.cy].insert(at, c);
                self.cx += 1;
                true
            }
            Key::Enter => {
                let at = self.byte_at(self.cx);
                let tail = self.lines[self.cy].split_off(at);
                self.lines.insert(self.cy + 1, tail);
                self.cy += 1;
                self.cx = 0;
                true
            }
            Key::Backspace => {
                if self.cx > 0 {
                    let at = self.byte_at(self.cx - 1);
                    self.lines[self.cy].remove(at);
                    self.cx -= 1;
                } else if self.cy > 0 {
                    let cur = self.lines.remove(self.cy);
                    self.cy -= 1;
                    self.cx = self.line_len();
                    self.lines[self.cy].push_str(&cur);
                }
                true
            }
            Key::Left => {
                self.cx = self.cx.saturating_sub(1);
                true
            }
            Key::Right => {
                self.cx = (self.cx + 1).min(self.line_len());
                true
            }
            Key::Up | Key::Down => {
                self.vertical(press.key);
                true
            }
            Key::Home => {
                self.cx = 0;
                true
            }
            Key::End => {
                self.cx = self.line_len();
                true
            }
            _ => false,
        }
    }

    fn command_key(&mut self, press: KeyPress) -> bool {
        match press.key {
            Key::Char('h') | Key::Left => {
                self.cx = self.cx.saturating_sub(1);
                true
            }
            Key::Char('l') | Key::Right => {
                self.cx = (self.cx + 1).min(self.line_len().saturating_sub(1));
                true
            }
            Key::Char('j') | Key::Down => {
                self.vertical(Key::Down);
                true
            }
            Key::Char('k') | Key::Up => {
                self.vertical(Key::Up);
                true
            }
            Key::Char('0') | Key::Home => {
                self.cx = 0;
                true
            }
            Key::Char('$') | Key::End => {
                self.cx = self.line_len().saturating_sub(1);
                true
            }
            Key::Char('i') => {
                self.snapshot();
                self.mode = Mode::Insert;
                true
            }
            Key::Char('a') => {
                self.snapshot();
                self.mode = Mode::Insert;
                self.cx = (self.cx + 1).min(self.line_len());
                true
            }
            Key::Char('A') => {
                self.snapshot();
                self.mode = Mode::Insert;
                self.cx = self.line_len();
                true
            }
            Key::Char('o') => {
                self.snapshot();
                self.lines.insert(self.cy + 1, String::new());
                self.cy += 1;
                self.cx = 0;
                self.mode = Mode::Insert;
                true
            }
            Key::Char('x') => {
                if self.line_len() > 0 {
                    self.snapshot();
                    let at = self.byte_at(self.cx);
                    self.lines[self.cy].remove(at);
                    self.clamp();
                }
                true
            }
            Key::Char('u') => {
                self.undo();
                true
            }
            // Getting text *out* of Dropkick without a file picker: yank the
            // buffer to the system clipboard and paste it wherever you like.
            Key::Char('y') => {
                self.yank = true;
                true
            }
            _ => false,
        }
    }

    fn vertical(&mut self, key: Key) {
        match key {
            Key::Up => self.cy = self.cy.saturating_sub(1),
            Key::Down => self.cy = (self.cy + 1).min(self.lines.len().saturating_sub(1)),
            _ => {}
        }
        self.clamp();
    }

    /// Take a pending yank request, if `y` was pressed since the last check.
    pub fn take_yank(&mut self) -> Option<String> {
        if self.yank {
            self.yank = false;
            Some(self.text())
        } else {
            None
        }
    }

    /// Esc in insert mode drops to command mode; the app layer decides what Esc
    /// means once there is no level left to drop to.
    pub fn leave_one_level(&mut self) -> bool {
        if self.mode == Mode::Insert {
            self.mode = Mode::Command;
            self.cx = self.cx.saturating_sub(1);
            self.clamp();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(n: &mut Scratchpad, s: &str) {
        for c in s.chars() {
            n.on_key(KeyPress::ch(c));
        }
    }

    #[test]
    fn insert_then_escape_returns_to_command() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        assert_eq!(n.mode, Mode::Insert);
        type_str(&mut n, "hello");
        assert_eq!(n.text(), "hello");
        assert!(n.leave_one_level());
        assert_eq!(n.mode, Mode::Command);
        assert!(!n.leave_one_level());
    }

    #[test]
    fn command_mode_letters_do_not_type() {
        let mut n = Scratchpad::new();
        type_str(&mut n, "hjkl");
        assert_eq!(n.text(), "");
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "abcd");
        n.on_key(KeyPress::plain(Key::Left));
        n.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(n.lines, vec!["abc".to_string(), "d".to_string()]);
        n.on_key(KeyPress::plain(Key::Backspace));
        assert_eq!(n.text(), "abcd");
    }

    #[test]
    fn undo_reverses_a_whole_insert_burst() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "don't forget migration 37");
        n.leave_one_level();
        assert!(n.text().starts_with("don't forget"));
        n.on_key(KeyPress::ch('u'));
        assert_eq!(n.text(), "");
    }

    #[test]
    fn undo_recovers_a_stray_keystroke() {
        // The panel takes focus ~11ms before it paints, so a keystroke can land
        // in a tool the user has not seen yet. That must be recoverable.
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "real note");
        n.leave_one_level();
        n.on_key(KeyPress::ch('x')); // stray deletion
        assert_ne!(n.text(), "real note");
        n.on_key(KeyPress::ch('u'));
        assert_eq!(n.text(), "real note");
    }

    #[test]
    fn undo_on_empty_stack_is_harmless() {
        let mut n = Scratchpad::new();
        assert!(!n.undo());
        assert_eq!(n.text(), "");
    }

    #[test]
    fn o_opens_a_line_below_and_inserts() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "first");
        n.leave_one_level();
        n.on_key(KeyPress::ch('o'));
        assert_eq!(n.mode, Mode::Insert);
        type_str(&mut n, "second");
        assert_eq!(n.text(), "first\nsecond");
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "café — naïve");
        n.on_key(KeyPress::plain(Key::Backspace));
        n.on_key(KeyPress::plain(Key::Left));
        n.on_key(KeyPress::ch('é'));
        assert!(n.text().contains("caf"));
    }

    #[test]
    fn y_yanks_the_whole_buffer_once() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "call Frank");
        n.leave_one_level();
        assert!(n.take_yank().is_none());
        n.on_key(KeyPress::ch('y'));
        assert_eq!(n.take_yank().as_deref(), Some("call Frank"));
        assert!(n.take_yank().is_none(), "a yank must not repeat");
    }

    #[test]
    fn y_in_insert_mode_is_just_a_letter() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "yes");
        assert!(n.take_yank().is_none());
        assert_eq!(n.text(), "yes");
    }

    #[test]
    fn alt_chords_are_never_consumed_as_text() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        assert!(!n.on_key(KeyPress::alt('q')));
        assert_eq!(n.text(), "");
    }
}
