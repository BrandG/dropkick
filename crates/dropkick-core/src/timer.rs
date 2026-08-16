//! Timer and stopwatch.
//!
//! The first tool that does something while Dropkick is not on screen. Every
//! other tool is inert when dismissed; a countdown that only fires while you
//! happen to be looking at it is not a countdown.
//!
//! Time arrives as a parameter rather than being read here, for the same reason
//! the Esc ladder takes `now_ms`: the core stays deterministic and the whole
//! thing is testable without sleeping.

use crate::key::{Key, KeyPress};

/// Refuse to set a countdown longer than this. Anything beyond it is a typo.
pub const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Counts up from zero.
    Stopwatch,
    /// Counts down to zero, then notifies.
    Countdown,
}

#[derive(Debug)]
pub struct Timer {
    pub mode: Mode,
    /// Duration being typed, e.g. `25m` or `1h30m`.
    pub input: String,
    pub error: Option<String>,
    running: bool,
    /// Milliseconds banked from previous run segments.
    accumulated_ms: u64,
    /// `now_ms` when the current segment started, if running.
    segment_start: Option<u64>,
    /// Total for a countdown.
    duration_ms: u64,
    /// A countdown reached zero and the notification has not been raised yet.
    expired_unreported: bool,
    /// A countdown has finished; the display stays at zero until reset.
    pub finished: bool,
}

impl Default for Timer {
    fn default() -> Self {
        Timer {
            mode: Mode::Stopwatch,
            input: String::new(),
            error: None,
            running: false,
            accumulated_ms: 0,
            segment_start: None,
            duration_ms: 0,
            expired_unreported: false,
            finished: false,
        }
    }
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    /// Milliseconds counted so far in the current session.
    pub fn elapsed_ms(&self, now_ms: u64) -> u64 {
        let live = match self.segment_start {
            Some(start) if self.running => now_ms.saturating_sub(start),
            _ => 0,
        };
        self.accumulated_ms + live
    }

    /// Milliseconds left on a countdown, saturating at zero.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.duration_ms.saturating_sub(self.elapsed_ms(now_ms))
    }

    /// What the big readout shows.
    pub fn display_ms(&self, now_ms: u64) -> u64 {
        match self.mode {
            Mode::Stopwatch => self.elapsed_ms(now_ms),
            Mode::Countdown => self.remaining_ms(now_ms),
        }
    }

    /// Advance the clock. Returns true exactly once, on the tick where a
    /// countdown reaches zero, so the platform raises one notification and not
    /// one per tick.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.mode == Mode::Countdown && self.running && self.remaining_ms(now_ms) == 0 {
            self.accumulated_ms = self.duration_ms;
            self.segment_start = None;
            self.running = false;
            self.finished = true;
            if self.expired_unreported {
                self.expired_unreported = false;
                return true;
            }
        }
        false
    }

    pub fn start(&mut self, now_ms: u64) {
        if self.running {
            return;
        }
        if self.mode == Mode::Countdown && (self.duration_ms == 0 || self.finished) {
            return;
        }
        self.running = true;
        self.segment_start = Some(now_ms);
        if self.mode == Mode::Countdown {
            self.expired_unreported = true;
        }
    }

    pub fn pause(&mut self, now_ms: u64) {
        if !self.running {
            return;
        }
        self.accumulated_ms = self.elapsed_ms(now_ms);
        self.segment_start = None;
        self.running = false;
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.accumulated_ms = 0;
        self.segment_start = None;
        self.expired_unreported = false;
        self.finished = false;
    }

    /// Begin a countdown from the typed duration.
    fn arm(&mut self, now_ms: u64) {
        match parse_duration(&self.input) {
            Some(ms) => {
                self.mode = Mode::Countdown;
                self.duration_ms = ms;
                self.error = None;
                self.input.clear();
                self.reset();
                self.start(now_ms);
            }
            None => {
                self.error = Some(if self.input.trim().is_empty() {
                    "type a duration first, like 25m".to_string()
                } else {
                    format!("{} is not a duration", self.input)
                });
            }
        }
    }

    pub fn on_key(&mut self, press: KeyPress, now_ms: u64) -> bool {
        if !press.mods.is_plain() {
            return false;
        }
        match press.key {
            // Space is start/pause and never enters the duration, so there is
            // no ambiguity about what a keypress means while typing one.
            Key::Char(' ') => {
                if self.running {
                    self.pause(now_ms);
                } else if self.mode == Mode::Countdown && self.finished {
                    // Nothing to resume; a finished countdown needs a reset.
                } else {
                    self.start(now_ms);
                }
                true
            }
            Key::Enter => {
                self.arm(now_ms);
                true
            }
            Key::Char('r') => {
                self.reset();
                self.error = None;
                true
            }
            // 'w' for stopwatch, not 's': `s` is a duration unit, and `30s` is
            // the most natural way to type thirty seconds. A key that is both a
            // unit and a command silently does the wrong one.
            Key::Char('w') => {
                self.mode = Mode::Stopwatch;
                self.duration_ms = 0;
                self.input.clear();
                self.error = None;
                self.reset();
                self.start(now_ms);
                true
            }
            Key::Backspace => {
                self.input.pop();
                self.error = None;
                true
            }
            Key::Char(c) if c.is_ascii_digit() || matches!(c, 'h' | 'm' | 's') => {
                if self.input.chars().count() < 12 {
                    self.input.push(c);
                    self.error = None;
                }
                true
            }
            _ => false,
        }
    }
}

