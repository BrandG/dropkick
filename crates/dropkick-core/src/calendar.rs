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
    /// How often it comes back. [`Repeat::None`] for nearly everything.
    pub repeat: Repeat,
}

/// How an entry recurs.
///
/// **A closed set of five, chosen deliberately over a grammar.** A recurrence
/// *language* — RFC 5545's `RRULE`, or anything that can say "first Monday" —
/// has no natural stopping point: `BYSETPOS`, `COUNT` against `UNTIL`,
/// exception dates, and the edge cases nobody thinks of until they arrive.
/// Cycling a fixed list with one key cannot grow into that, because there is
/// no syntax to extend.
///
/// If this ever needs "first Monday of the month", that is a different
/// feature and a different design. It is not an extension of this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    None,
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl Repeat {
    /// The order `r` walks, ending back at `None` so the key can undo itself.
    pub fn next(self) -> Repeat {
        match self {
            Repeat::None => Repeat::Hourly,
            Repeat::Hourly => Repeat::Daily,
            Repeat::Daily => Repeat::Weekly,
            Repeat::Weekly => Repeat::Monthly,
            Repeat::Monthly => Repeat::None,
        }
    }

    /// What the panel shows beside an entry. Empty for `None`, so an entry
    /// that does not repeat says nothing rather than saying "never".
    pub fn label(self) -> &'static str {
        match self {
            Repeat::None => "",
            Repeat::Hourly => "hourly",
            Repeat::Daily => "daily",
            Repeat::Weekly => "weekly",
            Repeat::Monthly => "monthly",
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Repeat::None => None,
            other => Some(other.label()),
        }
    }

    fn from_token(s: &str) -> Option<Repeat> {
        match s {
            "hourly" => Some(Repeat::Hourly),
            "daily" => Some(Repeat::Daily),
            "weekly" => Some(Repeat::Weekly),
            "monthly" => Some(Repeat::Monthly),
            _ => None,
        }
    }
}

impl Entry {
    fn sort_key(&self) -> (i64, u32, String) {
        // All-day entries sort to the top of their day: they are the ones with
        // no position within it.
        (self.date.to_days(), self.time.map_or(0, |t| t + 1), self.text.clone())
    }

    pub fn to_line(&self) -> String {
        let mut t = self.time.map_or(ALL_DAY.to_string(), format_time);
        // The recurrence rides in the *time* field, not beside the text.
        //
        // Putting it before the text would make an entry called "daily
        // standup" parse as a daily recurrence of "standup" -- the same class
        // of collision as a key that is both a unit and a command. The time
        // field is already strictly parsed, so nothing free-form can wander
        // into it. Old files have no suffix and read exactly as before.
        if let Some(token) = self.repeat.token() {
            t.push('/');
            t.push_str(token);
        }
        format!("{} {} {}", self.date.to_iso(), t, self.text)
    }

    /// Whether this entry lands on `date` — the one place recurrence is
    /// decided, so the month dots, the day list and the alarms cannot
    /// disagree about it.
    pub fn occurs_on(&self, date: Date) -> bool {
        if date == self.date {
            return true;
        }
        if date < self.date {
            return false;
        }
        match self.repeat {
            Repeat::None => false,
            Repeat::Hourly | Repeat::Daily => true,
            Repeat::Weekly => date.weekday() == self.date.weekday(),
            // **The skip rule, and it needs no special case.** A monthly entry
            // lands on the same day number, and there is no 31st of February
            // for it to land on, so those months are simply missed. The 29th
            // and 30th skip February except in leap years; the 29th of
            // February comes back every four. One rule, no exceptions table.
            Repeat::Monthly => date.day == self.date.day,
        }
    }

    /// Every minute-past-midnight this entry alarms at on `date`.
    ///
    /// All-day entries never alarm: they say "today is X", not "at X, do Y".
    pub fn alarm_times_on(&self, date: Date) -> Vec<u32> {
        let Some(t) = self.time else { return Vec::new() };
        if !self.occurs_on(date) {
            return Vec::new();
        }
        match self.repeat {
            // Hourly keeps the minute past the hour and starts from the
            // entry's own time on its first day, every hour after that.
            Repeat::Hourly => {
                let minute = t % 60;
                let first = if date == self.date { t / 60 } else { 0 };
                (first..24).map(|h| h * 60 + minute).collect()
            }
            _ => vec![t],
        }
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

    /// Every occurrence on `date` whose time has arrived by `now_min`, no
    /// earlier than `floor`.
    ///
    /// `floor` is how "only while running" is expressed: the platform passes
    /// the minute the daemon started, so this morning's 09:00 is not
    /// announced by a daemon that started at 14:00. On any later day the
    /// caller passes 0 and everything qualifies.
    ///
    /// Pure, and returns *what* is due rather than doing anything about it —
    /// the caller owns the "has this one already been announced" bookkeeping,
    /// and this owns the part worth testing.
    pub fn due_at(&self, date: Date, now_min: u32, floor: u32) -> Vec<(u32, &str)> {
        let mut out: Vec<(u32, &str)> = Vec::new();
        for entry in &self.entries {
            for minute in entry.alarm_times_on(date) {
                if minute <= now_min && minute >= floor {
                    out.push((minute, entry.text.as_str()));
                }
            }
        }
        out.sort();
        out
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn on(&self, date: Date) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.occurs_on(date)).collect()
    }

