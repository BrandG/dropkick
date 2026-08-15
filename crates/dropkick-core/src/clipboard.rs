//! Clipboard history — the tool where a bug leaks secrets rather than losing a
//! note, so the rules are here from the first commit rather than retrofitted.
//!
//! Four of them, all tested below:
//!
//! 1. **Masked on arrival.** Dropkick resumes into the tool you left it in. If
//!    that was the clipboard, a summon would otherwise put your last twenty
//!    copied things on screen — in a meeting, on a shared display. Entries
//!    render masked until you press a key, and `Esc` still dismisses without
//!    ever revealing anything.
//! 2. **Password-manager hints are obeyed.** Password managers tag clipboard
//!    content with an `x-kde-passwordManagerHint` MIME type meaning "do not
//!    record this". Dropkick doesn't.
//! 3. **Session-only by default.** History lives in memory. Nothing reaches
//!    disk unless the user asks for it.
//! 4. **Capture can be paused, and history cleared, in one keystroke each.**
//!
//! The tool never touches the system clipboard itself: selecting an entry
//! raises a [`crate::app::Request`] for the platform layer to carry out. Core
//! stays free of Wayland, Win32 and everything else.

use crate::key::{Key, KeyPress};

/// The MIME type password managers attach to say "this is a secret".
pub const PASSWORD_HINT: &str = "x-kde-passwordManagerHint";

/// Entries beyond this are dropped oldest-first. Pinned entries never count
/// against it and are never evicted.
pub const DEFAULT_MAX: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub pinned: bool,
    /// Best-effort note of where the copy came from, derived from the offered
    /// MIME types (a Chromium copy carries `chromium/x-internal-source-...`).
    pub source: Option<String>,
}

impl Entry {
    /// One-line preview, collapsed and truncated for the list.
    pub fn preview(&self, width: usize) -> String {
        let flat: String = self
            .text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() <= width {
            flat
        } else {
            let head: String = flat.chars().take(width.saturating_sub(1)).collect();
            format!("{head}…")
        }
    }

    /// What the list shows before the user has revealed anything.
    pub fn masked(&self, width: usize) -> String {
        let n = self.text.chars().count().min(width.saturating_sub(8)).max(3);
        format!("{} ({} chars)", "•".repeat(n), self.text.chars().count())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Search,
}

#[derive(Debug)]
pub struct Clipboard {
    /// Newest first.
    entries: Vec<Entry>,
    pub selected: usize,
    pub mode: Mode,
    pub query: String,
    /// Masked until the user presses a key. Reset every time the tool is
    /// entered, not merely on first launch.
    pub revealed: bool,
    pub capture_paused: bool,
    pub max: usize,
}

impl Default for Clipboard {
    fn default() -> Self {
        Clipboard {
            entries: Vec::new(),
            selected: 0,
            mode: Mode::List,
            query: String::new(),
            revealed: false,
            capture_paused: false,
            max: DEFAULT_MAX,
        }
    }
}

/// What the tool wants the platform to do after a keypress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Key was not for us.
    Ignored,
    Redraw,
    /// Put this on the system clipboard, then get out of the way.
    Paste(String),
}

