//! Calculator: a line of input, a recursive-descent evaluator, and a history.
//!
//! No letter commands. The calculator accepts free text, so every printable key
//! belongs to the expression — which is precisely why tab switching had to live
//! on a modifier.

use crate::key::{Key, KeyPress};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub expr: String,
    pub result: String,
}

#[derive(Debug, Default)]
pub struct Calculator {
    pub input: String,
    pub history: Vec<Entry>,
    pub error: Option<String>,
    /// Result of the last successful evaluation, reachable as `ans`.
    pub ans: Option<f64>,
    /// Today's date, for `today`, `tomorrow` and `yesterday`. Supplied by the
    /// platform on summon like everything else date-shaped; `None` until then,
    /// which makes those words simply not resolve rather than resolve wrongly.
    pub today: Option<crate::date::Date>,
}

impl Calculator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when the key was consumed.
    pub fn on_key(&mut self, press: KeyPress) -> bool {
        if !press.mods.is_plain() {
            return false;
        }
        match press.key {
            Key::Char(c) => {
                self.input.push(c);
                self.error = None;
                true
            }
            Key::Backspace => {
                self.input.pop();
                self.error = None;
                true
            }
            Key::Enter => {
                self.evaluate();
                true
            }
            _ => false,
        }
    }

    pub fn evaluate(&mut self) {
        let expr = self.input.trim().to_string();
        if expr.is_empty() {
            return;
        }
        match evaluate_line(&expr, self.ans, self.today) {
            Ok(out) => {
                // A date answer leaves `ans` alone: there is no sensible number
                // for it, and silently setting it to a day count would make the
                // next `ans` mean something the user never saw.
                if let Some(v) = out.ans {
                    self.ans = Some(v);
                }
                self.history.push(Entry { expr, result: out.display });
                self.input.clear();
                self.error = None;
                // The panel is small; old history is not worth the memory.
                if self.history.len() > 64 {
                    self.history.remove(0);
                }
            }
            Err(msg) => self.error = Some(msg),
        }
    }
}

/// Render a float the way a person would write it.
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() {
        "0".to_string()
    } else {
        s
    }
}

// --- units and dates -------------------------------------------------------
//
// These sit *in front of* the arithmetic evaluator rather than inside it. The
// evaluator deals in `f64` and knows nothing about metres or Tuesdays; adding
// units to its grammar would mean every value carrying a dimension through
// every operator. Dispatching on the shape of the line keeps `2+2` on exactly
// the path it has always taken.

/// What evaluating a line produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    /// What to show in the history.
    pub display: String,
    /// What `ans` becomes, when the answer is a plain number. A date is not,
    /// so `ans` is left alone rather than being set to something meaningless.
    pub ans: Option<f64>,
}

/// Split on the last ` in ` or ` to `.
///
/// The *last*, deliberately: `25 in in cm` is 25 inches in centimetres, and
/// splitting on the first separator would read the unit as the keyword and
/// leave `in cm` as the target.
fn split_conversion(s: &str) -> Option<(&str, &str)> {
    let a = s.rfind(" in ").map(|i| (i, 4));
    let b = s.rfind(" to ").map(|i| (i, 4));
    let (at, len) = match (a, b) {
        (Some(x), Some(y)) => {
            if x.0 > y.0 {
                x
            } else {
                y
            }
        }
        (Some(x), None) => x,
        (None, Some(y)) => y,
        (None, None) => return None,
    };
    Some((&s[..at], &s[at + len..]))
}

