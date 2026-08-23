//! Colour: converting between the ways a colour gets written down.
//!
//! Eight-bit sRGB throughout. No alpha: this exists to answer "what is that
//! colour in hex", and an alpha channel that silently rounds through HSL and
//! back would be a way to lose information rather than a feature.
//!
//! HSL is stored as it is computed rather than kept alongside the RGB, so a
//! round trip through HSL and back can move a channel by one. That is inherent
//! to landing on 256 steps, not a bug, and it is why RGB is the source of truth
//! here and HSL is only ever a rendering of it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn to_rgb_string(self) -> String {
        format!("rgb({}, {}, {})", self.r, self.g, self.b)
    }

    /// Hue in degrees, saturation and lightness as percentages.
    pub fn to_hsl(self) -> (f64, f64, f64) {
        let (r, g, b) = (self.r as f64 / 255.0, self.g as f64 / 255.0, self.b as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            // Grey. Hue is undefined; reporting 0 is the convention.
            return (0.0, 0.0, l * 100.0);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        ((h + 360.0) % 360.0, s * 100.0, l * 100.0)
    }

    pub fn to_hsl_string(self) -> String {
        let (h, s, l) = self.to_hsl();
        format!("hsl({}, {}%, {}%)", h.round() as i64, s.round() as i64, l.round() as i64)
    }

    pub fn from_hsl(h: f64, s: f64, l: f64) -> Color {
        let (h, s, l) = (h.rem_euclid(360.0), (s / 100.0).clamp(0.0, 1.0), (l / 100.0).clamp(0.0, 1.0));
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;
        let (r, g, b) = match h as u32 / 60 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let f = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Color::new(f(r), f(g), f(b))
    }

    /// Relative luminance, per WCAG. Used by [`Self::prefers_dark_text`].
    pub fn luminance(self) -> f64 {
        let f = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(self.r) + 0.7152 * f(self.g) + 0.0722 * f(self.b)
    }

    /// Should text drawn on this colour be black rather than white?
    ///
    /// The crossover is not a matter of taste. WCAG contrast against white is
    /// `1.05 / (L + 0.05)` and against black is `(L + 0.05) / 0.05`; setting
    /// them equal gives `L = sqrt(1.05 * 0.05) - 0.05`, about 0.179. Above
    /// that, black wins.
    ///
    /// Worth deriving rather than eyeballing: a guessed threshold of 0.35 put
    /// white on tomato, which is 3:1 where black would have been 7:1.
    pub fn prefers_dark_text(self) -> bool {
        // sqrt(1.05 * 0.05) - 0.05, written out so the derivation is visible.
        let crossover = (1.05f64 * 0.05).sqrt() - 0.05;
        self.luminance() > crossover
    }

    /// The nearest CSS colour name, and whether it is exact.
    pub fn nearest_name(self) -> (&'static str, bool) {
        let mut best = ("black", u32::MAX);
        for (name, r, g, b) in NAMED {
            let d = (self.r as i32 - *r as i32).pow(2)
                + (self.g as i32 - *g as i32).pow(2)
                + (self.b as i32 - *b as i32).pow(2);
            if (d as u32) < best.1 {
                best = (name, d as u32);
            }
        }
        (best.0, best.1 == 0)
    }
}

/// Parse a colour written any of the usual ways.
///
/// Returns `None` rather than a guess. A colour tool that turns a typo into
/// some nearby colour is worse than one that says it does not understand.
pub fn parse(input: &str) -> Option<Color> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }

    // A name.
    let compact: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if let Some((_, r, g, b)) = NAMED.iter().find(|(n, ..)| *n == compact) {
        return Some(Color::new(*r, *g, *b));
    }

    // Functional notation: rgb(...) or hsl(...).
    if let Some(rest) = s.strip_prefix("rgb") {
        let n = numbers(rest)?;
        if n.len() != 3 {
            return None;
        }
        // Values outside 0..=255 are a mistake, not something to clamp
        // silently -- rgb(300, 0, 0) is not red.
        if n.iter().any(|v| *v < 0.0 || *v > 255.0 || v.fract() != 0.0) {
            return None;
        }
        return Some(Color::new(n[0] as u8, n[1] as u8, n[2] as u8));
    }
    if let Some(rest) = s.strip_prefix("hsl") {
        let n = numbers(rest)?;
        if n.len() != 3 {
            return None;
        }
        return Some(Color::from_hsl(n[0], n[1], n[2]));
    }

    // Hex, with or without the hash.
    let hex = s.strip_prefix('#').unwrap_or(&s);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        // #abc means #aabbcc.
        3 => Some(Color::new(d(0)? * 17, d(1)? * 17, d(2)? * 17)),
        6 => Some(Color::new(p(0)?, p(2)?, p(4)?)),
        _ => None,
    }
}

