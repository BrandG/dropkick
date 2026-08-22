//! Calendar: a month grid, and entries attached to days.
//!
//! Today's date is not read here. It arrives from the platform on summon, the
//! same way time does for the Timer — see [`crate::date`].
//!
//! ## The file
//!
//! One entry per line, in a form that is readable and greppable without
//! Dropkick, because "plain files in a directory you can open" is a stated rule
//! rather than an accident:
//!
//! ```text
//! 2026-08-24 15:00 Dentist
//! 2026-08-24 --:-- Call the landlord
//! ```
//!
//! `--:--` is an all-day entry. Dates are zero-padded ISO so that sorting the
//! file as text sorts it chronologically; anyone can sort it with `sort` and
//! not break it.
//!
//! Unreadable lines are **kept, not dropped**. A parse bug or a hand-edit typo
//! must not silently delete somebody's appointments on the next write.

use crate::date::{format_time, parse_time, Date, TimeParse};
use crate::key::{Key, KeyPress};

const ALL_DAY: &str = "--:--";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub date: Date,
    /// Minutes since midnight, or `None` for an all-day entry.
    pub time: Option<u32>,
    pub text: String,
}

impl Entry {
    fn sort_key(&self) -> (i64, u32, String) {
        // All-day entries sort to the top of their day: they are the ones with
        // no position within it.
        (self.date.to_days(), self.time.map_or(0, |t| t + 1), self.text.clone())
    }

