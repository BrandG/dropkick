//! Generate: passwords and UUIDs.
//!
//! **Nothing here is stored.** There is no history, no "last generated", and
//! the value does not survive a summon — the panel comes back blank. That is
//! the whole reason this tool can exist in something that also keeps clipboard
//! history: a secret that was never written down cannot leak from a list.
//!
//! Randomness does not live here. Like time in [`crate::timer`], it enters as a
//! parameter and is never read inside, which is what makes generation testable
//! against fixed bytes instead of "run it a lot and squint". The platform fills
//! the pool from the OS; this module only spends it.

use crate::key::{Key, KeyPress};

/// Characters excluded when "avoid lookalikes" is on. The set people actually
/// confuse when reading a password off a screen and typing it somewhere else.
const AMBIGUOUS: &str = "lI1O0";

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
/// Deliberately not the full ASCII punctuation set: quotes, backslashes and
/// backticks get mangled by shells, CSV and half of the web's input validation.
const SYMBOLS: &str = "!@#$%^&*-_=+?";

pub const MIN_LEN: usize = 4;
pub const MAX_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Password,
    Uuid,
}

/// What the tool wants the app to do about a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Copy this to the clipboard *as a secret* — it must not be recorded in
    /// clipboard history, here or by anything else on the system.
    CopySecret(String),
    Redraw,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classes {
    pub lower: bool,
    pub upper: bool,
    pub digits: bool,
    pub symbols: bool,
}

impl Classes {
    fn any(&self) -> bool {
        self.lower || self.upper || self.digits || self.symbols
    }
}

#[derive(Debug)]
pub struct Generate {
    pub kind: Kind,
    pub length: usize,
    pub classes: Classes,
    pub exclude_ambiguous: bool,
    /// The value on screen. Empty until something is generated, and emptied
    /// again on summon.
    pub value: String,
    /// Set when the alphabet is empty, so the panel can say why rather than
    /// showing nothing and looking broken.
    pub problem: Option<&'static str>,
    /// Unspent random bytes. The platform tops this up; see [`Self::wants_entropy`].
    pool: Vec<u8>,
    /// True once a copy has happened, so the panel can say so.
    pub copied: bool,
}

impl Default for Generate {
    fn default() -> Self {
        Generate {
            kind: Kind::Password,
            length: 20,
            classes: Classes { lower: true, upper: true, digits: true, symbols: true },
            exclude_ambiguous: false,
            value: String::new(),
            problem: None,
            pool: Vec::new(),
            copied: false,
        }
    }
}