/// Pull the numbers out of `(1, 2, 3)` or ` 1 2 3 `, ignoring `%` and commas.
fn numbers(s: &str) -> Option<Vec<f64>> {
    let cleaned: String = s
        .chars()
        .map(|c| if c == ',' || c == '(' || c == ')' || c == '%' { ' ' } else { c })
        .collect();
    let mut out = Vec::new();
    for tok in cleaned.split_whitespace() {
        out.push(tok.parse::<f64>().ok()?);
    }
    (!out.is_empty()).then_some(out)
}

// --- the tool ---------------------------------------------------------------

use crate::key::{Key, KeyPress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Ask the platform to run the screen colour picker.
    Pick,
    /// Put this on the clipboard.
    Copy(String),
    Redraw,
    Ignored,
}

#[derive(Debug, Default)]
pub struct ColorTool {
    /// Free text, like the Calculator: every printable key belongs to it, so
    /// the eyedropper has to live on a key that cannot be typed.
    pub input: String,
    pub current: Option<Color>,
    pub problem: Option<&'static str>,
    /// True while the platform is waiting on the screen picker, so the panel
    /// can say why nothing is happening.
    pub picking: bool,
    pub notice: Option<String>,
    /// Whether the eyedropper is offered at all. See [`Self::new`].
    pub eyedropper: bool,
}

impl ColorTool {
    /// The eyedropper is **off**, and the switch is one line.
    ///
    /// It is written, tested, and correct: the portal builds the Request path
    /// the match rule predicts, the signal arrives, and the reply decodes. What
    /// it decodes to is a refusal — COSMIC returns response code 2 in about a
    /// millisecond, showing no picker, for every client that asks. Measured
    /// 2026-08-23; the exchange is in `docs/PROJECT_LOG.md`.
    ///
    /// So the key is not offered rather than offered and broken. Set this true
    /// to try it again — if COSMIC ships an implementation, that is the whole
    /// change, and everything behind it still works.
    pub fn new() -> Self {
        ColorTool { eyedropper: false, ..Self::default() }
    }

    pub fn set(&mut self, c: Color) {
        self.current = Some(c);
        self.problem = None;
        self.picking = false;
        self.input.clear();
    }

    /// The picker was cancelled or failed. Distinct from a colour arriving:
    /// leaving `picking` set would strand the panel saying "pick a colour".
    pub fn picking_ended(&mut self, why: Option<&'static str>) {
        self.picking = false;
        self.problem = why;
    }

    pub fn on_enter(&mut self) {
        self.problem = None;
        self.notice = None;
        // A picker left running across a summon cannot be recovered, and the
        // reply would arrive for a panel that has moved on.
        self.picking = false;
    }

    pub fn on_key(&mut self, press: KeyPress) -> Outcome {
        if !press.mods.is_plain() {
            return Outcome::Ignored;
        }
        self.notice = None;
        match press.key {
            Key::Char(c) => {
                self.input.push(c);
                self.problem = None;
                Outcome::Redraw
            }
            Key::Backspace => {
                self.input.pop();
                self.problem = None;
                Outcome::Redraw
            }
            Key::Enter => {
                if self.input.trim().is_empty() {
                    return Outcome::Ignored;
                }
                match parse(&self.input) {
                    Some(c) => self.set(c),
                    None => self.problem = Some("try #6FB3F2, rgb(111,179,242), hsl(210,82%,69%) or a name"),
                }
                Outcome::Redraw
            }
            // Tab, not a letter: every printable key is part of the input.
            Key::Tab => {
                if !self.eyedropper {
                    // Say why rather than doing nothing. A key that is
                    // documented and then silently inert is worse than one that
                    // explains itself.
                    self.problem = Some("the eyedropper is off: this desktop refuses colour picking");
                    return Outcome::Redraw;
                }
                self.picking = true;
                self.problem = None;
                Outcome::Pick
            }
            // Copy the hex, which is the form people paste somewhere else.
            Key::Delete => Outcome::Ignored,
            _ => Outcome::Ignored,
        }
    }