    pub fn to_line(&self) -> String {
        let t = self.time.map_or(ALL_DAY.to_string(), format_time);
        format!("{} {} {}", self.date.to_iso(), t, self.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Moving around the month.
    Grid,
    /// Typing an entry's text.
    Text,
    /// Typing an entry's time, having already typed the text.
    Time,
    /// The typed time could be morning or afternoon and the user must say
    /// which. Carries the hour so both readings can be offered.
    Disambiguate(u32),
    /// The day's entries have focus, for deleting one.
    List,
}

#[derive(Debug)]
pub struct Calendar {
    entries: Vec<Entry>,
    /// Lines the parser did not understand, kept verbatim and written back out
    /// so that a bad line costs the user nothing.
    unparsed: Vec<String>,
    pub today: Date,
    pub focus: Date,
    pub mode: Mode,
    /// Text being typed for a new entry.
    pub draft_text: String,
    /// Time being typed for a new entry.
    pub draft_time: String,
    /// Which of the focused day's entries is selected in [`Mode::List`].
    pub selected: usize,
    pub problem: Option<&'static str>,
    /// Set on any change; the platform debounces and writes.
    pub dirty: bool,
}

impl Default for Calendar {
    fn default() -> Self {
        // A placeholder until the platform supplies the real date on summon.
        // Deliberately not "some arbitrary day" that could be mistaken for real:
        // the epoch is obviously wrong if it ever shows up on screen.
        let epoch = Date::new(1970, 1, 1);
        Calendar {
            entries: Vec::new(),
            unparsed: Vec::new(),
            today: epoch,
            focus: epoch,
            mode: Mode::Grid,
            draft_text: String::new(),
            draft_time: String::new(),
            selected: 0,
            problem: None,
            dirty: false,
        }
    }
}

impl Calendar {
    pub fn new() -> Self {
        Self::default()
    }

    /// The platform's answer to "what day is it", from `/etc/localtime`.
    ///
    /// Only moves the focus when the day actually changed, so that re-summoning
    /// does not yank you back to today while you are looking at next month.
    pub fn set_today(&mut self, today: Date) {
        if self.today == today {
            return;
        }
        let was_on_today = self.focus == self.today;
        self.today = today;
        if was_on_today {
            self.focus = today;
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn on(&self, date: Date) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.date == date).collect()
    }

    pub fn has_entries(&self, date: Date) -> bool {
        self.entries.iter().any(|e| e.date == date)
    }

    fn sort(&mut self) {
        self.entries.sort_by_key(|e| e.sort_key());
    }

    pub fn add(&mut self, date: Date, time: Option<u32>, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.entries.push(Entry { date, time, text: to_single_line(text) });
        self.sort();
        self.dirty = true;
    }

    /// Serialise for the platform to write. Unparsed lines go back out with
    /// everything else.
    pub fn serialise(&self) -> String {
        let mut out = String::from(
            "# Dropkick calendar. One entry per line: DATE TIME TEXT\n\
             # TIME is 24-hour, or --:-- for an all-day entry.\n",
        );
        for e in &self.entries {
            out.push_str(&e.to_line());
            out.push('\n');
        }
        for line in &self.unparsed {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Replace all state from a file's contents. Returns how many lines could
    /// not be understood, for the caller to mention rather than swallow.
    pub fn load(&mut self, text: &str) -> usize {
        self.entries.clear();
        self.unparsed.clear();
        for raw in text.lines() {
            let line = raw.trim_end();
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            match parse_line(line) {
                Some(e) => self.entries.push(e),
                None => self.unparsed.push(line.to_string()),
            }
        }
        self.sort();
        self.dirty = false;
        self.unparsed.len()
    }

    pub fn unparsed_count(&self) -> usize {
        self.unparsed.len()
    }

    fn move_focus(&mut self, days: i64) {
        self.focus = self.focus.add_days(days);
        self.selected = 0;
    }

    fn move_month(&mut self, months: i32) {
        self.focus = self.focus.add_months(months);
        self.selected = 0;
    }

    fn commit_entry(&mut self, time: Option<u32>) {
        let text = std::mem::take(&mut self.draft_text);
        self.add(self.focus, time, &text);
        self.draft_time.clear();
        self.mode = Mode::Grid;
        self.problem = None;
    }

    /// Esc: give up the innermost thing first. Returns false when there is
    /// nothing left to leave and the app should take over.
    pub fn leave_one_level(&mut self) -> bool {
        match self.mode {
            // Backing out of the time prompt returns to the text, so a typo in
            // the time does not throw away what was typed.
            Mode::Disambiguate(_) => {
                self.mode = Mode::Time;
                self.problem = None;
                true
            }
            Mode::Time => {
                self.mode = Mode::Text;
                self.problem = None;
                true
            }
            Mode::Text => {
                self.draft_text.clear();
                self.draft_time.clear();
                self.mode = Mode::Grid;
                self.problem = None;
                true
            }
            Mode::List => {
                self.mode = Mode::Grid;
                true
            }
            Mode::Grid => false,
        }
    }

    /// Summoning drops any half-typed entry and returns to today.
    pub fn on_enter(&mut self) {
        self.mode = Mode::Grid;
        self.draft_text.clear();
        self.draft_time.clear();
        self.selected = 0;
        self.problem = None;
    }

    pub fn on_key(&mut self, press: KeyPress) -> bool {
        if !press.mods.is_plain() {
            return false;
        }
        match self.mode {
            Mode::Text => self.text_key(press),
            Mode::Time => self.time_key(press),
            Mode::Disambiguate(h) => self.disambiguate_key(press, h),
            Mode::List => self.list_key(press),
            Mode::Grid => self.grid_key(press),
        }
    }

    fn grid_key(&mut self, press: KeyPress) -> bool {
        self.problem = None;
        match press.key {
            Key::Char('h') | Key::Left => {
                self.move_focus(-1);
                true
            }
            Key::Char('l') | Key::Right => {
                self.move_focus(1);
                true
            }
            Key::Char('k') | Key::Up => {
                self.move_focus(-7);
                true
            }
            Key::Char('j') | Key::Down => {
                self.move_focus(7);
                true
            }
            // Month steps. Not H/L: Alt+H and Alt+L already cycle tabs, and
            // having plain H mean "previous month" while Alt+H means "previous
            // tool" is the kind of near-collision that trips fingers.
            Key::Char('[') => {
                self.move_month(-1);
                true
            }
            Key::Char(']') => {
                self.move_month(1);
                true
            }
            Key::Char('t') => {
                self.focus = self.today;
                self.selected = 0;
                true
            }
            Key::Char('a') | Key::Char('i') | Key::Enter => {
                self.mode = Mode::Text;
                self.draft_text.clear();
                self.draft_time.clear();
                true
            }
            Key::Tab | Key::Char('n') => {
                if self.has_entries(self.focus) {
                    self.mode = Mode::List;
                    self.selected = 0;
                }
                true
            }
            _ => false,
        }
    }

    fn text_key(&mut self, press: KeyPress) -> bool {
        match press.key {
            Key::Char(c) => {
                self.draft_text.push(c);
                true
            }
            Key::Backspace => {
                self.draft_text.pop();
                true
            }
            Key::Enter => {
                if self.draft_text.trim().is_empty() {
                    // Nothing typed: treat Enter as backing out rather than
                    // creating a blank entry.
                    self.mode = Mode::Grid;
                } else {
                    self.mode = Mode::Time;
                }
                true
            }
            _ => false,
        }
    }

    fn time_key(&mut self, press: KeyPress) -> bool {
        match press.key {
            Key::Char(c) => {
                self.draft_time.push(c);
                self.problem = None;
                true
            }
            Key::Backspace => {
                self.draft_time.pop();
                self.problem = None;
                true
            }
            Key::Enter => {
                match parse_time(&self.draft_time) {
                    TimeParse::AllDay => self.commit_entry(None),
                    TimeParse::At(m) => self.commit_entry(Some(m)),
                    // The user's rule: when it could be either, ask.
                    TimeParse::Ambiguous(h) => {
                        self.mode = Mode::Disambiguate(h);
                        self.problem = None;
                    }
                    TimeParse::Invalid(msg) => self.problem = Some(msg),
                }
                true
            }
            _ => false,
        }
    }

    fn disambiguate_key(&mut self, press: KeyPress, hour: u32) -> bool {
        match press.key {
            Key::Char('a') | Key::Char('A') => {
                let h = if hour == 12 { 0 } else { hour };
                self.commit_entry(Some(h * 60));
                true
            }
            Key::Char('p') | Key::Char('P') => {
                let h = if hour == 12 { 12 } else { hour + 12 };
                self.commit_entry(Some(h * 60));
                true
            }
            _ => false,
        }
    }

    fn list_key(&mut self, press: KeyPress) -> bool {
        let count = self.on(self.focus).len();
        if count == 0 {
            self.mode = Mode::Grid;
            return true;
        }
        match press.key {
            Key::Char('j') | Key::Down => {
                self.selected = (self.selected + 1).min(count - 1);
                true
            }
            Key::Char('k') | Key::Up => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            Key::Char('d') | Key::Delete => {
                // Find the entry by identity within the day, then remove it
                // from the flat list -- indices into a filtered view cannot be
                // used against the backing vector.
                if let Some(target) = self.on(self.focus).get(self.selected).map(|e| (*e).clone()) {
                    if let Some(pos) = self.entries.iter().position(|e| *e == target) {
                        self.entries.remove(pos);
                        self.dirty = true;
                    }
                }
                let left = self.on(self.focus).len();
                if left == 0 {
                    self.mode = Mode::Grid;
                    self.selected = 0;
                } else {
                    self.selected = self.selected.min(left - 1);
                }
                true
            }
            _ => false,
        }
    }
}

/// Newlines and tabs would break the one-entry-per-line format, so they are
/// flattened on the way in rather than corrupting the file on the way out.
fn to_single_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

fn parse_line(line: &str) -> Option<Entry> {
    let mut parts = line.trim().splitn(3, ' ');
    let date = Date::parse_iso(parts.next()?)?;
    let time_field = parts.next()?;
    let text = parts.next()?.trim();
    if text.is_empty() {
        return None;
    }
    let time = if time_field == ALL_DAY {
        None
    } else {
        match parse_time(time_field) {
            TimeParse::At(m) => Some(m),
            _ => return None,
        }
    };
    Some(Entry { date, time, text: text.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calendar {
        let mut c = Calendar::new();
        c.set_today(Date::new(2026, 8, 21));
        c.focus = Date::new(2026, 8, 21);
        c
    }

    fn press(c: &mut Calendar, s: &str) {
        for ch in s.chars() {
            c.on_key(KeyPress::ch(ch));
        }
    }

    #[test]
    fn entries_round_trip_through_the_file_format() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 24), Some(15 * 60), "Dentist");
        c.add(Date::new(2026, 8, 24), None, "Call the landlord");
        let text = c.serialise();

        let mut b = Calendar::new();
        assert_eq!(b.load(&text), 0);
        assert_eq!(b.entries(), c.entries());
    }

    #[test]
    fn all_day_entries_sort_above_timed_ones() {
        let mut c = cal();
        let d = Date::new(2026, 8, 24);
        c.add(d, Some(9 * 60), "Standup");
        c.add(d, None, "Rent due");
        c.add(d, Some(15 * 60), "Dentist");
        let texts: Vec<&str> = c.on(d).iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["Rent due", "Standup", "Dentist"]);
    }

    #[test]
    fn unreadable_lines_are_kept_not_dropped() {
        let mut c = Calendar::new();
        let file = "2026-08-24 15:00 Dentist\nthis line is nonsense\n2026-99-99 15:00 bad date\n";
        assert_eq!(c.load(file), 2, "two lines unparsed");
        let out = c.serialise();
        // The whole point: a parse failure must not cost the user data.
        assert!(out.contains("this line is nonsense"));
        assert!(out.contains("2026-99-99 15:00 bad date"));
        assert!(out.contains("2026-08-24 15:00 Dentist"));
    }

    #[test]
    fn comments_and_blank_lines_are_not_treated_as_damage() {
        let mut c = Calendar::new();
        assert_eq!(c.load("# a comment\n\n   \n2026-08-24 15:00 Dentist\n"), 0);
        assert_eq!(c.entries().len(), 1);
    }

    #[test]
    fn text_with_spaces_survives_a_round_trip() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 24), Some(600), "Lunch with Dana and Sam");
        let mut b = Calendar::new();
        b.load(&c.serialise());
        assert_eq!(b.entries()[0].text, "Lunch with Dana and Sam");
    }

    #[test]
    fn hjkl_moves_by_day_and_by_week() {
        let mut c = cal();
        press(&mut c, "l");
        assert_eq!(c.focus, Date::new(2026, 8, 22));
        press(&mut c, "h");
        assert_eq!(c.focus, Date::new(2026, 8, 21));
        press(&mut c, "j");
        assert_eq!(c.focus, Date::new(2026, 8, 28), "j is a week forward");
        press(&mut c, "k");
        assert_eq!(c.focus, Date::new(2026, 8, 21));
    }

    #[test]
    fn day_movement_crosses_months() {
        let mut c = cal();
        c.focus = Date::new(2026, 8, 31);
        press(&mut c, "l");
        assert_eq!(c.focus, Date::new(2026, 9, 1));
    }

    #[test]
    fn brackets_step_whole_months_and_t_returns_to_today() {
        let mut c = cal();
        press(&mut c, "]");
        assert_eq!(c.focus, Date::new(2026, 9, 21));
        press(&mut c, "[[");
        assert_eq!(c.focus, Date::new(2026, 7, 21));
        press(&mut c, "t");
        assert_eq!(c.focus, c.today);
    }

    #[test]
    fn adding_an_entry_takes_text_then_time() {
        let mut c = cal();
        press(&mut c, "a");
        assert_eq!(c.mode, Mode::Text);
        press(&mut c, "Dentist");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Time);
        press(&mut c, "15:00");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Grid);
        let e = &c.on(c.focus)[0];
        assert_eq!(e.text, "Dentist");
        assert_eq!(e.time, Some(15 * 60));
    }

    #[test]
    fn an_empty_time_makes_an_all_day_entry() {
        let mut c = cal();
        press(&mut c, "a");
        press(&mut c, "Rent due");
        c.on_key(KeyPress::plain(Key::Enter));
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.on(c.focus)[0].time, None);
    }

    #[test]
    fn an_ambiguous_time_asks_instead_of_guessing() {
        let mut c = cal();
        press(&mut c, "a");
        press(&mut c, "Dentist");
        c.on_key(KeyPress::plain(Key::Enter));
        press(&mut c, "3");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Disambiguate(3), "must ask, not pick");
        assert!(c.on(c.focus).is_empty(), "nothing committed while asking");
        c.on_key(KeyPress::ch('p'));
        assert_eq!(c.on(c.focus)[0].time, Some(15 * 60));
    }

    #[test]
    fn the_ambiguity_prompt_handles_twelve_correctly() {
        for (key, expect) in [('a', 0), ('p', 12 * 60)] {
            let mut c = cal();
            press(&mut c, "a");
            press(&mut c, "Thing");
            c.on_key(KeyPress::plain(Key::Enter));
            press(&mut c, "12");
            c.on_key(KeyPress::plain(Key::Enter));
            c.on_key(KeyPress::ch(key));
            assert_eq!(c.on(c.focus)[0].time, Some(expect), "12{key}m");
        }
    }

    #[test]
    fn an_unreadable_time_says_so_and_keeps_the_prompt_open() {
        let mut c = cal();
        press(&mut c, "a");
        press(&mut c, "Dentist");
        c.on_key(KeyPress::plain(Key::Enter));
        press(&mut c, "25:00");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Time, "stays put so it can be corrected");
        assert!(c.problem.is_some());
        assert!(c.on(c.focus).is_empty());
    }

    #[test]
    fn esc_backs_out_one_step_at_a_time_without_losing_the_text() {
        let mut c = cal();
        press(&mut c, "a");
        press(&mut c, "Dentist");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Time);
        assert!(c.leave_one_level());
        assert_eq!(c.mode, Mode::Text);
        assert_eq!(c.draft_text, "Dentist", "a wrong time must not eat the text");
        assert!(c.leave_one_level());
        assert_eq!(c.mode, Mode::Grid);
        assert!(!c.leave_one_level(), "grid is the last rung");
    }

    #[test]
    fn enter_on_empty_text_backs_out_rather_than_making_a_blank_entry() {
        let mut c = cal();
        press(&mut c, "a");
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.mode, Mode::Grid);
        assert!(c.on(c.focus).is_empty());
    }

    #[test]
    fn entries_can_be_selected_and_deleted() {
        let mut c = cal();
        let d = c.focus;
        c.add(d, Some(9 * 60), "Standup");
        c.add(d, Some(15 * 60), "Dentist");
        c.on_key(KeyPress::plain(Key::Tab));
        assert_eq!(c.mode, Mode::List);
        press(&mut c, "j");
        assert_eq!(c.selected, 1);
        press(&mut c, "d");
        let left: Vec<&str> = c.on(d).iter().map(|e| e.text.as_str()).collect();
        assert_eq!(left, vec!["Standup"], "the selected one went, not the first");
    }

    #[test]
    fn deleting_the_last_entry_returns_to_the_grid() {
        let mut c = cal();
        c.add(c.focus, None, "Only");
        c.on_key(KeyPress::plain(Key::Tab));
        press(&mut c, "d");
        assert_eq!(c.mode, Mode::Grid);
        assert!(c.on(c.focus).is_empty());
    }

    #[test]
    fn deleting_removes_the_right_entry_when_other_days_have_some() {
        let mut c = cal();
        let other = c.focus.add_days(-3);
        c.add(other, Some(60), "Elsewhere");
        c.add(c.focus, Some(120), "Here");
        c.on_key(KeyPress::plain(Key::Tab));
        press(&mut c, "d");
        // The filtered index was 0, but "Elsewhere" is index 0 in the vector.
        assert_eq!(c.entries().len(), 1);
        assert_eq!(c.entries()[0].text, "Elsewhere");
    }

    #[test]
    fn tab_does_nothing_on_a_day_with_no_entries() {
        let mut c = cal();
        c.on_key(KeyPress::plain(Key::Tab));
        assert_eq!(c.mode, Mode::Grid);
    }

    #[test]
    fn control_characters_cannot_break_the_line_format() {
        let mut c = cal();
        c.add(c.focus, None, "two\nlines\tand a tab");
        assert_eq!(c.serialise().lines().filter(|l| l.contains("two")).count(), 1);
        let mut b = Calendar::new();
        assert_eq!(b.load(&c.serialise()), 0, "must not produce an unparsable line");
    }

    #[test]
    fn a_new_day_moves_the_focus_only_if_it_was_on_today() {
        let mut c = cal();
        press(&mut c, "]");
        let looking_at = c.focus;
        c.set_today(Date::new(2026, 8, 22));
        assert_eq!(c.focus, looking_at, "must not yank you back while browsing");

        let mut c2 = cal();
        c2.set_today(Date::new(2026, 8, 22));
        assert_eq!(c2.focus, Date::new(2026, 8, 22), "but do follow if you were on today");
    }

    #[test]
    fn adding_and_deleting_mark_the_file_dirty() {
        let mut c = cal();
        assert!(!c.dirty);
        c.add(c.focus, None, "Thing");
        assert!(c.dirty, "a change must ask to be saved");
        c.dirty = false;
        c.on_key(KeyPress::plain(Key::Tab));
        press(&mut c, "d");
        assert!(c.dirty);
    }

    #[test]
    fn loading_does_not_mark_the_file_dirty() {
        let mut c = Calendar::new();
        c.load("2026-08-24 15:00 Dentist\n");
        assert!(!c.dirty, "reading a file is not a change to it");
    }

    #[test]
    fn summoning_drops_a_half_typed_entry() {
        let mut c = cal();
        press(&mut c, "a");
        press(&mut c, "half typed");
        c.on_enter();
        assert_eq!(c.mode, Mode::Grid);
        assert!(c.draft_text.is_empty());
        assert!(c.on(c.focus).is_empty());
    }
}