/// `<expression> <unit> in <unit>`, e.g. `25 feet in furlongs`, `(20+5) ft to m`.
fn try_conversion(input: &str, ans: Option<f64>) -> Option<Result<Evaluated, String>> {
    let (left, right) = split_conversion(input)?;
    let to = right.trim();
    if to.is_empty() || !crate::units::is_unit(to) {
        return None;
    }
    // The source unit is the trailing word of the left side; everything before
    // it is an ordinary expression, so `(20+5) ft in m` works.
    let left = left.trim();
    let (head, from) = if crate::units::is_unit(left) {
        // The whole left side is a unit, with no quantity: `ft in cm`. There is
        // no separator to split on, so splitting would fail and the line would
        // fall through to the arithmetic parser and die on `unknown name ft`.
        ("", left)
    } else {
        let cut = left.rfind(|c: char| c.is_whitespace() || c == ')' || c.is_ascii_digit())?;
        left.split_at(cut + 1)
    };
    let from = from.trim();
    if from.is_empty() || !crate::units::is_unit(from) {
        return None;
    }
    let head = head.trim();
    // A bare unit with no quantity means one of them: `ft in cm`.
    let value = if head.is_empty() { Ok(1.0) } else { eval(head, ans) };
    Some(value.and_then(|v| {
        crate::units::convert(v, from, to)
            .map(|out| Evaluated { display: format!("{} {}", format_number(out), to), ans: Some(out) })
    }))
}

/// A date literal: ISO, or one of the three words worth having.
fn parse_date_word(s: &str, today: Option<crate::date::Date>) -> Option<crate::date::Date> {
    let t = s.trim().to_ascii_lowercase();
    match t.as_str() {
        "today" => today,
        "tomorrow" => today.map(|d| d.add_days(1)),
        "yesterday" => today.map(|d| d.add_days(-1)),
        _ => crate::date::Date::parse_iso(&t),
    }
}

/// Whole-calendar periods. Months and years are not fixed lengths, which is
/// exactly why they belong here and not in the unit table.
fn parse_period(s: &str) -> Option<(f64, &'static str)> {
    let t = s.trim().to_ascii_lowercase();
    let (n, word) = match t.split_once(char::is_whitespace) {
        Some((n, w)) => (n.trim().parse::<f64>().ok()?, w.trim().to_string()),
        // `+ 1week` and a bare `+ week` are not worth supporting; a number and
        // a word is the whole grammar.
        None => return None,
    };
    let unit = match word.trim_end_matches('s') {
        "day" => "day",
        "week" => "week",
        "month" => "month",
        "year" => "year",
        _ => return None,
    };
    Some((n, unit))
}

fn show_date(d: crate::date::Date) -> String {
    const FULL: [&str; 7] =
        ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    format!("{} ({})", d.to_iso(), FULL[d.weekday() as usize])
}

/// Date arithmetic: `date + n days`, `date - n weeks`, `date - date`, or a bare
/// date, which answers "what day of the week is that".
fn try_date(input: &str, today: Option<crate::date::Date>) -> Option<Result<Evaluated, String>> {
    let s = input.trim();

    // Split on the last + or - that has a space before it, so that an ISO
    // date's own hyphens are not mistaken for the operator.
    let op_at = s
        .char_indices()
        .rfind(|(i, c)| (*c == '+' || *c == '-') && *i > 0 && s.as_bytes()[i - 1] == b' ');

    let Some((at, op)) = op_at else {
        // No operator: a bare date reports its weekday.
        let d = parse_date_word(s, today)?;
        return Some(Ok(Evaluated { display: show_date(d), ans: None }));
    };

    let left = s[..at].trim();
    let right = s[at + 1..].trim();
    let Some(a) = parse_date_word(left, today) else {
        // Not a date at all -- let the arithmetic evaluator have it.
        return None;
    };

    // date - date, in days.
    if op == '-' {
        if let Some(b) = parse_date_word(right, today) {
            let days = a.to_days() - b.to_days();
            let plural = if days.abs() == 1 { "day" } else { "days" };
            return Some(Ok(Evaluated {
                display: format!("{days} {plural}"),
                ans: Some(days as f64),
            }));
        }
    }

    let Some((n, unit)) = parse_period(right) else {
        return Some(Err(format!("`{right}` is not a number of days, weeks, months or years")));
    };
    if n.fract() != 0.0 {
        return Some(Err("whole periods only: 3 days, not 3.5".to_string()));
    }
    let n = if op == '-' { -n } else { n } as i64;
    let out = match unit {
        "day" => a.add_days(n),
        "week" => a.add_days(n * 7),
        "month" => a.add_months(n as i32),
        _ => a.add_months(n as i32 * 12),
    };
    Some(Ok(Evaluated { display: show_date(out), ans: None }))
}