    /// `y` cannot copy -- it is a letter, and letters are input. Copying is
    /// therefore driven by the app layer through this, on Ctrl+Y.
    pub fn copy_hex(&mut self) -> Outcome {
        match self.current {
            Some(c) => {
                self.notice = Some(format!("{} copied", c.to_hex()));
                Outcome::Copy(c.to_hex())
            }
            None => Outcome::Ignored,
        }
    }

    /// Esc clears what has been typed before leaving the tool.
    pub fn leave_one_level(&mut self) -> bool {
        if self.picking {
            self.picking = false;
            return true;
        }
        if !self.input.is_empty() {
            self.input.clear();
            self.problem = None;
            return true;
        }
        false
    }
}

/// The CSS named colours. A static table, like the unit table: a colour tool
/// that cannot resolve `tomato` is missing the part people actually use.
#[rustfmt::skip]
pub const NAMED: &[(&str, u8, u8, u8)] = &[
    ("aliceblue",240,248,255), ("antiquewhite",250,235,215), ("aqua",0,255,255),
    ("aquamarine",127,255,212), ("azure",240,255,255), ("beige",245,245,220),
    ("bisque",255,228,196), ("black",0,0,0), ("blanchedalmond",255,235,205),
    ("blue",0,0,255), ("blueviolet",138,43,226), ("brown",165,42,42),
    ("burlywood",222,184,135), ("cadetblue",95,158,160), ("chartreuse",127,255,0),
    ("chocolate",210,105,30), ("coral",255,127,80), ("cornflowerblue",100,149,237),
    ("cornsilk",255,248,220), ("crimson",220,20,60), ("cyan",0,255,255),
    ("darkblue",0,0,139), ("darkcyan",0,139,139), ("darkgoldenrod",184,134,11),
    ("darkgray",169,169,169), ("darkgreen",0,100,0), ("darkgrey",169,169,169),
    ("darkkhaki",189,183,107), ("darkmagenta",139,0,139), ("darkolivegreen",85,107,47),
    ("darkorange",255,140,0), ("darkorchid",153,50,204), ("darkred",139,0,0),
    ("darksalmon",233,150,122), ("darkseagreen",143,188,143), ("darkslateblue",72,61,139),
    ("darkslategray",47,79,79), ("darkslategrey",47,79,79), ("darkturquoise",0,206,209),
    ("darkviolet",148,0,211), ("deeppink",255,20,147), ("deepskyblue",0,191,255),
    ("dimgray",105,105,105), ("dimgrey",105,105,105), ("dodgerblue",30,144,255),
    ("firebrick",178,34,34), ("floralwhite",255,250,240), ("forestgreen",34,139,34),
    ("fuchsia",255,0,255), ("gainsboro",220,220,220), ("ghostwhite",248,248,255),
    ("gold",255,215,0), ("goldenrod",218,165,32), ("gray",128,128,128),
    ("green",0,128,0), ("greenyellow",173,255,47), ("grey",128,128,128),
    ("honeydew",240,255,240), ("hotpink",255,105,180), ("indianred",205,92,92),
    ("indigo",75,0,130), ("ivory",255,255,240), ("khaki",240,230,140),
    ("lavender",230,230,250), ("lavenderblush",255,240,245), ("lawngreen",124,252,0),
    ("lemonchiffon",255,250,205), ("lightblue",173,216,230), ("lightcoral",240,128,128),
    ("lightcyan",224,255,255), ("lightgoldenrodyellow",250,250,210), ("lightgray",211,211,211),
    ("lightgreen",144,238,144), ("lightgrey",211,211,211), ("lightpink",255,182,193),
    ("lightsalmon",255,160,122), ("lightseagreen",32,178,170), ("lightskyblue",135,206,250),
    ("lightslategray",119,136,153), ("lightslategrey",119,136,153), ("lightsteelblue",176,196,222),
    ("lightyellow",255,255,224), ("lime",0,255,0), ("limegreen",50,205,50),
    ("linen",250,240,230), ("magenta",255,0,255), ("maroon",128,0,0),
    ("mediumaquamarine",102,205,170), ("mediumblue",0,0,205), ("mediumorchid",186,85,211),
    ("mediumpurple",147,112,219), ("mediumseagreen",60,179,113), ("mediumslateblue",123,104,238),
    ("mediumspringgreen",0,250,154), ("mediumturquoise",72,209,204), ("mediumvioletred",199,21,133),
    ("midnightblue",25,25,112), ("mintcream",245,255,250), ("mistyrose",255,228,225),
    ("moccasin",255,228,181), ("navajowhite",255,222,173), ("navy",0,0,128),
    ("oldlace",253,245,230), ("olive",128,128,0), ("olivedrab",107,142,35),
    ("orange",255,165,0), ("orangered",255,69,0), ("orchid",218,112,214),
    ("palegoldenrod",238,232,170), ("palegreen",152,251,152), ("paleturquoise",175,238,238),
    ("palevioletred",219,112,147), ("papayawhip",255,239,213), ("peachpuff",255,218,185),
    ("peru",205,133,63), ("pink",255,192,203), ("plum",221,160,221),
    ("powderblue",176,224,230), ("purple",128,0,128), ("rebeccapurple",102,51,153),
    ("red",255,0,0), ("rosybrown",188,143,143), ("royalblue",65,105,225),
    ("saddlebrown",139,69,19), ("salmon",250,128,114), ("sandybrown",244,164,96),
    ("seagreen",46,139,87), ("seashell",255,245,238), ("sienna",160,82,45),
    ("silver",192,192,192), ("skyblue",135,206,235), ("slateblue",106,90,205),
    ("slategray",112,128,144), ("slategrey",112,128,144), ("snow",255,250,250),
    ("springgreen",0,255,127), ("steelblue",70,130,180), ("tan",210,180,140),
    ("teal",0,128,128), ("thistle",216,191,216), ("tomato",255,99,71),
    ("turquoise",64,224,208), ("violet",238,130,238), ("wheat",245,222,179),
    ("white",255,255,255), ("whitesmoke",245,245,245), ("yellow",255,255,0),
    ("yellowgreen",154,205,50),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_with_and_without_the_hash() {
        let c = Color::new(0x6F, 0xB3, 0xF2);
        assert_eq!(parse("#6FB3F2"), Some(c));
        assert_eq!(parse("6fb3f2"), Some(c));
        assert_eq!(parse("  #6fb3f2  "), Some(c));
    }

    #[test]
    fn short_hex_doubles_each_digit() {
        assert_eq!(parse("#abc"), Some(Color::new(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse("#fff"), Some(Color::new(255, 255, 255)));
        assert_eq!(parse("#000"), Some(Color::new(0, 0, 0)));
    }

    #[test]
    fn functional_notation_parses() {
        assert_eq!(parse("rgb(255, 99, 71)"), Some(Color::new(255, 99, 71)));
        assert_eq!(parse("rgb 255 99 71"), Some(Color::new(255, 99, 71)));
        assert_eq!(parse("hsl(9, 100%, 64%)"), Some(Color::new(255, 99, 71)));
    }

    #[test]
    fn names_resolve_including_the_awkward_one() {
        assert_eq!(parse("tomato"), Some(Color::new(255, 99, 71)));
        assert_eq!(parse("rebeccapurple"), Some(Color::new(102, 51, 153)));
        assert_eq!(parse("Rebecca Purple"), Some(Color::new(102, 51, 153)));
        assert_eq!(parse("cornflower-blue"), Some(Color::new(100, 149, 237)));
    }

    #[test]
    fn nonsense_is_refused_rather_than_approximated() {
        for bad in ["", "notacolour", "#12", "#1234567", "rgb(1,2)", "#gg0000", "rgb(300,0,0)"] {
            assert_eq!(parse(bad), None, "{bad} should not parse");
        }
    }

    #[test]
    fn out_of_range_rgb_is_refused_not_clamped() {
        // rgb(300, 0, 0) is a mistake. Clamping it to red would hide that.
        assert_eq!(parse("rgb(300, 0, 0)"), None);
        assert_eq!(parse("rgb(-1, 0, 0)"), None);
        assert_eq!(parse("rgb(1.5, 0, 0)"), None);
    }

    #[test]
    fn hex_output_is_uppercase_and_padded() {
        assert_eq!(Color::new(0, 0, 0).to_hex(), "#000000");
        assert_eq!(Color::new(255, 99, 71).to_hex(), "#FF6347");
        assert_eq!(Color::new(1, 2, 3).to_hex(), "#010203");
    }

    #[test]
    fn hsl_matches_known_values() {
        let (h, s, l) = Color::new(255, 0, 0).to_hsl();
        assert!((h - 0.0).abs() < 0.5 && (s - 100.0).abs() < 0.5 && (l - 50.0).abs() < 0.5);
        let (h, s, l) = Color::new(0, 128, 0).to_hsl();
        assert!((h - 120.0).abs() < 0.5 && (s - 100.0).abs() < 0.5 && (l - 25.1).abs() < 0.5);
    }

    #[test]
    fn grey_reports_zero_hue_rather_than_a_division_by_zero() {
        let (h, s, l) = Color::new(128, 128, 128).to_hsl();
        assert_eq!(h, 0.0);
        assert_eq!(s, 0.0);
        assert!((l - 50.2).abs() < 0.5);
        // And white and black, the other two places d == 0.
        assert_eq!(Color::new(255, 255, 255).to_hsl().1, 0.0);
        assert_eq!(Color::new(0, 0, 0).to_hsl().2, 0.0);
    }

    #[test]
    fn hsl_round_trips_within_one_step_of_255() {
        // Not exact, and cannot be: HSL is continuous and this lands on 256
        // steps. Within one is the honest guarantee.
        for c in [
            Color::new(255, 99, 71),
            Color::new(0x6F, 0xB3, 0xF2),
            Color::new(1, 2, 3),
            Color::new(0, 0, 0),
            Color::new(255, 255, 255),
            Color::new(17, 200, 90),
        ] {
            let (h, s, l) = c.to_hsl();
            let back = Color::from_hsl(h, s, l);
            let d = |a: u8, b: u8| (a as i32 - b as i32).abs();
            assert!(
                d(c.r, back.r) <= 1 && d(c.g, back.g) <= 1 && d(c.b, back.b) <= 1,
                "{c:?} -> hsl -> {back:?}"
            );
        }
    }

    #[test]
    fn every_named_colour_parses_back_to_itself() {
        for (name, r, g, b) in NAMED {
            assert_eq!(parse(name), Some(Color::new(*r, *g, *b)), "{name}");
        }
    }

    #[test]
    fn named_colours_are_sorted_and_unique() {
        // Sorted so the table stays greppable; unique so a duplicate cannot
        // shadow a different colour.
        let names: Vec<&str> = NAMED.iter().map(|(n, ..)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "the table is out of alphabetical order");
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "a name is listed twice");
    }

    #[test]
    fn an_exact_name_is_reported_as_exact() {
        assert_eq!(Color::new(255, 99, 71).nearest_name(), ("tomato", true));
        let (name, exact) = Color::new(255, 100, 71).nearest_name();
        assert_eq!(name, "tomato");
        assert!(!exact, "one off is not the same colour");
    }

    fn tool_type(t: &mut ColorTool, s: &str) {
        for ch in s.chars() {
            t.on_key(KeyPress::ch(ch));
        }
    }

    #[test]
    fn typing_a_colour_and_pressing_enter_sets_it() {
        let mut t = ColorTool::new();
        tool_type(&mut t, "tomato");
        t.on_key(KeyPress::plain(Key::Enter));
        assert_eq!(t.current, Some(Color::new(255, 99, 71)));
        assert!(t.input.is_empty(), "the input clears once it is accepted");
    }

    #[test]
    fn a_colour_that_does_not_parse_keeps_the_text_for_repair() {
        let mut t = ColorTool::new();
        tool_type(&mut t, "#gg0000");
        t.on_key(KeyPress::plain(Key::Enter));
        assert!(t.current.is_none());
        assert!(t.problem.is_some());
        assert_eq!(t.input, "#gg0000", "do not make them type it again");
    }

    #[test]
    fn tab_asks_for_the_eyedropper_and_says_it_is_waiting() {
        let mut t = ColorTool::new();
        t.eyedropper = true;
        assert_eq!(t.on_key(KeyPress::plain(Key::Tab)), Outcome::Pick);
        assert!(t.picking);
    }

    #[test]
    fn the_eyedropper_is_off_by_default() {
        // COSMIC refuses the portal call, so the key is not offered. Everything
        // behind it still works; this is the one line that turns it back on.
        let t = ColorTool::new();
        assert!(!t.eyedropper);
    }

    #[test]
    fn tab_explains_itself_when_the_eyedropper_is_off() {
        let mut t = ColorTool::new();
        assert_eq!(t.on_key(KeyPress::plain(Key::Tab)), Outcome::Redraw);
        assert!(!t.picking, "it must not pretend to be picking");
        assert!(t.problem.is_some(), "a documented key must not be silently inert");
    }

    #[test]
    fn a_cancelled_pick_does_not_strand_the_panel() {
        let mut t = ColorTool::new();
        t.eyedropper = true;
        t.on_key(KeyPress::plain(Key::Tab));
        t.picking_ended(None);
        assert!(!t.picking, "otherwise it says 'pick a colour' forever");
        assert!(t.problem.is_none(), "cancelling is not an error");
    }

    #[test]
    fn letters_are_input_not_commands() {
        // The whole reason the eyedropper is on Tab: `t` has to type a `t`.
        let mut t = ColorTool::new();
        tool_type(&mut t, "teal");
        assert_eq!(t.input, "teal");
        assert!(!t.picking);
    }

    #[test]
    fn copying_reports_the_hex_and_does_nothing_without_a_colour() {
        let mut t = ColorTool::new();
        assert_eq!(t.copy_hex(), Outcome::Ignored);
        t.set(Color::new(255, 99, 71));
        assert_eq!(t.copy_hex(), Outcome::Copy("#FF6347".to_string()));
        assert!(t.notice.is_some());
    }

    #[test]
    fn esc_clears_the_input_before_leaving_the_tool() {
        let mut t = ColorTool::new();
        tool_type(&mut t, "half typed");
        assert!(t.leave_one_level());
        assert!(t.input.is_empty());
        assert!(!t.leave_one_level(), "and then the ladder is out of rungs");
    }

    #[test]
    fn esc_cancels_a_pending_pick_first() {
        let mut t = ColorTool::new();
        t.eyedropper = true;
        t.on_key(KeyPress::plain(Key::Tab));
        assert!(t.leave_one_level());
        assert!(!t.picking);
    }

    #[test]
    fn summoning_does_not_resume_a_pick_that_can_never_answer() {
        let mut t = ColorTool::new();
        t.eyedropper = true;
        t.on_key(KeyPress::plain(Key::Tab));
        t.on_enter();
        assert!(!t.picking);
    }

    #[test]
    fn text_on_a_swatch_picks_whichever_actually_contrasts() {
        // Each of these was checked against the WCAG contrast formula, not by
        // looking at it: tomato is the one a guessed threshold got wrong.
        for (c, dark) in [
            (Color::new(255, 99, 71), true),   // tomato, 7.0:1 black vs 3.0:1 white
            (Color::new(255, 255, 255), true), // white
            (Color::new(0, 0, 0), false),      // black
            (Color::new(27, 42, 74), false),   // midnight-ish
            (Color::new(255, 255, 0), true),   // yellow
            (Color::new(0, 0, 255), false),    // blue
        ] {
            assert_eq!(c.prefers_dark_text(), dark, "{c:?}");
        }
    }

    #[test]
    fn the_text_crossover_is_where_the_two_contrasts_are_equal() {
        // Derived, so assert the derivation: just either side of the crossover
        // the choice flips, and at that point the two contrasts agree.
        let contrast_white = |l: f64| 1.05 / (l + 0.05);
        let contrast_black = |l: f64| (l + 0.05) / 0.05;
        let l = (1.05f64 * 0.05).sqrt() - 0.05;
        assert!((contrast_white(l) - contrast_black(l)).abs() < 1e-6);
    }

    #[test]
    fn luminance_orders_black_below_white() {
        assert!(Color::new(0, 0, 0).luminance() < Color::new(128, 128, 128).luminance());
        assert!(Color::new(128, 128, 128).luminance() < Color::new(255, 255, 255).luminance());
        // Green carries most of the perceived brightness, which is the whole
        // point of using luminance rather than an average.
        assert!(Color::new(0, 255, 0).luminance() > Color::new(0, 0, 255).luminance());
    }
}
