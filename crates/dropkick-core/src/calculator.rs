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
        match eval(&expr, self.ans) {
            Ok(value) => {
                self.ans = Some(value);
                self.history.push(Entry { expr, result: format_number(value) });
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