    pub fn has_entries(&self, date: Date) -> bool {
        self.entries.iter().any(|e| e.occurs_on(date))
    }

    fn sort(&mut self) {
        self.entries.sort_by_key(|e| e.sort_key());
    }

    pub fn add(&mut self, date: Date, time: Option<u32>, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.entries.push(Entry {
            date,
            time,
            text: to_single_line(text),
            repeat: Repeat::None,
        });
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
            // Cycle how often it repeats. Set here rather than while adding,
            // so the common case -- an entry that happens once -- costs
            // nothing, and so an entry that already exists can change its
            // mind without being deleted and retyped.
            Key::Char('r') => {
                if let Some(target) = self.on(self.focus).get(self.selected).map(|e| (*e).clone()) {
                    if let Some(pos) = self.entries.iter().position(|e| *e == target) {
                        self.entries[pos].repeat = self.entries[pos].repeat.next();
                        self.dirty = true;
                    }
                }
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
    let (time_field, repeat) = match time_field.split_once('/') {
        Some((t, token)) => (t, Repeat::from_token(token)?),
        None => (time_field, Repeat::None),
    };
    let time = if time_field == ALL_DAY {
        None
    } else {
        match parse_time(time_field) {
            TimeParse::At(m) => Some(m),
            _ => return None,
        }
    };
    Some(Entry { date, time, text: text.to_string(), repeat })
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

    fn repeating(date: Date, time: Option<u32>, repeat: Repeat) -> Entry {
        Entry { date, time, text: "thing".to_string(), repeat }
    }

    #[test]
    fn a_one_off_happens_once() {
        let e = repeating(Date::new(2026, 8, 21), Some(540), Repeat::None);
        assert!(e.occurs_on(Date::new(2026, 8, 21)));
        assert!(!e.occurs_on(Date::new(2026, 8, 22)));
        assert!(!e.occurs_on(Date::new(2026, 8, 20)));
    }

    #[test]
    fn nothing_recurs_backwards() {
        // A daily entry made today is not a claim about last week.
        let e = repeating(Date::new(2026, 8, 21), Some(540), Repeat::Daily);
        assert!(!e.occurs_on(Date::new(2026, 8, 20)));
        assert!(e.occurs_on(Date::new(2026, 8, 21)));
        assert!(e.occurs_on(Date::new(2027, 1, 1)));
    }

    #[test]
    fn weekly_keeps_the_weekday() {
        // 2026-08-21 is a Friday.
        let e = repeating(Date::new(2026, 8, 21), Some(540), Repeat::Weekly);
        assert!(e.occurs_on(Date::new(2026, 8, 28)), "the next Friday");
        assert!(!e.occurs_on(Date::new(2026, 8, 27)), "but not the Thursday");
        assert!(e.occurs_on(Date::new(2026, 9, 4)));
    }

    /// The rule the user asked to be told about: the date has to exist.
    #[test]
    fn monthly_on_the_31st_skips_months_that_have_no_31st() {
        let e = repeating(Date::new(2026, 1, 31), None, Repeat::Monthly);
        assert!(e.occurs_on(Date::new(2026, 3, 31)), "March has one");
        assert!(e.occurs_on(Date::new(2026, 5, 31)));
        // February, April, June, September and November do not.
        for (m, last) in [(2u32, 28u32), (4, 30), (6, 30), (9, 30), (11, 30)] {
            for day in 1..=last {
                assert!(
                    !e.occurs_on(Date::new(2026, m, day)),
                    "month {m} should be skipped entirely, but day {day} matched"
                );
            }
        }
    }

    #[test]
    fn monthly_on_the_29th_skips_february_except_in_a_leap_year() {
        let e = repeating(Date::new(2026, 1, 29), None, Repeat::Monthly);
        assert!(!e.occurs_on(Date::new(2027, 2, 28)), "2027 is not a leap year");
        assert!(e.occurs_on(Date::new(2028, 2, 29)), "2028 is");
    }

    #[test]
    fn the_29th_of_february_comes_back_every_four_years() {
        let e = repeating(Date::new(2028, 2, 29), None, Repeat::Monthly);
        assert!(e.occurs_on(Date::new(2032, 2, 29)));
        assert!(e.occurs_on(Date::new(2028, 3, 29)), "and every other 29th");
    }

    #[test]
    fn an_all_day_entry_never_alarms_however_it_repeats() {
        // "today is X", not "at X, do Y".
        let e = repeating(Date::new(2026, 8, 21), None, Repeat::Daily);
        assert!(e.alarm_times_on(Date::new(2026, 8, 22)).is_empty());
    }

    #[test]
    fn hourly_starts_at_its_own_time_then_runs_the_clock() {
        // 14:23 on the first day, then every hour at 23 past.
        let e = repeating(Date::new(2026, 8, 21), Some(14 * 60 + 23), Repeat::Hourly);
        let first = e.alarm_times_on(Date::new(2026, 8, 21));
        assert_eq!(first.first(), Some(&(14 * 60 + 23)), "not before it was made");
        assert_eq!(first.last(), Some(&(23 * 60 + 23)));
        assert_eq!(first.len(), 10);

        let later = e.alarm_times_on(Date::new(2026, 8, 22));
        assert_eq!(later.len(), 24, "a full day after that");
        assert_eq!(later.first(), Some(&23));
    }

    #[test]
    fn due_at_reports_times_that_have_arrived_and_not_ones_still_coming() {
        let mut c = cal();
        c.entries.push(repeating(Date::new(2026, 8, 21), Some(540), Repeat::None));
        let day = Date::new(2026, 8, 21);
        assert!(c.due_at(day, 539, 0).is_empty(), "09:00 has not arrived at 08:59");
        assert_eq!(c.due_at(day, 540, 0).len(), 1, "it has at 09:00");
        assert_eq!(c.due_at(day, 1200, 0).len(), 1, "and stays due later in the day");
    }

    /// The "only while running" rule. A daemon started at 14:00 must not
    /// announce this morning's alarms five hours late.
    #[test]
    fn due_at_ignores_occurrences_from_before_the_floor() {
        let mut c = cal();
        c.entries.push(repeating(Date::new(2026, 8, 21), Some(540), Repeat::None));
        let day = Date::new(2026, 8, 21);
        assert!(c.due_at(day, 14 * 60, 14 * 60).is_empty(), "09:00 is behind the floor");
        assert_eq!(c.due_at(day, 14 * 60, 0).len(), 1, "with no floor it is due");
    }

    #[test]
    fn a_recurring_entry_shows_on_the_month_grid_and_in_the_day_list() {
        // The month dots, the day list and the alarms all go through
        // occurs_on, so this is really checking they cannot disagree.
        let mut c = cal();
        c.entries.push(repeating(Date::new(2026, 8, 21), Some(540), Repeat::Weekly));
        assert!(c.has_entries(Date::new(2026, 8, 28)));
        assert_eq!(c.on(Date::new(2026, 8, 28)).len(), 1);
        assert!(!c.has_entries(Date::new(2026, 8, 27)));
    }

    #[test]
    fn r_cycles_the_repeat_of_the_selected_entry_and_back_to_none() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 21), Some(540), "standup");
        c.focus = Date::new(2026, 8, 21);
        c.on_key(KeyPress::plain(Key::Tab));
        assert_eq!(c.mode, Mode::List);
        for want in [
            Repeat::Hourly,
            Repeat::Daily,
            Repeat::Weekly,
            Repeat::Monthly,
            Repeat::None,
        ] {
            c.on_key(KeyPress::ch('r'));
            assert_eq!(c.entries[0].repeat, want);
        }
    }

    #[test]
    fn changing_the_repeat_marks_the_calendar_for_saving() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 21), Some(540), "standup");
        c.focus = Date::new(2026, 8, 21);
        c.on_key(KeyPress::plain(Key::Tab));
        c.dirty = false;
        c.on_key(KeyPress::ch('r'));
        assert!(c.dirty, "otherwise the change is lost on quit");
    }

    #[test]
    fn a_repeat_survives_a_round_trip_through_the_file() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 21), Some(540), "standup");
        c.entries[0].repeat = Repeat::Weekly;
        c.add(Date::new(2026, 8, 22), None, "rent");
        c.entries[1].repeat = Repeat::Monthly;
        let mut b = Calendar::new();
        assert_eq!(b.load(&c.serialise()), 0, "no unparsable lines");
        assert_eq!(b.entries[0].repeat, Repeat::Weekly);
        assert_eq!(b.entries[1].repeat, Repeat::Monthly);
        assert_eq!(b.entries[1].time, None, "an all-day entry can still repeat");
    }

    /// The reason the marker rides in the time field rather than beside the
    /// text: an entry whose text starts with a repeat word must stay text.
    #[test]
    fn an_entry_called_daily_standup_is_not_a_daily_entry() {
        let mut c = cal();
        c.add(Date::new(2026, 8, 21), Some(540), "daily standup");
        let mut b = Calendar::new();
        b.load(&c.serialise());
        assert_eq!(b.entries[0].repeat, Repeat::None);
        assert_eq!(b.entries[0].text, "daily standup");
    }

    #[test]
    fn an_old_file_without_any_repeat_marker_still_loads() {
        let mut c = Calendar::new();
        let unparsed = c.load("2026-08-21 09:00 standup\n2026-08-22 --:-- a whole day\n");
        assert_eq!(unparsed, 0);
        assert_eq!(c.entries.len(), 2);
        assert_eq!(c.entries[0].repeat, Repeat::None);
    }

    #[test]
    fn a_line_with_an_unknown_repeat_marker_is_kept_rather_than_dropped() {
        // Same rule as everywhere else: a line we cannot read goes back out
        // untouched instead of being deleted on the next save.
        let mut c = Calendar::new();
        let unparsed = c.load("2026-08-21 09:00/fortnightly standup\n");
        assert_eq!(unparsed, 1);
        assert!(c.serialise().contains("fortnightly"));
    }
}
