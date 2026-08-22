//! Civil dates and clock times, with no dependency and no system clock.
//!
//! The rest of Dropkick takes `now_ms` from the platform, but that is
//! milliseconds since the *daemon started* — a stopwatch, not a calendar. A
//! calendar needs to know what day it actually is, in the local timezone, and
//! that has to enter the same way everything else does: as a parameter. The
//! platform reads `/etc/localtime` and hands in a [`Date`]; nothing here knows
//! what a timezone is.

/// A day on the civil calendar. No timezone, no time of day: this is what a
/// wall calendar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    /// 1..=12
    pub month: u32,
    /// 1..=31
    pub day: u32,
}

pub const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

/// Monday first. The week starts on Monday here because the month grid reads
/// better with the weekend together at the end.
pub const WEEKDAY_NAMES: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Date { year, month, day }
    }

    pub fn is_leap(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    pub fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Self::is_leap(year) => 29,
            2 => 28,
            _ => 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= Self::days_in_month(self.year, self.month)
    }

    /// Days since 1970-01-01. Howard Hinnant's `days_from_civil`, which is the
    /// standard branch-free formulation and correct for any year in range —
    /// worth using verbatim rather than inventing, because the interesting part
    /// is the century rule and it is easy to get subtly wrong.
    pub fn to_days(&self) -> i64 {
        let y = if self.month <= 2 { self.year - 1 } else { self.year } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400; // [0, 399]
        let m = self.month as i64;
        let d = self.day as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
        era * 146097 + doe - 719468
    }

    /// The inverse of [`Self::to_days`].
    pub fn from_days(days: i64) -> Date {
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097; // [0, 146096]
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
        let mp = (5 * doy + 2) / 153; // [0, 11]
        let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
        let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
        Date { year: (if m <= 2 { y + 1 } else { y }) as i32, month: m as u32, day: d as u32 }
    }

    /// 0 = Monday .. 6 = Sunday.
    pub fn weekday(&self) -> u32 {
        // 1970-01-01 was a Thursday, which is index 3 in a Monday-first week.
        (self.to_days() + 3).rem_euclid(7) as u32
    }

    pub fn add_days(&self, n: i64) -> Date {
        Date::from_days(self.to_days() + n)
    }

    /// Move by whole months, clamping the day to the end of the target month so
    /// that the 31st of January goes to the 28th of February rather than
    /// silently landing in March.
    pub fn add_months(&self, n: i32) -> Date {
        let total = self.year * 12 + (self.month as i32 - 1) + n;
        let year = total.div_euclid(12);
        let month = total.rem_euclid(12) as u32 + 1;
        let day = self.day.min(Self::days_in_month(year, month));
        Date { year, month, day }
    }

    pub fn first_of_month(&self) -> Date {
        Date { year: self.year, month: self.month, day: 1 }
    }

    /// `2026-08-24`. The storage format and the sort order in one: dates in this
    /// form sort lexicographically the same way they sort chronologically.
    pub fn to_iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn parse_iso(s: &str) -> Option<Date> {
        let b: Vec<&str> = s.trim().split('-').collect();
        if b.len() != 3 {
            return None;
        }
        let d = Date {
            year: b[0].parse().ok()?,
            month: b[1].parse().ok()?,
            day: b[2].parse().ok()?,
        };
        d.is_valid().then_some(d)
    }
}

/// What a typed time string turned out to mean.
///
/// The rule, chosen by the user: **never guess.** An `am`/`pm` marker settles
/// it; failing that a colon means the 24-hour clock; failing that, if the
/// number could genuinely be either, ask rather than pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeParse {
    /// Minutes since midnight.
    At(u32),
    /// Nothing was typed. An entry with no time is an all-day item.
    AllDay,
    /// A bare 1..=12 that could be morning or afternoon. Carries the hour so
    /// the prompt can offer both readings.
    Ambiguous(u32),
    Invalid(&'static str),
}