impl Generate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the settings file. Called on summon, like every other setting.
    pub fn configure(&mut self, length: usize, classes: Classes, exclude_ambiguous: bool) {
        self.length = length.clamp(MIN_LEN, MAX_LEN);
        self.classes = classes;
        self.exclude_ambiguous = exclude_ambiguous;
    }

    /// How many bytes the pool wants. The platform refills on this, so a long
    /// password never finds itself half-generated and out of randomness.
    pub fn wants_entropy(&self) -> Option<usize> {
        const LOW: usize = 256;
        const FULL: usize = 1024;
        (self.pool.len() < LOW).then(|| FULL - self.pool.len())
    }

    pub fn supply_entropy(&mut self, bytes: &[u8]) {
        self.pool.extend_from_slice(bytes);
    }

    /// The alphabet the current settings describe.
    fn alphabet(&self) -> String {
        let mut out = String::new();
        for (on, set) in [
            (self.classes.lower, LOWER),
            (self.classes.upper, UPPER),
            (self.classes.digits, DIGITS),
            (self.classes.symbols, SYMBOLS),
        ] {
            if on {
                out.push_str(set);
            }
        }
        if self.exclude_ambiguous {
            out.retain(|c| !AMBIGUOUS.contains(c));
        }
        out
    }

    /// Take one byte, or `None` if the pool is dry.
    fn byte(&mut self) -> Option<u8> {
        if self.pool.is_empty() {
            return None;
        }
        Some(self.pool.remove(0))
    }

    /// Pick an index below `n` without modulo bias.
    ///
    /// `byte % n` favours the low indices whenever `n` does not divide 256 --
    /// with 62 characters, the first eight are ~1.6% likelier than the rest.
    /// Small, but it is a bias in a password generator, and rejection sampling
    /// removes it for the cost of occasionally drawing another byte.
    fn index_below(&mut self, n: usize) -> Option<usize> {
        debug_assert!(n > 0 && n <= 256);
        let limit = 256 - (256 % n);
        loop {
            let b = self.byte()? as usize;
            if b < limit {
                return Some(b % n);
            }
        }
    }

    pub fn regenerate(&mut self) {
        self.copied = false;
        self.problem = None;
        match self.kind {
            Kind::Uuid => self.generate_uuid(),
            Kind::Password => self.generate_password(),
        }
    }

    fn generate_password(&mut self) {
        if !self.classes.any() {
            self.value.clear();
            self.problem = Some("no character types selected - press a A 0 !");
            return;
        }
        let alphabet: Vec<char> = self.alphabet().chars().collect();
        if alphabet.is_empty() {
            self.value.clear();
            self.problem = Some("every character was excluded - turn off x");
            return;
        }
        let mut out = String::with_capacity(self.length);
        for _ in 0..self.length {
            let Some(i) = self.index_below(alphabet.len()) else {
                // Out of randomness. Show nothing rather than something short:
                // a truncated password that looks fine is the worst outcome.
                self.value.clear();
                self.problem = Some("out of randomness - try again");
                return;
            };
            out.push(alphabet[i]);
        }
        self.value = out;
    }

    /// UUID v4, per RFC 4122: 122 random bits, with the version and variant
    /// fields overwritten.
    fn generate_uuid(&mut self) {
        let mut b = [0u8; 16];
        for slot in b.iter_mut() {
            let Some(v) = self.byte() else {
                self.value.clear();
                self.problem = Some("out of randomness - try again");
                return;
            };
            *slot = v;
        }
        b[6] = (b[6] & 0x0F) | 0x40; // version 4
        b[8] = (b[8] & 0x3F) | 0x80; // variant 1
        let hex = |s: &[u8]| -> String {
            s.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
        };
        self.value = format!(
            "{}-{}-{}-{}-{}",
            hex(&b[0..4]),
            hex(&b[4..6]),
            hex(&b[6..8]),
            hex(&b[8..10]),
            hex(&b[10..16])
        );
    }

    /// Summoning clears the value. A password left on screen from twenty
    /// minutes ago is a hazard, not a convenience, and this tool promises that
    /// nothing is kept.
    pub fn on_enter(&mut self) {
        self.value.clear();
        self.problem = None;
        self.copied = false;
    }

    pub fn on_key(&mut self, press: KeyPress) -> Outcome {
        if !press.mods.is_plain() {
            return Outcome::Ignored;
        }
        match press.key {
            // Kinds. Lowercase letters, so they cannot collide with the class
            // toggles below -- `p` for password can never also mean
            // punctuation, which is the mistake `s` made in the Timer.
            Key::Char('p') | Key::Char('u') => {
                let kind =
                    if press.key == Key::Char('p') { Kind::Password } else { Kind::Uuid };
                if self.kind != kind {
                    self.kind = kind;
                    self.value.clear();
                    self.problem = None;
                    self.copied = false;
                }
                self.regenerate();
                Outcome::Redraw
            }
            // Character classes, each named for what it adds.
            Key::Char('a') | Key::Char('A') | Key::Char('0') | Key::Char('!') => {
                if self.kind != Kind::Password {
                    return Outcome::Ignored;
                }
                match press.key {
                    Key::Char('a') => self.classes.lower = !self.classes.lower,
                    Key::Char('A') => self.classes.upper = !self.classes.upper,
                    Key::Char('0') => self.classes.digits = !self.classes.digits,
                    _ => self.classes.symbols = !self.classes.symbols,
                }
                self.regenerate();
                Outcome::Redraw
            }
            Key::Char('x') => {
                if self.kind != Kind::Password {
                    return Outcome::Ignored;
                }
                self.exclude_ambiguous = !self.exclude_ambiguous;
                self.regenerate();
                Outcome::Redraw
            }
            Key::Char('+') | Key::Char('=') | Key::Right => {
                if self.kind != Kind::Password {
                    return Outcome::Ignored;
                }
                self.length = (self.length + 1).min(MAX_LEN);
                self.regenerate();
                Outcome::Redraw
            }
            Key::Char('-') | Key::Char('_') | Key::Left => {
                if self.kind != Kind::Password {
                    return Outcome::Ignored;
                }
                self.length = self.length.saturating_sub(1).max(MIN_LEN);
                self.regenerate();
                Outcome::Redraw
            }
            Key::Char('r') | Key::Char(' ') => {
                self.regenerate();
                Outcome::Redraw
            }
            Key::Char('y') | Key::Enter => {
                if self.value.is_empty() {
                    return Outcome::Ignored;
                }
                self.copied = true;
                Outcome::CopySecret(self.value.clone())
            }
            _ => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool of predictable bytes. Every value 0..=255 in order, repeated --
    /// enough for rejection sampling to be exercised rather than avoided.
    fn seeded() -> Generate {
        let mut g = Generate::new();
        let bytes: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        g.supply_entropy(&bytes);
        g
    }

    #[test]
    fn password_has_the_requested_length() {
        let mut g = seeded();
        g.length = 32;
        g.regenerate();
        assert_eq!(g.value.chars().count(), 32);
    }

    #[test]
    fn password_uses_only_the_selected_classes() {
        let mut g = seeded();
        g.classes = Classes { lower: true, upper: false, digits: false, symbols: false };
        g.length = 64;
        g.regenerate();
        assert!(!g.value.is_empty());
        assert!(g.value.chars().all(|c| c.is_ascii_lowercase()), "got {}", g.value);
    }

    #[test]
    fn excluding_ambiguous_removes_every_lookalike() {
        let mut g = seeded();
        g.exclude_ambiguous = true;
        g.length = 128;
        g.regenerate();
        assert!(!g.value.is_empty());
        for c in AMBIGUOUS.chars() {
            assert!(!g.value.contains(c), "{c} survived in {}", g.value);
        }
    }

    #[test]
    fn turning_every_class_off_explains_itself() {
        let mut g = seeded();
        g.classes = Classes { lower: false, upper: false, digits: false, symbols: false };
        g.regenerate();
        assert!(g.value.is_empty());
        assert!(g.problem.is_some(), "an empty panel must say why it is empty");
    }

    #[test]
    fn an_empty_pool_produces_nothing_rather_than_something_short() {
        let mut g = Generate::new();
        g.supply_entropy(&[7, 9, 11]);
        g.length = 40;
        g.regenerate();
        assert!(g.value.is_empty(), "a truncated password must never be shown");
        assert!(g.problem.is_some());
    }

    #[test]
    fn rejection_sampling_does_not_bias_the_low_indices() {
        // With a 62-character alphabet, 256 % 62 == 8, so bytes 248..=255 must
        // be rejected. Feeding only those and then a usable byte proves they
        // were skipped rather than folded down onto 'a'..'h'.
        let mut g = Generate::new();
        g.classes = Classes { lower: true, upper: true, digits: true, symbols: false };
        g.exclude_ambiguous = false;
        g.length = 1;
        g.supply_entropy(&[248, 249, 250, 255, 61]);
        g.regenerate();
        let alphabet: Vec<char> = g.alphabet().chars().collect();
        assert_eq!(alphabet.len(), 62);
        assert_eq!(g.value.chars().next(), Some(alphabet[61]));
    }

    #[test]
    fn uuid_has_the_right_shape_and_version() {
        let mut g = seeded();
        g.kind = Kind::Uuid;
        g.regenerate();
        let v = g.value.clone();
        let parts: Vec<&str> = v.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert!(v.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{v}");
        assert_eq!(parts[2].chars().next(), Some('4'), "version nibble");
        let variant = parts[3].chars().next().unwrap();
        assert!("89ab".contains(variant), "variant nibble was {variant}");
    }

    #[test]
    fn uuid_ignores_the_password_only_keys() {
        let mut g = seeded();
        g.kind = Kind::Uuid;
        g.regenerate();
        let before = g.value.clone();
        for c in ['a', 'A', '0', '!', 'x', '+', '-'] {
            assert_eq!(g.on_key(KeyPress::ch(c)), Outcome::Ignored);
        }
        assert_eq!(g.value, before, "a UUID has no length or character classes");
    }

    #[test]
    fn copy_asks_for_a_secret_copy() {
        let mut g = seeded();
        g.regenerate();
        let value = g.value.clone();
        assert_eq!(g.on_key(KeyPress::ch('y')), Outcome::CopySecret(value));
        assert!(g.copied);
    }

    #[test]
    fn copying_nothing_does_nothing() {
        let mut g = seeded();
        assert!(g.value.is_empty());
        assert_eq!(g.on_key(KeyPress::ch('y')), Outcome::Ignored);
    }

    #[test]
    fn summoning_clears_the_value() {
        let mut g = seeded();
        g.regenerate();
        assert!(!g.value.is_empty());
        g.on_enter();
        assert!(g.value.is_empty(), "a password must not still be on screen later");
        assert!(!g.copied);
    }

    #[test]
    fn regenerating_gives_a_different_value() {
        let mut g = seeded();
        g.regenerate();
        let first = g.value.clone();
        g.on_key(KeyPress::ch('r'));
        assert_ne!(g.value, first);
    }

    #[test]
    fn length_is_clamped_at_both_ends() {
        let mut g = seeded();
        g.length = MIN_LEN;
        g.on_key(KeyPress::ch('-'));
        assert_eq!(g.length, MIN_LEN);
        g.length = MAX_LEN;
        g.on_key(KeyPress::ch('+'));
        assert_eq!(g.length, MAX_LEN);
    }

    #[test]
    fn changing_a_setting_regenerates_rather_than_leaving_a_stale_value() {
        let mut g = seeded();
        g.regenerate();
        let before = g.value.clone();
        g.on_key(KeyPress::ch('A'));
        assert_ne!(g.value, before, "the value on screen must match the settings shown");
    }

    #[test]
    fn switching_kind_and_back_does_not_show_the_old_password() {
        let mut g = seeded();
        g.on_key(KeyPress::ch('p'));
        let password = g.value.clone();
        g.on_key(KeyPress::ch('u'));
        assert_ne!(g.value, password);
        g.on_key(KeyPress::ch('p'));
        assert_ne!(g.value, password, "a fresh password, not the previous one");
    }

    #[test]
    fn the_pool_asks_to_be_refilled_before_it_runs_out() {
        let mut g = Generate::new();
        assert!(g.wants_entropy().is_some(), "an empty pool must ask");
        g.supply_entropy(&vec![0u8; 1024]);
        assert!(g.wants_entropy().is_none(), "a full pool must not");
    }
}