/// Evaluate a line: a conversion, a date, or plain arithmetic, in that order.
pub fn evaluate_line(
    input: &str,
    ans: Option<f64>,
    today: Option<crate::date::Date>,
) -> Result<Evaluated, String> {
    if let Some(r) = try_conversion(input, ans) {
        return r;
    }
    if let Some(r) = try_date(input, today) {
        return r;
    }
    eval(input, ans).map(|v| Evaluated { display: format_number(v), ans: Some(v) })
}

// --- evaluator -------------------------------------------------------------

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    ans: Option<f64>,
}

pub fn eval(input: &str, ans: Option<f64>) -> Result<f64, String> {
    let mut p = Parser { src: input.as_bytes(), pos: 0, ans };
    let v = p.expr()?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(format!("unexpected `{}`", p.rest()));
    }
    if v.is_nan() {
        return Err("not a number".to_string());
    }
    Ok(v)
}

impl<'a> Parser<'a> {
    fn rest(&self) -> String {
        String::from_utf8_lossy(&self.src[self.pos..]).to_string()
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.src.get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                let rhs = self.power()?;
                if rhs == 0.0 {
                    return Err("division by zero".to_string());
                }
                v /= rhs;
            } else if self.eat(b'%') {
                let rhs = self.power()?;
                if rhs == 0.0 {
                    return Err("division by zero".to_string());
                }
                v %= rhs;
            } else {
                return Ok(v);
            }
        }
    }

    /// Right-associative, so 2^3^2 is 512 rather than 64.
    fn power(&mut self) -> Result<f64, String> {
        let base = self.unary()?;
        if self.eat(b'^') {
            let exp = self.power()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat(b'-') {
            return Ok(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<f64, String> {
        match self.peek() {
            None => Err("unexpected end of expression".to_string()),
            Some(b'(') => {
                self.pos += 1;
                let v = self.expr()?;
                if !self.eat(b')') {
                    return Err("missing `)`".to_string());
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() => self.ident(),
            Some(c) => Err(format!("unexpected `{}`", c as char)),
        }
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while self
            .src
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.' || *c == b'_')
        {
            self.pos += 1;
        }
        let raw: String = String::from_utf8_lossy(&self.src[start..self.pos]).replace('_', "");
        raw.parse::<f64>().map_err(|_| format!("bad number `{raw}`"))
    }

    fn ident(&mut self) -> Result<f64, String> {
        let start = self.pos;
        while self.src.get(self.pos).is_some_and(|c| c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let name = String::from_utf8_lossy(&self.src[start..self.pos]).to_lowercase();
        match name.as_str() {
            "pi" => Ok(std::f64::consts::PI),
            "e" => Ok(std::f64::consts::E),
            "ans" => self.ans.ok_or_else(|| "no previous answer".to_string()),
            other => {
                // Function call: sqrt(2), abs(-3)...
                if self.eat(b'(') {
                    let arg = self.expr()?;
                    if !self.eat(b')') {
                        return Err("missing `)`".to_string());
                    }
                    return match other {
                        "sqrt" => Ok(arg.sqrt()),
                        "abs" => Ok(arg.abs()),
                        "ln" => Ok(arg.ln()),
                        "log" => Ok(arg.log10()),
                        "sin" => Ok(arg.sin()),
                        "cos" => Ok(arg.cos()),
                        "tan" => Ok(arg.tan()),
                        "round" => Ok(arg.round()),
                        "floor" => Ok(arg.floor()),
                        "ceil" => Ok(arg.ceil()),
                        _ => Err(format!("unknown function `{other}`")),
                    };
                }
                Err(format!("unknown name `{other}`"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(s: &str) -> f64 {
        eval(s, None).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(e("437*1.18"), 515.66);
        assert_eq!(e("2+3*4"), 14.0);
        assert_eq!(e("(2+3)*4"), 20.0);
        assert_eq!(e("453/17"), 453.0 / 17.0);
        assert_eq!(e("-4+10"), 6.0);
        assert_eq!(e("10 % 3"), 1.0);
    }

    #[test]
    fn power_is_right_associative() {
        assert_eq!(e("2^3^2"), 512.0);
        assert_eq!(e("2^10"), 1024.0);
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(e("sqrt(16)"), 4.0);
        assert_eq!(e("abs(0-7)"), 7.0);
        assert!((e("pi") - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn ans_refers_to_previous_result() {
        assert_eq!(eval("ans*2", Some(21.0)).unwrap(), 42.0);
        assert!(eval("ans", None).is_err());
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        assert!(eval("1/0", None).is_err());
        assert!(eval("(1+2", None).is_err());
        assert!(eval("2+", None).is_err());
        assert!(eval("frobnicate(2)", None).is_err());
        assert!(eval("", None).is_err());
    }

    #[test]
    fn formatting_drops_noise() {
        assert_eq!(format_number(4.0), "4");
        assert_eq!(format_number(515.66), "515.66");
        assert_eq!(format_number(-0.5), "-0.5");
    }

    #[test]
    fn enter_moves_input_into_history() {
        let mut c = Calculator::new();
        for ch in "437*1.18".chars() {
            c.on_key(KeyPress::ch(ch));
        }
        c.on_key(KeyPress::plain(Key::Enter));
        assert!(c.input.is_empty());
        assert_eq!(c.history.len(), 1);
        assert_eq!(c.history[0].result, "515.66");
        assert_eq!(c.ans, Some(515.66));
    }

    #[test]
    fn bad_expression_keeps_input_for_repair() {
        let mut c = Calculator::new();
        for ch in "2+".chars() {
            c.on_key(KeyPress::ch(ch));
        }
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.input, "2+");
        assert!(c.error.is_some());
        assert!(c.history.is_empty());
    }

    #[test]
    fn alt_chords_are_never_consumed_as_text() {
        let mut c = Calculator::new();
        assert!(!c.on_key(KeyPress::alt('e')));
        assert!(c.input.is_empty());
    }
}

#[cfg(test)]
mod unit_and_date_tests {
    use super::*;
    use crate::date::Date;

    fn today() -> Option<Date> {
        Some(Date::new(2026, 8, 22))
    }

    fn ev(s: &str) -> String {
        evaluate_line(s, None, today()).unwrap().display
    }

    fn err(s: &str) -> String {
        evaluate_line(s, None, today()).unwrap_err()
    }

    #[test]
    fn the_line_from_the_bug_report_now_works() {
        assert_eq!(ev("25 feet in furlongs"), "0.0378787879 furlongs");
    }

    #[test]
    fn plain_arithmetic_is_untouched() {
        // The whole risk of this change: 2+2 must not take a new path.
        assert_eq!(ev("2+2"), "4");
        assert_eq!(ev("437*1.18"), "515.66");
        assert_eq!(ev("sqrt(16)"), "4");
        assert_eq!(ev("(3+4)*2"), "14");
    }

    #[test]
    fn conversions_accept_in_or_to() {
        assert_eq!(ev("1 ft in in"), "12 in");
        assert_eq!(ev("1 ft to in"), "12 in");
    }

    #[test]
    fn the_unit_abbreviation_in_is_not_mistaken_for_the_keyword() {
        // `25 in in cm` is 25 inches. Splitting on the first ` in ` would read
        // the unit as the keyword and leave `in cm` as the target.
        assert_eq!(ev("25 in in cm"), "63.5 cm");
    }

    #[test]
    fn an_expression_can_precede_the_unit() {
        assert_eq!(ev("(20+5) ft in ft"), "25 ft");
        assert_eq!(ev("2*3 kg in g"), "6000 g");
    }

    #[test]
    fn a_bare_unit_means_one_of_them() {
        assert_eq!(ev("ft in cm"), "30.48 cm");
    }

    #[test]
    fn temperature_conversions_use_the_offset() {
        assert_eq!(ev("98.6 F in C"), "37 C");
        assert_eq!(ev("0 c in f"), "32 f");
    }

    #[test]
    fn a_bad_conversion_explains_itself() {
        assert!(err("1 kg in m").contains("mass"));
        assert!(err("1 m in smoots").contains("smoots"));
    }

    #[test]
    fn date_plus_days_gives_a_date_and_its_weekday() {
        assert_eq!(ev("2026-08-22 + 90 days"), "2026-11-20 (Friday)");
        assert_eq!(ev("2026-08-22 + 1 day"), "2026-08-23 (Sunday)");
    }

    #[test]
    fn date_minus_periods_goes_backwards() {
        assert_eq!(ev("2026-08-22 - 1 week"), "2026-08-15 (Saturday)");
        assert_eq!(ev("2026-01-31 + 1 month"), "2026-02-28 (Saturday)");
        assert_eq!(ev("2026-08-22 - 1 year"), "2025-08-22 (Friday)");
    }

    #[test]
    fn date_minus_date_is_a_count_of_days() {
        assert_eq!(ev("2026-12-25 - 2026-08-22"), "125 days");
        assert_eq!(ev("2026-08-23 - 2026-08-22"), "1 day", "singular");
        assert_eq!(ev("2026-08-22 - 2026-12-25"), "-125 days", "and it can be negative");
    }

    #[test]
    fn today_tomorrow_and_yesterday_resolve() {
        assert_eq!(ev("today"), "2026-08-22 (Saturday)");
        assert_eq!(ev("tomorrow"), "2026-08-23 (Sunday)");
        assert_eq!(ev("yesterday"), "2026-08-21 (Friday)");
        assert_eq!(ev("2026-12-25 - today"), "125 days");
    }

    #[test]
    fn a_bare_date_answers_what_day_of_the_week() {
        assert_eq!(ev("2000-01-01"), "2000-01-01 (Saturday)");
    }

    #[test]
    fn an_iso_dates_own_hyphens_are_not_read_as_subtraction() {
        // The trap: `2026-08-22` is three numbers and two minus signs to an
        // arithmetic parser, and would quietly evaluate to 1996.
        assert_eq!(ev("2026-08-22"), "2026-08-22 (Saturday)");
    }

    #[test]
    fn a_date_answer_does_not_overwrite_ans() {
        let mut c = Calculator::new();
        c.today = today();
        for ch in "10*10".chars() {
            c.on_key(KeyPress::ch(ch));
        }
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.ans, Some(100.0));
        for ch in "today".chars() {
            c.on_key(KeyPress::ch(ch));
        }
        c.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(c.ans, Some(100.0), "a date is not a number; ans must survive");
    }

    #[test]
    fn a_conversion_sets_ans_to_the_converted_value() {
        let mut c = Calculator::new();
        for ch in "1 ft in in".chars() {
            c.on_key(KeyPress::ch(ch));
        }
        c.on_key(KeyPress::plain(Key::Enter));
        // Not exactly 12.0: 0.3048/0.0254 has no exact binary representation.
        // The display rounds it to "12 in"; `ans` keeps the real value.
        assert!((c.ans.unwrap() - 12.0).abs() < 1e-9, "got {:?}", c.ans);
        assert_eq!(c.history[0].result, "12 in");
    }

    #[test]
    fn date_words_do_not_resolve_before_the_platform_supplies_a_date() {
        // Better to fail than to invent a day.
        assert!(evaluate_line("today", None, None).is_err());
    }

    #[test]
    fn a_bad_period_says_what_it_wanted() {
        assert!(err("today + 3 fortnights").contains("days, weeks, months or years"));
        assert!(err("today + 3.5 days").contains("whole periods"));
    }

    #[test]
    fn words_that_are_not_units_still_reach_the_arithmetic_error() {
        // `in` present but neither side a unit: this must not be swallowed by
        // the conversion path.
        let e = err("hello in there");
        assert!(e.contains("hello"), "got {e}");
    }
}