/// Parse a time the way the user specified.
///
/// Order matters and is not the order it was first described in: `am`/`pm` is
/// checked **before** the colon, because `3:30pm` contains both and letting the
/// colon win would silently produce 03:30 — exactly the quiet wrong answer the
/// rule exists to prevent.
pub fn parse_time(input: &str) -> TimeParse {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return TimeParse::AllDay;
    }
    let s = s.replace(['.', ' '], "");

    // 1. An explicit am/pm marker settles everything.
    for (suffix, pm) in [("am", false), ("pm", true)] {
        if let Some(head) = s.strip_suffix(suffix) {
            let Some((h, m)) = split_hm(head) else {
                return TimeParse::Invalid("use 3pm or 3:30pm");
            };
            if !(1..=12).contains(&h) || m > 59 {
                return TimeParse::Invalid("12-hour times run 1:00 to 12:59");
            }
            // 12am is midnight and 12pm is noon -- the one pair that everyone,
            // including every implementation that gets this wrong, reverses.
            let h24 = match (h, pm) {
                (12, false) => 0,
                (12, true) => 12,
                (h, false) => h,
                (h, true) => h + 12,
            };
            return TimeParse::At(h24 * 60 + m);
        }
    }

    // 2. A colon means the 24-hour clock.
    if s.contains(':') {
        let Some((h, m)) = split_hm(&s) else {
            return TimeParse::Invalid("use 15:00");
        };
        if h > 23 || m > 59 {
            return TimeParse::Invalid("the 24-hour clock stops at 23:59");
        }
        return TimeParse::At(h * 60 + m);
    }

    // 3. A bare number. Only 1..=12 is genuinely ambiguous: 13 and up can only
    //    be the 24-hour clock, and 0 is midnight, so those need no prompt.
    if s.chars().all(|c| c.is_ascii_digit()) {
        if s.len() > 2 {
            // 930 could be 9:30, but reading it that way is a guess. Say so.
            return TimeParse::Invalid("write it with a colon, like 9:30");
        }
        let Ok(h) = s.parse::<u32>() else {
            return TimeParse::Invalid("that is not a time");
        };
        return match h {
            0 => TimeParse::At(0),
            1..=12 => TimeParse::Ambiguous(h),
            13..=23 => TimeParse::At(h * 60),
            _ => TimeParse::Invalid("the 24-hour clock stops at 23:59"),
        };
    }

    TimeParse::Invalid("try 15:00, 3pm, or leave it blank")
}

fn split_hm(s: &str) -> Option<(u32, u32)> {
    match s.split_once(':') {
        Some((h, m)) => {
            if m.len() != 2 || !m.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some((h.parse().ok()?, m.parse().ok()?))
        }
        None => {
            if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some((s.parse().ok()?, 0))
        }
    }
}