/// Parse `90`, `25m`, `1h30m`, `2m30s`. A bare number is seconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut total_ms: u64 = 0;
    let mut number: Option<u64> = None;
    let mut saw_unit = false;

    for c in text.chars() {
        if let Some(d) = c.to_digit(10) {
            number = Some(number.unwrap_or(0).checked_mul(10)?.checked_add(d as u64)?);
            continue;
        }
        let unit_ms = match c {
            'h' => 60 * 60 * 1000,
            'm' => 60 * 1000,
            's' => 1000,
            _ => return None,
        };
        let value = number.take()?;
        total_ms = total_ms.checked_add(value.checked_mul(unit_ms)?)?;
        saw_unit = true;
    }

    // A trailing bare number is seconds, so `90` works and `1h30` means 1h30s.
    if let Some(value) = number {
        total_ms = total_ms.checked_add(value.checked_mul(1000)?)?;
    } else if !saw_unit {
        return None;
    }

    if total_ms == 0 || total_ms > MAX_DURATION_MS {
        return None;
    }
    Some(total_ms)
}

/// `1:05.3` while under an hour, `1:02:03` beyond it.
pub fn format_duration(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let tenths = (ms % 1000) / 100;
    let (h, m, s) = (total_seconds / 3600, (total_seconds / 60) % 60, total_seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}.{tenths}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space() -> KeyPress {
        KeyPress::ch(' ')
    }

    #[test]
    fn parses_the_durations_people_type() {
        assert_eq!(parse_duration("90"), Some(90_000));
        assert_eq!(parse_duration("25m"), Some(25 * 60_000));
        assert_eq!(parse_duration("1h"), Some(3_600_000));
        assert_eq!(parse_duration("1h30m"), Some(5_400_000));
        assert_eq!(parse_duration("2m30s"), Some(150_000));
        assert_eq!(parse_duration("45s"), Some(45_000));
    }

    #[test]
    fn rejects_nonsense_without_panicking() {
        for bad in ["", "   ", "abc", "5x", "m", "-5", "99999999999999999999", "0", "25d"] {
            assert_eq!(parse_duration(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn rejects_absurd_durations() {
        assert_eq!(parse_duration("25h"), None);
        assert_eq!(parse_duration("2000m"), None);
    }

    #[test]
    fn formats_readably() {
        assert_eq!(format_duration(0), "0:00.0");
        assert_eq!(format_duration(65_400), "1:05.4");
        assert_eq!(format_duration(3_723_000), "1:02:03");
    }

    #[test]
    fn stopwatch_counts_up_and_pauses() {
        let mut t = Timer::new();
        t.start(1_000);
        assert_eq!(t.elapsed_ms(4_000), 3_000);
        t.pause(4_000);
        // Time passing while paused must not count.
        assert_eq!(t.elapsed_ms(9_000), 3_000);
        t.start(10_000);
        assert_eq!(t.elapsed_ms(12_000), 5_000);
    }

    #[test]
    fn reset_returns_to_zero() {
        let mut t = Timer::new();
        t.start(0);
        t.pause(5_000);
        t.reset();
        assert_eq!(t.elapsed_ms(9_000), 0);
        assert!(!t.is_running());
    }

    #[test]
    fn countdown_expires_once_and_only_once() {
        let mut t = Timer::new();
        t.input = "5s".to_string();
        t.arm(0);
        assert_eq!(t.remaining_ms(2_000), 3_000);
        assert!(!t.tick(2_000), "must not fire early");
        assert!(t.tick(5_000), "must fire at zero");
        assert!(!t.tick(5_100), "must not fire twice");
        assert!(!t.tick(9_000));
        assert!(t.finished);
        assert!(!t.is_running());
    }

    #[test]
    fn a_late_tick_still_fires() {
        // The daemon ticks once a second while hidden, so expiry is usually
        // noticed after the fact rather than exactly on time.
        let mut t = Timer::new();
        t.input = "5s".to_string();
        t.arm(0);
        assert!(t.tick(60_000), "a tick long after expiry must still fire");
    }

    #[test]
    fn a_paused_countdown_does_not_expire() {
        let mut t = Timer::new();
        t.input = "5s".to_string();
        t.arm(0);
        t.pause(1_000);
        assert!(!t.tick(60_000), "paused time must not run out");
        assert_eq!(t.remaining_ms(60_000), 4_000);
    }

    #[test]
    fn space_toggles_running() {
        let mut t = Timer::new();
        t.on_key(space(), 0);
        assert!(t.is_running());
        t.on_key(space(), 1_000);
        assert!(!t.is_running());
        assert_eq!(t.elapsed_ms(5_000), 1_000);
    }

    #[test]
    fn typing_a_duration_then_enter_arms_a_countdown() {
        let mut t = Timer::new();
        for c in "25m".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        assert_eq!(t.input, "25m");
        t.on_key(KeyPress::plain(Key::Enter), 0);
        assert_eq!(t.mode, Mode::Countdown);
        assert!(t.is_running());
        assert!(t.input.is_empty());
        assert_eq!(t.remaining_ms(60_000), 24 * 60_000);
    }

    #[test]
    fn space_never_lands_in_the_duration() {
        let mut t = Timer::new();
        for c in "2".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        t.on_key(space(), 0);
        assert_eq!(t.input, "2", "space must not be typed into the duration");
    }

    #[test]
    fn a_bad_duration_reports_and_keeps_what_was_typed() {
        let mut t = Timer::new();
        for c in "5h".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        for c in "h".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        t.on_key(KeyPress::plain(Key::Enter), 0);
        assert!(t.error.is_some());
        assert!(!t.is_running());
    }

    #[test]
    fn enter_with_nothing_typed_says_so() {
        let mut t = Timer::new();
        t.on_key(KeyPress::plain(Key::Enter), 0);
        assert!(t.error.as_deref().unwrap().contains("duration"));
    }

    #[test]
    fn a_finished_countdown_needs_a_reset_not_a_resume() {
        let mut t = Timer::new();
        t.input = "1s".to_string();
        t.arm(0);
        t.tick(1_000);
        assert!(t.finished);
        t.on_key(space(), 2_000);
        assert!(!t.is_running(), "space must not restart a finished countdown");
        t.on_key(KeyPress::ch('r'), 2_000);
        assert!(!t.finished);
    }

    #[test]
    fn seconds_can_actually_be_typed() {
        // Regression: `s` used to start a stopwatch, so `30s` silently threw
        // away the duration and switched modes instead of arming 30 seconds.
        let mut t = Timer::new();
        for c in "30s".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        assert_eq!(t.input, "30s");
        t.on_key(KeyPress::plain(Key::Enter), 0);
        assert_eq!(t.mode, Mode::Countdown);
        assert_eq!(t.remaining_ms(0), 30_000);
    }

    #[test]
    fn every_duration_unit_is_typable() {
        let mut t = Timer::new();
        for c in "1h30m15s".chars() {
            t.on_key(KeyPress::ch(c), 0);
        }
        assert_eq!(t.input, "1h30m15s");
    }

    #[test]
    fn w_starts_a_stopwatch_from_a_countdown() {
        let mut t = Timer::new();
        t.input = "10m".to_string();
        t.arm(0);
        t.on_key(KeyPress::ch('w'), 1_000);
        assert_eq!(t.mode, Mode::Stopwatch);
        assert!(t.is_running());
        assert_eq!(t.elapsed_ms(3_000), 2_000);
    }

    #[test]
    fn alt_chords_are_never_consumed() {
        let mut t = Timer::new();
        assert!(!t.on_key(KeyPress::alt('q'), 0));
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_panic() {
        let mut t = Timer::new();
        t.start(10_000);
        assert_eq!(t.elapsed_ms(5_000), 0, "saturating, not underflowing");
    }
}