impl Clipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Indices of entries matching the current query, newest first.
    pub fn visible(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.entries.len()).collect();
        }
        let needle = self.query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.text.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.visible().get(self.selected).and_then(|i| self.entries.get(*i))
    }

    /// Called when the platform observes a new selection.
    ///
    /// `mimes` is the full offered type list; it is inspected for the password
    /// manager hint and for a source note. Returns true if the entry was kept.
    pub fn record(&mut self, text: &str, mimes: &[String]) -> bool {
        if self.capture_paused {
            return false;
        }
        if text.trim().is_empty() {
            return false;
        }
        // Rule 2: a password manager asked us not to remember this.
        if mimes.iter().any(|m| m.contains(PASSWORD_HINT)) {
            return false;
        }
        // Re-copying the same thing should not fill the list with duplicates.
        if let Some(existing) = self.entries.iter().position(|e| e.text == text) {
            let entry = self.entries.remove(existing);
            self.entries.insert(0, entry);
            self.clamp();
            return true;
        }

        let source = mimes
            .iter()
            .find(|m| m.contains("x-internal-source") || m.contains("x-special"))
            .and_then(|m| m.split(['/', '-']).next().map(str::to_string));

        self.entries.insert(0, Entry { text: text.to_string(), pinned: false, source });
        self.evict();
        self.clamp();
        true
    }

    /// Drop the oldest unpinned entries once over the cap.
    fn evict(&mut self) {
        let mut unpinned = self.entries.iter().filter(|e| !e.pinned).count();
        while unpinned > self.max {
            if let Some(pos) = self.entries.iter().rposition(|e| !e.pinned) {
                self.entries.remove(pos);
                unpinned -= 1;
            } else {
                break;
            }
        }
    }

    fn clamp(&mut self) {
        let n = self.visible().len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    /// Called whenever the tool becomes the active tab. Re-masks: coming back
    /// to the clipboard is exactly the moment the contents should not be on
    /// screen already.
    pub fn on_enter(&mut self) {
        self.revealed = false;
        self.mode = Mode::List;
        self.query.clear();
        self.selected = 0;
    }

    /// Drop everything unpinned. Pinned entries survive: clearing is for the
    /// accidental paste of something sensitive, not for the things you chose to
    /// keep.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
        self.clamp();
    }

    /// Wipe everything including pins. The panic key.
    pub fn nuke(&mut self) {
        self.entries.clear();
        self.selected = 0;
        self.query.clear();
        self.mode = Mode::List;
    }

    pub fn on_key(&mut self, press: KeyPress) -> Outcome {
        if !press.mods.is_plain() {
            return Outcome::Ignored;
        }

        // Rule 1. Any key except Esc reveals, and is consumed doing so, so a
        // masked list cannot be navigated, pasted from or deleted from blind.
        // Esc is routed by the app layer and never arrives here.
        if !self.revealed {
            self.revealed = true;
            return Outcome::Redraw;
        }

        match self.mode {
            Mode::Search => self.search_key(press),
            Mode::List => self.list_key(press),
        }
    }

    fn search_key(&mut self, press: KeyPress) -> Outcome {
        match press.key {
            Key::Char(c) => {
                self.query.push(c);
                self.selected = 0;
                Outcome::Redraw
            }
            Key::Backspace => {
                self.query.pop();
                self.selected = 0;
                Outcome::Redraw
            }
            Key::Enter => {
                self.mode = Mode::List;
                Outcome::Redraw
            }
            Key::Up | Key::Down => self.list_key(press),
            _ => Outcome::Ignored,
        }
    }

    fn list_key(&mut self, press: KeyPress) -> Outcome {
        let count = self.visible().len();
        match press.key {
            Key::Char('j') | Key::Down => {
                if count > 0 {
                    self.selected = (self.selected + 1).min(count - 1);
                }
                Outcome::Redraw
            }
            Key::Char('k') | Key::Up => {
                self.selected = self.selected.saturating_sub(1);
                Outcome::Redraw
            }
            Key::Home => {
                self.selected = 0;
                Outcome::Redraw
            }
            Key::End => {
                self.selected = count.saturating_sub(1);
                Outcome::Redraw
            }
            Key::Char('/') => {
                self.mode = Mode::Search;
                self.query.clear();
                self.selected = 0;
                Outcome::Redraw
            }
            Key::Enter | Key::Char('y') => match self.selected_entry() {
                Some(e) => Outcome::Paste(e.text.clone()),
                None => Outcome::Ignored,
            },
            Key::Char('p') => {
                if let Some(&i) = self.visible().get(self.selected) {
                    self.entries[i].pinned = !self.entries[i].pinned;
                }
                Outcome::Redraw
            }
            Key::Char('d') | Key::Delete => {
                if let Some(&i) = self.visible().get(self.selected) {
                    self.entries.remove(i);
                    self.clamp();
                }
                Outcome::Redraw
            }
            Key::Char('x') => {
                self.clear();
                Outcome::Redraw
            }
            Key::Char('c') => {
                self.capture_paused = !self.capture_paused;
                Outcome::Redraw
            }
            Key::Char('m') => {
                self.revealed = false;
                Outcome::Redraw
            }
            _ => Outcome::Ignored,
        }
    }

    /// Search is a level in the Esc ladder: leave it before leaving the tool.
    pub fn leave_one_level(&mut self) -> bool {
        if self.mode == Mode::Search {
            self.mode = Mode::List;
            self.query.clear();
            self.selected = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(c: char) -> KeyPress {
        KeyPress::ch(c)
    }

    fn revealed_clipboard() -> Clipboard {
        let mut c = Clipboard::new();
        c.record("first", &[]);
        c.record("second", &[]);
        c.revealed = true;
        c
    }

    // --- rule 2: password manager hints -----------------------------------

    #[test]
    fn password_manager_hint_is_never_recorded() {
        let mut c = Clipboard::new();
        let mimes = vec![
            "text/plain".to_string(),
            format!("{PASSWORD_HINT}"),
        ];
        assert!(!c.record("hunter2", &mimes));
        assert!(c.is_empty());
    }

    #[test]
    fn hint_is_matched_even_with_a_value_suffix() {
        let mut c = Clipboard::new();
        let mimes = vec![format!("{PASSWORD_HINT}: secret")];
        assert!(!c.record("hunter2", &mimes));
        assert!(c.is_empty());
    }

    #[test]
    fn ordinary_copies_are_recorded() {
        let mut c = Clipboard::new();
        assert!(c.record("just some text", &["text/plain".to_string()]));
        assert_eq!(c.entries().len(), 1);
    }

    // --- rule 1: masked resume --------------------------------------------

    #[test]
    fn entries_start_masked_and_the_mask_hides_the_text() {
        let mut c = Clipboard::new();
        c.record("sekrit token abc123", &[]);
        assert!(!c.revealed);
        let shown = c.entries()[0].masked(40);
        assert!(!shown.contains("sekrit"));
        assert!(!shown.contains("abc123"));
    }

    #[test]
    fn first_key_only_reveals_and_does_nothing_else() {
        let mut c = revealed_clipboard();
        c.on_enter();
        assert!(!c.revealed);
        // 'd' would delete if it got through.
        assert_eq!(c.on_key(plain('d')), Outcome::Redraw);
        assert!(c.revealed);
        assert_eq!(c.entries().len(), 2, "the masking keypress must not delete");
    }

    #[test]
    fn a_masked_list_cannot_be_pasted_from() {
        let mut c = revealed_clipboard();
        c.on_enter();
        // Enter would paste the selection if it got through.
        assert_eq!(c.on_key(KeyPress::plain(Key::Enter)), Outcome::Redraw);
        assert!(c.revealed);
    }

    #[test]
    fn re_entering_the_tool_masks_again() {
        let mut c = revealed_clipboard();
        assert!(c.revealed);
        c.on_enter();
        assert!(!c.revealed, "switching away and back must not leave it revealed");
    }

    #[test]
    fn m_re_masks_on_demand() {
        let mut c = revealed_clipboard();
        c.on_key(plain('m'));
        assert!(!c.revealed);
    }

    // --- navigation, search, pinning --------------------------------------

    #[test]
    fn newest_entry_is_first_and_selected() {
        let c = revealed_clipboard();
        assert_eq!(c.entries()[0].text, "second");
        assert_eq!(c.selected_entry().unwrap().text, "second");
    }

    #[test]
    fn recopying_moves_an_entry_up_rather_than_duplicating() {
        let mut c = revealed_clipboard();
        c.record("first", &[]);
        assert_eq!(c.entries().len(), 2);
        assert_eq!(c.entries()[0].text, "first");
    }

    #[test]
    fn enter_pastes_the_selection() {
        let mut c = revealed_clipboard();
        c.on_key(plain('j'));
        assert_eq!(c.on_key(KeyPress::plain(Key::Enter)), Outcome::Paste("first".into()));
    }

    #[test]
    fn search_filters_and_survives_selection() {
        let mut c = Clipboard::new();
        c.record("postgres connection string", &[]);
        c.record("unrelated note", &[]);
        c.record("postgres backup command", &[]);
        c.revealed = true;

        c.on_key(plain('/'));
        for ch in "postgres".chars() {
            c.on_key(plain(ch));
        }
        assert_eq!(c.visible().len(), 2);
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::List);
        assert!(c.selected_entry().unwrap().text.contains("postgres"));
    }

    #[test]
    fn search_is_a_level_in_the_esc_ladder() {
        let mut c = revealed_clipboard();
        c.on_key(plain('/'));
        assert_eq!(c.mode, Mode::Search);
        assert!(c.leave_one_level());
        assert_eq!(c.mode, Mode::List);
        assert!(!c.leave_one_level(), "list level is the app's to dismiss");
    }

    #[test]
    fn deleting_removes_only_the_selection() {
        let mut c = revealed_clipboard();
        c.on_key(plain('d'));
        assert_eq!(c.entries().len(), 1);
        assert_eq!(c.entries()[0].text, "first");
    }

    // --- retention ---------------------------------------------------------

    #[test]
    fn clear_keeps_pinned_entries() {
        let mut c = revealed_clipboard();
        c.on_key(plain('p')); // pin "second"
        c.on_key(plain('x'));
        assert_eq!(c.entries().len(), 1);
        assert!(c.entries()[0].pinned);
    }

    #[test]
    fn nuke_takes_everything_including_pins() {
        let mut c = revealed_clipboard();
        c.on_key(plain('p'));
        c.nuke();
        assert!(c.is_empty());
    }

    #[test]
    fn history_is_capped_and_pins_are_never_evicted() {
        let mut c = Clipboard::new();
        c.max = 5;
        c.record("keep me", &[]);
        c.revealed = true;
        c.on_key(plain('p'));
        for i in 0..50 {
            c.record(&format!("entry {i}"), &[]);
        }
        assert!(c.entries().iter().any(|e| e.text == "keep me"));
        assert_eq!(c.entries().iter().filter(|e| !e.pinned).count(), 5);
    }

    #[test]
    fn paused_capture_records_nothing() {
        let mut c = revealed_clipboard();
        c.on_key(plain('c'));
        assert!(c.capture_paused);
        assert!(!c.record("should not appear", &[]));
        assert_eq!(c.entries().len(), 2);
    }

    #[test]
    fn empty_and_whitespace_copies_are_ignored() {
        let mut c = Clipboard::new();
        assert!(!c.record("", &[]));
        assert!(!c.record("   \n\t ", &[]));
        assert!(c.is_empty());
    }

    #[test]
    fn alt_chords_are_never_consumed() {
        let mut c = revealed_clipboard();
        assert_eq!(c.on_key(KeyPress::alt('q')), Outcome::Ignored);
    }

    #[test]
    fn preview_collapses_newlines_and_truncates() {
        let e = Entry {
            text: "line one\nline two\n\nline three".into(),
            pinned: false,
            source: None,
        };
        let p = e.preview(20);
        assert!(!p.contains('\n'));
        assert!(p.chars().count() <= 20);
    }

    #[test]
    fn operations_on_an_empty_history_do_not_panic() {
        let mut c = Clipboard::new();
        c.revealed = true;
        for k in ['j', 'k', 'd', 'p', 'x', 'y', '/'] {
            c.on_key(plain(k));
        }
        assert!(c.is_empty());
    }
}