/// Minutes since midnight, as `15:00`.
pub fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_years_follow_the_century_rule() {
        assert!(Date::is_leap(2024));
        assert!(!Date::is_leap(2100), "divisible by 100 but not 400");
        assert!(Date::is_leap(2000), "divisible by 400");
        assert_eq!(Date::days_in_month(2024, 2), 29);
        assert_eq!(Date::days_in_month(2026, 2), 28);
    }

    #[test]
    fn epoch_day_zero_is_a_thursday() {
        let d = Date::new(1970, 1, 1);
        assert_eq!(d.to_days(), 0);
        assert_eq!(d.weekday(), 3, "Monday-first, so Thursday is 3");
    }

    #[test]
    fn days_round_trip_across_a_wide_range() {
        // Every day for eight years, including two leap years and a century.
        let mut d = Date::new(1998, 1, 1);
        for _ in 0..(365 * 8) {
            assert_eq!(Date::from_days(d.to_days()), d, "round trip failed at {d:?}");
            assert!(d.is_valid());
            d = d.add_days(1);
        }
    }

    #[test]
    fn known_weekdays() {
        // Spot checks against a real calendar.
        assert_eq!(Date::new(2026, 8, 21).weekday(), 4, "2026-08-21 is a Friday");
        assert_eq!(Date::new(2000, 1, 1).weekday(), 5, "2000-01-01 was a Saturday");
        assert_eq!(Date::new(2024, 2, 29).weekday(), 3, "leap day 2024 was a Thursday");
    }

    #[test]
    fn adding_days_crosses_month_and_year_ends() {
        assert_eq!(Date::new(2026, 1, 31).add_days(1), Date::new(2026, 2, 1));
        assert_eq!(Date::new(2026, 12, 31).add_days(1), Date::new(2027, 1, 1));
        assert_eq!(Date::new(2024, 2, 28).add_days(1), Date::new(2024, 2, 29));
        assert_eq!(Date::new(2026, 2, 28).add_days(1), Date::new(2026, 3, 1));
    }

    #[test]
    fn adding_months_clamps_instead_of_overflowing_into_the_next() {
        // The classic bug: Jan 31 + 1 month landing on March 3.
        assert_eq!(Date::new(2026, 1, 31).add_months(1), Date::new(2026, 2, 28));
        assert_eq!(Date::new(2024, 1, 31).add_months(1), Date::new(2024, 2, 29));
        assert_eq!(Date::new(2026, 3, 31).add_months(-1), Date::new(2026, 2, 28));
    }

    #[test]
    fn adding_months_crosses_years_in_both_directions() {
        assert_eq!(Date::new(2026, 12, 15).add_months(1), Date::new(2027, 1, 15));
        assert_eq!(Date::new(2026, 1, 15).add_months(-1), Date::new(2025, 12, 15));
        assert_eq!(Date::new(2026, 6, 1).add_months(-18), Date::new(2024, 12, 1));
    }

    #[test]
    fn iso_round_trips_and_sorts_chronologically() {
        let d = Date::new(2026, 8, 4);
        assert_eq!(d.to_iso(), "2026-08-04");
        assert_eq!(Date::parse_iso("2026-08-04"), Some(d));
        // Zero padding is what makes string order match date order.
        assert!(Date::new(2026, 8, 4).to_iso() < Date::new(2026, 8, 12).to_iso());
        assert!(Date::new(2026, 9, 1).to_iso() > Date::new(2026, 8, 30).to_iso());
    }

    #[test]
    fn iso_rejects_impossible_dates() {
        assert_eq!(Date::parse_iso("2026-02-30"), None);
        assert_eq!(Date::parse_iso("2026-13-01"), None);
        assert_eq!(Date::parse_iso("not-a-date"), None);
        assert_eq!(Date::parse_iso("2026-08"), None);
    }

    #[test]
    fn am_pm_wins_over_the_colon() {
        // The gap in the rule as first stated: 3:30pm contains a colon, and
        // letting the colon win would quietly mean 03:30.
        assert_eq!(parse_time("3:30pm"), TimeParse::At(15 * 60 + 30));
        assert_eq!(parse_time("3:30am"), TimeParse::At(3 * 60 + 30));
    }

    #[test]
    fn twelve_am_is_midnight_and_twelve_pm_is_noon() {
        assert_eq!(parse_time("12am"), TimeParse::At(0));
        assert_eq!(parse_time("12pm"), TimeParse::At(12 * 60));
        assert_eq!(parse_time("12:30am"), TimeParse::At(30));
        assert_eq!(parse_time("12:30pm"), TimeParse::At(12 * 60 + 30));
    }

    #[test]
    fn a_colon_means_the_24_hour_clock() {
        assert_eq!(parse_time("15:00"), TimeParse::At(15 * 60));
        assert_eq!(parse_time("09:30"), TimeParse::At(9 * 60 + 30));
        assert_eq!(parse_time("0:00"), TimeParse::At(0));
        assert_eq!(parse_time("23:59"), TimeParse::At(23 * 60 + 59));
    }

    #[test]
    fn a_bare_ambiguous_hour_asks_rather_than_guessing() {
        assert_eq!(parse_time("3"), TimeParse::Ambiguous(3));
        assert_eq!(parse_time("11"), TimeParse::Ambiguous(11));
        assert_eq!(parse_time("12"), TimeParse::Ambiguous(12));
    }

    #[test]
    fn bare_hours_that_cannot_be_ambiguous_do_not_ask() {
        // 13 and up can only be the 24-hour clock, so prompting would be noise.
        assert_eq!(parse_time("13"), TimeParse::At(13 * 60));
        assert_eq!(parse_time("23"), TimeParse::At(23 * 60));
        assert_eq!(parse_time("0"), TimeParse::At(0));
    }

    #[test]
    fn nothing_typed_is_an_all_day_entry() {
        assert_eq!(parse_time(""), TimeParse::AllDay);
        assert_eq!(parse_time("   "), TimeParse::AllDay);
    }

    #[test]
    fn spacing_and_punctuation_do_not_change_the_meaning() {
        assert_eq!(parse_time("3 PM"), TimeParse::At(15 * 60));
        assert_eq!(parse_time("3 p.m."), TimeParse::At(15 * 60));
        assert_eq!(parse_time("  15:00  "), TimeParse::At(15 * 60));
    }

    #[test]
    fn nonsense_is_rejected_with_something_to_read() {
        for bad in ["25:00", "12:60", "13pm", "0pm", "abc", "930", "3:5"] {
            match parse_time(bad) {
                TimeParse::Invalid(msg) => assert!(!msg.is_empty(), "{bad} needs a message"),
                other => panic!("{bad} should be rejected, got {other:?}"),
            }
        }
    }

    #[test]
    fn three_digit_numbers_are_refused_rather_than_read_as_h_mm() {
        // 930 "obviously" means 9:30 -- and reading it that way is a guess.
        assert!(matches!(parse_time("930"), TimeParse::Invalid(_)));
    }

    #[test]
    fn formatting_is_the_inverse_of_parsing_for_unambiguous_input() {
        for s in ["00:00", "09:30", "15:00", "23:59"] {
            let TimeParse::At(m) = parse_time(s) else { panic!("{s} should parse") };
            assert_eq!(format_time(m), s);
        }
    }
}
