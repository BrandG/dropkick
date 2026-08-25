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
    /// Selecting by character, from the anchor to the cursor inclusive.
    Visual,
    /// Selecting whole lines, from the anchor's line to the cursor's.
    VisualLine,
}

impl Mode {
    /// Command and both visual modes sit *on* a character; insert sits after
    /// one. Everything that clamps or draws a cursor cares about this and not
    /// about which of the three it is.
    fn is_command_like(self) -> bool {
        !matches!(self, Mode::Insert)
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine)
    }
}

/// What a motion key means, independent of whether it is moving the cursor,
/// extending a selection, or giving an operator its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    FirstLine,
    LastLine,
    /// The doubled operator — the `dd` in `dd`, the `yy` in `yy`.
    WholeLine,
}

/// An inclusive range over the buffer, in logical (line, character) positions.
/// `linewise` ranges take whole lines and ignore the character columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub lo: (usize, usize),
    pub hi: (usize, usize),
    pub linewise: bool,
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
    /// Text waiting to be handed to the platform's clipboard.
    yank: Option<String>,
    /// An operator waiting for a motion — the `d` of `dd`, the `y` of `yG`.
    /// `None` the rest of the time, which is nearly always.
    pending: Option<char>,
    /// A lone `g` waiting to see whether it is the first half of `gg`.
    pending_g: bool,
    /// Where the current selection started. `Some` exactly while a visual mode
    /// is active.
    anchor: Option<(usize, usize)>,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Scratchpad {
            lines: vec![String::new()],
            cx: 0,
            cy: 0,
            mode: Mode::Command,
            undo: Vec::new(),
            yank: None,
            pending: None,
            pending_g: false,
            anchor: None,
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

    /// Replace the buffer with a file's contents.
    ///
    /// Undo history is dropped: there is nothing sensible to undo *to* from
    /// text that was typed in a previous run, and offering an undo that jumps
    /// to a blank buffer would be worse than offering none.
    pub fn load(&mut self, text: &str) {
        self.lines = text.lines().map(str::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cx = 0;
        self.cy = 0;
        self.mode = Mode::Command;
        self.anchor = None;
        self.pending = None;
        self.pending_g = false;
        self.undo.clear();
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
        let max = if self.mode.is_command_like() {
            self.line_len().saturating_sub(1)
        } else {
            self.line_len()
        };
        self.cx = self.cx.min(max);
        // The anchor is a buffer position too, and the buffer may have shrunk
        // under it. A selection anchored past the end would panic on slice.
        if let Some((ay, ax)) = self.anchor {
            let ay = ay.min(self.lines.len() - 1);
            let alen = self.lines[ay].chars().count();
            self.anchor = Some((ay, ax.min(alen.saturating_sub(1))));
        }
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
            _ => self.command_key(press),
        }
    }

    /// The public selection range, for the renderer to highlight. Inclusive at
    /// both ends. `None` unless a visual mode is active.
    ///
    /// Returned in logical (line, char) coordinates rather than screen rows on
    /// purpose: the renderer wraps lines for display, and a selection expressed
    /// in buffer positions highlights correctly across a wrap without either
    /// side knowing about the other.
    pub fn selection(&self) -> Option<Span> {
        self.visual_span()
    }

    fn visual_span(&self) -> Option<Span> {
        let anchor = self.anchor?;
        let cursor = (self.cy, self.cx);
        let (lo, hi) = if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) };
        Some(Span { lo, hi, linewise: self.mode == Mode::VisualLine })
    }

    /// Where a motion lands the cursor. `None` when it cannot move at all,
    /// which is how `dj` on the last line comes to do nothing rather than
    /// quietly deleting something else.
    fn motion_target(&self, m: Motion) -> Option<(usize, usize)> {
        let last = self.lines.len() - 1;
        match m {
            Motion::Left => (self.cx > 0).then(|| (self.cy, self.cx - 1)),
            Motion::Right => {
                (self.cx + 1 < self.line_len()).then(|| (self.cy, self.cx + 1))
            }
            Motion::Up => (self.cy > 0).then(|| (self.cy - 1, self.cx)),
            Motion::Down => (self.cy < last).then(|| (self.cy + 1, self.cx)),
            Motion::LineStart => Some((self.cy, 0)),
            Motion::LineEnd => Some((self.cy, self.line_len().saturating_sub(1))),
            Motion::FirstLine => Some((0, 0)),
            Motion::LastLine => Some((last, 0)),
            Motion::WholeLine => Some((self.cy, self.cx)),
        }
    }

    fn move_cursor(&mut self, m: Motion) {
        if let Some((y, x)) = self.motion_target(m) {
            self.cy = y;
            // Vertical motions keep the column; horizontal ones set it.
            self.cx = match m {
                Motion::Up | Motion::Down => self.cx,
                Motion::FirstLine | Motion::LastLine => 0,
                _ => x,
            };
            self.clamp();
        }
    }

    /// The range an operator covers when given this motion.
    ///
    /// Each arm is written out rather than derived from a general
    /// inclusive/exclusive rule. vi's motion algebra is subtle enough that a
    /// clever derivation would be wrong in a way nobody would notice until it
    /// ate a character, and there are only nine motions here.
    fn operator_span(&self, m: Motion) -> Option<Span> {
        let cur = (self.cy, self.cx);
        let line = |y: usize| Span { lo: (y, 0), hi: (y, 0), linewise: true };
        match m {
            // dd / yy: this line.
            Motion::WholeLine => Some(line(self.cy)),
            // Linewise, and only if the motion can actually move.
            Motion::Down => {
                let (y, _) = self.motion_target(m)?;
                Some(Span { lo: (self.cy, 0), hi: (y, 0), linewise: true })
            }
            Motion::Up => {
                let (y, _) = self.motion_target(m)?;
                Some(Span { lo: (y, 0), hi: (self.cy, 0), linewise: true })
            }
            Motion::FirstLine => Some(Span { lo: (0, 0), hi: (self.cy, 0), linewise: true }),
            Motion::LastLine => Some(Span {
                lo: (self.cy, 0),
                hi: (self.lines.len() - 1, 0),
                linewise: true,
            }),
            // d$ takes the cursor character through the end of the line.
            Motion::LineEnd => (self.line_len() > 0).then(|| Span {
                lo: cur,
                hi: (self.cy, self.line_len() - 1),
                linewise: false,
            }),
            // d0 stops *before* the cursor: vi does not delete the character
            // you are sitting on when moving to the start of the line.
            Motion::LineStart => (self.cx > 0).then(|| Span {
                lo: (self.cy, 0),
                hi: (self.cy, self.cx - 1),
                linewise: false,
            }),
            Motion::Right => (self.line_len() > 0).then_some(Span {
                lo: cur,
                hi: cur,
                linewise: false,
            }),
            Motion::Left => (self.cx > 0).then(|| Span {
                lo: (self.cy, self.cx - 1),
                hi: (self.cy, self.cx - 1),
                linewise: false,
            }),
        }
    }

    fn span_text(&self, span: Span) -> String {
        if span.linewise {
            return self.lines[span.lo.0..=span.hi.0].join("\n");
        }
        let chars = |y: usize| -> Vec<char> { self.lines[y].chars().collect() };
        if span.lo.0 == span.hi.0 {
            let c = chars(span.lo.0);
            return c[span.lo.1..=span.hi.1.min(c.len().saturating_sub(1))]
                .iter()
                .collect();
        }
        let first = chars(span.lo.0);
        let mut out = vec![first[span.lo.1.min(first.len())..].iter().collect::<String>()];
        for y in (span.lo.0 + 1)..span.hi.0 {
            out.push(self.lines[y].clone());
        }
        let lastc = chars(span.hi.0);
        out.push(lastc[..=span.hi.1.min(lastc.len().saturating_sub(1))].iter().collect());
        out.join("\n")
    }

    fn delete_span(&mut self, span: Span) {
        self.snapshot();
        if span.linewise {
            self.lines.drain(span.lo.0..=span.hi.0);
            self.cy = span.lo.0;
            self.cx = 0;
        } else if span.lo.0 == span.hi.0 {
            let mut c: Vec<char> = self.lines[span.lo.0].chars().collect();
            let hi = span.hi.1.min(c.len().saturating_sub(1));
            c.drain(span.lo.1..=hi);
            self.lines[span.lo.0] = c.into_iter().collect();
            self.cy = span.lo.0;
            self.cx = span.lo.1;
        } else {
            let head: String = self.lines[span.lo.0].chars().take(span.lo.1).collect();
            let tailc: Vec<char> = self.lines[span.hi.0].chars().collect();
            let tail: String = tailc.iter().skip(span.hi.1 + 1).collect();
            self.lines.drain(span.lo.0..=span.hi.0);
            self.lines.insert(span.lo.0, head + &tail);
            self.cy = span.lo.0;
            self.cx = span.lo.1;
        }
        // The buffer is never empty. There would be nowhere to put the cursor,
        // and every other method here indexes `lines[cy]` assuming a line
        // exists. Wiping everything leaves one blank line, as vi does.
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.clamp();
    }

    /// Run a pending operator over a motion, then drop back to command mode.
    fn apply_operator(&mut self, op: char, m: Motion) {
        let Some(span) = self.operator_span(m) else { return };
        match op {
            'd' => self.delete_span(span),
            'y' => self.yank = Some(self.span_text(span)),
            _ => {}
        }
    }

    /// A motion key, wherever it came from: extending a selection, feeding a
    /// pending operator, or just moving.
    fn do_motion(&mut self, m: Motion) -> bool {
        match self.pending.take() {
            Some(op) => self.apply_operator(op, m),
            None => self.move_cursor(m),
        }
        true
    }

    /// `d` and `y` in a visual mode act on the selection immediately — there is
    /// no motion to wait for, the selection *is* the range.
    fn apply_to_selection(&mut self, op: char) -> bool {
        let Some(span) = self.visual_span() else { return false };
        match op {
            'd' => self.delete_span(span),
            'y' => self.yank = Some(self.span_text(span)),
            _ => {}
        }
        self.anchor = None;
        self.mode = Mode::Command;
        self.clamp();
        true
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
        // A lone `g` is waiting to see if it is the first half of `gg`. It
        // resolves before anything else, including a pending operator, because
        // `dgg` is `d` waiting on a motion that is itself two keys.
        if self.pending_g {
            self.pending_g = false;
            if press.key == Key::Char('g') {
                return self.do_motion(Motion::FirstLine);
            }
            // `g` followed by something that is not `g` is not a motion, so any
            // operator waiting on it is abandoned too.
            self.pending = None;
            return true;
        }

        // An operator is pending and this key must be a motion. Anything else
        // aborts, as vi does. Aborting must not *also* run the key: a mistyped
        // `dz` that both cancelled and moved would leave the cursor somewhere
        // the user did not ask for, and the next `dd` would delete the wrong
        // line.
        if let Some(op) = self.pending {
            if press.key == Key::Char('g') {
                self.pending_g = true;
                return true;
            }
            // The doubled form: `dd`, `yy`.
            if press.key == Key::Char(op) {
                self.pending = None;
                self.apply_operator(op, Motion::WholeLine);
                return true;
            }
            return match Self::motion_for(press.key) {
                Some(m) => self.do_motion(m),
                None => {
                    self.pending = None;
                    true
                }
            };
        }

        // In a visual mode the operators act on the selection at once.
        if self.mode.is_visual() {
            match press.key {
                Key::Char('d') | Key::Char('x') => return self.apply_to_selection('d'),
                Key::Char('y') => return self.apply_to_selection('y'),
                _ => {}
            }
        }

        if let Some(m) = Self::motion_for(press.key) {
            return self.do_motion(m);
        }

        match press.key {
            Key::Char('g') => {
                self.pending_g = true;
                true
            }
            // `v` and `V` toggle: pressing the same one again cancels, and
            // pressing the other switches between character and line without
            // losing the anchor, both as vi does.
            Key::Char('v') | Key::Char('V') => {
                let want =
                    if press.key == Key::Char('v') { Mode::Visual } else { Mode::VisualLine };
                if self.mode == want {
                    self.anchor = None;
                    self.mode = Mode::Command;
                } else {
                    if self.anchor.is_none() {
                        self.anchor = Some((self.cy, self.cx));
                    }
                    self.mode = want;
                }
                true
            }
            Key::Char('i') => {
                self.enter_insert();
                true
            }
            Key::Char('a') => {
                self.enter_insert();
                self.cx = (self.cx + 1).min(self.line_len());
                true
            }
            Key::Char('A') => {
                self.enter_insert();
                self.cx = self.line_len();
                true
            }
            Key::Char('o') => {
                self.enter_insert();
                self.lines.insert(self.cy + 1, String::new());
                self.cy += 1;
                self.cx = 0;
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
            // Operators. Both wait for a motion; `dd` and `yy` are the doubled
            // forms. `y` used to yank the whole buffer on its own — it follows
            // vi now, so yanking everything is `ggyG`.
            Key::Char('d') | Key::Char('y') => {
                let Key::Char(c) = press.key else { unreachable!() };
                self.pending = Some(c);
                true
            }
            // `Y` yanks the whole buffer. This is the one deliberate departure
            // from vi in the Scratchpad, chosen by the user: `ggyG` is a lot of
            // keys for the thing this tool is most often used for, which is
            // writing something down and taking it elsewhere.
            //
            // vi spends `Y` on `yy`. The cost of taking it is that a vi user
            // pressing `Y` for one line gets everything, and finds out when
            // they paste. It is in the hint line for exactly that reason —
            // unlike the vi-standard keys, this one cannot be guessed.
            Key::Char('Y') => {
                self.yank = Some(self.text());
                true
            }
            Key::Char('u') => {
                self.undo();
                true
            }
            _ => false,
        }
    }

    /// Motions usable from command mode, in a visual mode, and as the second
    /// half of an operator. One table, so `l`, `dl` and `vl` cannot drift.
    fn motion_for(key: Key) -> Option<Motion> {
        Some(match key {
            Key::Char('h') | Key::Left => Motion::Left,
            Key::Char('l') | Key::Right => Motion::Right,
            Key::Char('j') | Key::Down => Motion::Down,
            Key::Char('k') | Key::Up => Motion::Up,
            Key::Char('0') | Key::Home => Motion::LineStart,
            Key::Char('$') | Key::End => Motion::LineEnd,
            Key::Char('G') => Motion::LastLine,
            _ => return None,
        })
    }

    fn enter_insert(&mut self) {
        self.snapshot();
        // Insert mode has no selection. Leaving the anchor set would resume a
        // stale one the next time a visual mode was entered.
        self.anchor = None;
        self.mode = Mode::Insert;
    }

    /// Switching tools or re-summoning must not leave an operator half-typed:
    /// a `d` pressed before you switched away would silently eat the next `d`
    /// you typed on the way back in. A selection is dropped for the same
    /// reason — coming back to a highlight you did not remember making, with
    /// `d` armed to delete it, is the worst of the available surprises.
    pub fn on_enter(&mut self) {
        self.pending = None;
        self.pending_g = false;
        if self.mode.is_visual() {
            self.mode = Mode::Command;
        }
        self.anchor = None;
    }

    fn vertical(&mut self, key: Key) {
        match key {
            Key::Up => self.cy = self.cy.saturating_sub(1),
            Key::Down => self.cy = (self.cy + 1).min(self.lines.len().saturating_sub(1)),
            _ => {}
        }
        self.clamp();
    }

    /// Take the text a yank produced, if one happened since the last check.
    pub fn take_yank(&mut self) -> Option<String> {
        self.yank.take()
    }

    /// Esc in insert mode drops to command mode; the app layer decides what Esc
    /// means once there is no level left to drop to.
    pub fn leave_one_level(&mut self) -> bool {
        // A half-typed operator is the innermost level. Esc after `d` aborts
        // the `d` and nothing else — it must not also drop a mode or dismiss
        // the panel. Esc never reaches `command_key`, so this cannot be left
        // to the abort path there.
        if self.pending.take().is_some() || std::mem::take(&mut self.pending_g) {
            return true;
        }
        // Then the selection, which is the next rung out.
        if self.mode.is_visual() {
            self.anchor = None;
            self.mode = Mode::Command;
            self.clamp();
            return true;
        }
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
    fn text_survives_a_round_trip_through_a_file() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "call Frank");
        n.on_key(KeyPress::plain(Key::Enter));
        type_str(&mut n, "  indented, and trailing spaces   ");
        let before = n.text();

        let mut back = Scratchpad::new();
        back.load(&before);
        assert_eq!(back.text(), before, "what was typed is what comes back");
    }

    #[test]
    fn loading_leaves_a_usable_buffer_whatever_the_file_held() {
        for text in ["", "\n", "one\ntwo\n", "no trailing newline", "\n\n\n"] {
            let mut n = Scratchpad::new();
            n.load(text);
            assert!(!n.lines.is_empty(), "{text:?} produced an empty buffer");
            assert_eq!(n.mode, Mode::Command);
            assert_eq!((n.cx, n.cy), (0, 0));
            // And it must be safe to type into immediately.
            n.on_key(KeyPress::ch('i'));
            n.on_key(KeyPress::ch('x'));
        }
    }

    #[test]
    fn loading_drops_undo_history() {
        // Undoing to a blank buffer you never saw would be worse than having
        // no undo at all.
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "typed this run");
        n.leave_one_level();
        n.load("from the file");
        assert!(!n.undo(), "undo reached back past the load");
        assert_eq!(n.text(), "from the file");
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

    /// Build a three-line buffer without going through insert mode.
    fn three_lines() -> Scratchpad {
        let mut n = Scratchpad::new();
        n.lines = vec!["one".into(), "two".into(), "three".into()];
        n
    }

    #[test]
    fn dd_deletes_the_current_line() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('j'));
        type_str(&mut n, "dd");
        assert_eq!(n.text(), "one\nthree");
        assert_eq!(n.cy, 1, "cursor lands on the line that moved up");
    }

    #[test]
    fn single_d_alone_changes_nothing() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('d'));
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn d_then_a_non_motion_aborts_without_running_it() {
        let mut n = three_lines();
        // `z` is not a motion. vi aborts; the danger is aborting and *also*
        // running the key, which would leave the next dd on the wrong line.
        type_str(&mut n, "dz");
        assert_eq!(n.text(), "one\ntwo\nthree");
        assert_eq!(n.cy, 0, "the aborting key must not also move the cursor");
        // And the operator is genuinely gone, not still pending.
        type_str(&mut n, "d");
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn dj_deletes_this_line_and_the_next() {
        let mut n = three_lines();
        type_str(&mut n, "dj");
        assert_eq!(n.text(), "three", "j is linewise, so dj takes both lines");
    }

    #[test]
    fn dj_on_the_last_line_does_nothing() {
        let mut n = three_lines();
        // vi refuses a motion that cannot move rather than falling back to
        // something smaller. Silently deleting just this line would be the
        // wrong-thing-quietly failure.
        type_str(&mut n, "jjdj");
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn esc_aborts_a_pending_operator_and_nothing_else() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('d'));
        assert!(n.leave_one_level(), "esc consumes the pending d");
        assert_eq!(n.text(), "one\ntwo\nthree");
        // With nothing pending and command mode already active, the ladder is
        // out of rungs and the app layer takes over.
        assert!(!n.leave_one_level());
    }

    #[test]
    fn dd_on_the_last_remaining_line_leaves_one_blank_line() {
        let mut n = Scratchpad::new();
        n.lines = vec!["only".into()];
        type_str(&mut n, "dd");
        assert_eq!(n.lines.len(), 1, "the buffer is never empty");
        assert_eq!(n.text(), "");
        assert_eq!(n.cy, 0);
        assert_eq!(n.cx, 0);
    }

    #[test]
    fn dd_at_the_bottom_pulls_the_cursor_up() {
        let mut n = three_lines();
        type_str(&mut n, "jjdd");
        assert_eq!(n.text(), "one\ntwo");
        assert_eq!(n.cy, 1, "cy cannot dangle past the end");
    }

    #[test]
    fn dd_is_undone_by_u_in_one_step() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('j'));
        type_str(&mut n, "dd");
        assert_eq!(n.text(), "one\nthree");
        n.on_key(KeyPress::ch('u'));
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn repeated_dd_clears_the_buffer() {
        let mut n = three_lines();
        for _ in 0..3 {
            type_str(&mut n, "dd");
        }
        assert_eq!(n.text(), "");
        assert_eq!(n.lines.len(), 1);
    }

    #[test]
    fn v_selects_characters_and_d_deletes_them() {
        let mut n = three_lines();
        // On "one", select o-n with v then l, and cut.
        type_str(&mut n, "vld");
        assert_eq!(n.text(), "e\ntwo\nthree");
        assert_eq!(n.mode, Mode::Command, "the operator drops out of visual");
    }

    #[test]
    fn v_selection_is_inclusive_of_both_ends() {
        let mut n = three_lines();
        type_str(&mut n, "vl");
        assert_eq!(n.selection(), Some(Span { lo: (0, 0), hi: (0, 1), linewise: false }));
        type_str(&mut n, "y");
        assert_eq!(n.take_yank().as_deref(), Some("on"));
    }

    #[test]
    fn visual_selection_extends_backwards_too() {
        let mut n = three_lines();
        // Sit on the last character, select leftwards.
        type_str(&mut n, "$vh");
        assert_eq!(n.selection(), Some(Span { lo: (0, 1), hi: (0, 2), linewise: false }));
        type_str(&mut n, "y");
        assert_eq!(n.take_yank().as_deref(), Some("ne"));
    }

    #[test]
    fn charwise_selection_spans_lines() {
        let mut n = three_lines();
        type_str(&mut n, "lvjy");
        // From "n" on line one, through the same column on line two.
        assert_eq!(n.take_yank().as_deref(), Some("ne\ntw"));
    }

    #[test]
    fn charwise_delete_across_lines_joins_the_remainder() {
        let mut n = three_lines();
        type_str(&mut n, "lvjd");
        assert_eq!(n.text(), "oo\nthree", "the head and tail become one line");
    }

    #[test]
    fn capital_v_selects_whole_lines() {
        let mut n = three_lines();
        type_str(&mut n, "lVjy");
        assert_eq!(
            n.take_yank().as_deref(),
            Some("one\ntwo"),
            "linewise ignores the column the cursor sits in"
        );
    }

    #[test]
    fn v_and_capital_v_switch_without_losing_the_anchor() {
        let mut n = three_lines();
        type_str(&mut n, "vj");
        let before = n.selection().unwrap();
        n.on_key(KeyPress::ch('V'));
        let after = n.selection().unwrap();
        assert_eq!(n.mode, Mode::VisualLine);
        assert_eq!((before.lo, before.hi), (after.lo, after.hi), "same range");
        assert!(after.linewise, "but now linewise");
    }

    #[test]
    fn pressing_v_again_cancels_the_selection() {
        let mut n = three_lines();
        type_str(&mut n, "vl");
        assert!(n.selection().is_some());
        n.on_key(KeyPress::ch('v'));
        assert_eq!(n.mode, Mode::Command);
        assert!(n.selection().is_none());
    }

    #[test]
    fn esc_cancels_a_selection_before_leaving_the_tool() {
        let mut n = three_lines();
        type_str(&mut n, "vj");
        assert!(n.leave_one_level(), "esc eats the selection");
        assert!(n.selection().is_none());
        assert_eq!(n.text(), "one\ntwo\nthree", "cancelling deletes nothing");
        assert!(!n.leave_one_level(), "and now the ladder is out of rungs");
    }

    #[test]
    fn gg_and_g_jump_to_the_ends() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('G'));
        assert_eq!(n.cy, 2);
        type_str(&mut n, "gg");
        assert_eq!(n.cy, 0);
    }

    #[test]
    fn gg_y_g_yanks_everything_from_anywhere() {
        let mut n = three_lines();
        type_str(&mut n, "jl");
        type_str(&mut n, "ggyG");
        assert_eq!(n.take_yank().as_deref(), Some("one\ntwo\nthree"));
    }

    #[test]
    fn gg_d_g_wipes_the_buffer() {
        let mut n = three_lines();
        type_str(&mut n, "ggdG");
        assert_eq!(n.text(), "");
        assert_eq!(n.lines.len(), 1, "the buffer is never empty");
    }

    #[test]
    fn visual_wipe_is_undone_in_one_step() {
        let mut n = three_lines();
        type_str(&mut n, "ggVGd");
        assert_eq!(n.text(), "");
        n.on_key(KeyPress::ch('u'));
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn g_followed_by_junk_abandons_a_waiting_operator() {
        let mut n = three_lines();
        // `dgz` is nonsense; the danger is the `d` surviving to eat a later key.
        type_str(&mut n, "dgz");
        assert_eq!(n.text(), "one\ntwo\nthree");
        type_str(&mut n, "d");
        assert_eq!(n.text(), "one\ntwo\nthree", "no operator was left armed");
    }

    #[test]
    fn d_dollar_takes_the_cursor_character_through_the_end() {
        let mut n = three_lines();
        type_str(&mut n, "ld$");
        assert_eq!(n.text(), "o\ntwo\nthree");
    }

    #[test]
    fn d_zero_stops_before_the_cursor() {
        let mut n = three_lines();
        // vi does not delete the character you are sitting on when going to
        // the start of the line.
        type_str(&mut n, "lld0");
        assert_eq!(n.text(), "e\ntwo\nthree");
    }

    #[test]
    fn yank_does_not_fire_while_a_selection_is_merely_moving() {
        let mut n = three_lines();
        type_str(&mut n, "vjj");
        assert!(n.take_yank().is_none(), "only y yanks");
    }

    #[test]
    fn on_enter_drops_a_live_selection() {
        let mut n = three_lines();
        type_str(&mut n, "vj");
        n.on_enter();
        assert_eq!(n.mode, Mode::Command);
        assert!(n.selection().is_none());
        // The d that follows acts on the cursor's line -- which `vj` did move
        // to, legitimately -- and not on the two lines the selection covered.
        type_str(&mut n, "dd");
        assert_eq!(n.text(), "one\nthree");
    }

    #[test]
    fn insert_mode_clears_a_selection() {
        let mut n = three_lines();
        type_str(&mut n, "vj");
        n.on_key(KeyPress::ch('i'));
        assert!(n.selection().is_none(), "insert has no selection to resume");
    }

    #[test]
    fn deleting_a_selection_leaves_the_cursor_in_the_buffer() {
        let mut n = three_lines();
        // Select to the end and cut: the cursor must not dangle past the last
        // line, which is what the anchor clamp in `clamp` guards.
        type_str(&mut n, "GVd");
        assert_eq!(n.text(), "one\ntwo");
        assert!(n.cy < n.lines.len());
        assert!(n.cx <= n.lines[n.cy].chars().count());
    }

    #[test]
    fn on_enter_clears_a_half_typed_operator() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('d'));
        n.on_enter();
        // Were the `d` still pending, this one would complete it and delete.
        n.on_key(KeyPress::ch('d'));
        assert_eq!(n.text(), "one\ntwo\nthree");
    }

    #[test]
    fn d_does_not_type_in_insert_mode() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "dd");
        assert_eq!(n.text(), "dd", "d is an operator only in command mode");
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
    fn yy_yanks_the_line_once() {
        let mut n = Scratchpad::new();
        n.on_key(KeyPress::ch('i'));
        type_str(&mut n, "call Frank");
        n.leave_one_level();
        assert!(n.take_yank().is_none());
        // Bare `y` is an operator now: it waits for a motion and yanks nothing
        // on its own.
        n.on_key(KeyPress::ch('y'));
        assert!(n.take_yank().is_none(), "a lone y is still waiting for a motion");
        n.on_key(KeyPress::ch('y'));
        assert_eq!(n.take_yank().as_deref(), Some("call Frank"));
        assert!(n.take_yank().is_none(), "a yank must not repeat");
    }

    #[test]
    fn capital_y_yanks_the_whole_buffer_from_anywhere() {
        let mut n = three_lines();
        n.on_key(KeyPress::ch('j'));
        n.on_key(KeyPress::ch('Y'));
        assert_eq!(n.take_yank().as_deref(), Some("one\ntwo\nthree"));
        assert_eq!(n.text(), "one\ntwo\nthree", "Y copies, it does not cut");
    }

    #[test]
    fn capital_y_ignores_a_selection_rather_than_yanking_it() {
        let mut n = three_lines();
        // Y means the buffer, always. Quietly yanking the selection instead
        // would be the same key meaning two things.
        type_str(&mut n, "vj");
        n.on_key(KeyPress::ch('Y'));
        assert_eq!(n.take_yank().as_deref(), Some("one\ntwo\nthree"));
    }

    #[test]
    fn yanking_the_whole_buffer_is_gg_y_g() {
        let mut n = three_lines();
        type_str(&mut n, "jj");
        type_str(&mut n, "ggyG");
        assert_eq!(n.take_yank().as_deref(), Some("one\ntwo\nthree"));
        assert_eq!(n.text(), "one\ntwo\nthree", "a yank does not modify the buffer");
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
