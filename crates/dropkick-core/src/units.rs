//! Unit conversion for the Calculator.
//!
//! A table of factors, plus temperature, which is not a factor — it has an
//! offset, and treating it like the rest is the classic way to confidently
//! report that 20 °C is 36 °F.
//!
//! Two places where a wrong answer would look right, both resolved by making
//! the ambiguous name refuse to be guessed at:
//!
//! * **Gallons and pints differ between the US and the UK** by about 20%. `gal`
//!   is the US gallon here; the imperial one is `impgal`. Same for pints.
//! * **`kb` is 1000 bytes, not 1024.** The binary units are spelled `kib`,
//!   `mib`, `gib`, as the standard has them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Length,
    Mass,
    Volume,
    Time,
    Data,
    Angle,
    Temperature,
}

impl Dimension {
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Length => "length",
            Dimension::Mass => "mass",
            Dimension::Volume => "volume",
            Dimension::Time => "time",
            Dimension::Data => "data",
            Dimension::Angle => "angle",
            Dimension::Temperature => "temperature",
        }
    }
}

/// A unit, as its size in the dimension's base unit.
///
/// Base units: metre, gram, litre, second, byte, degree. Temperature is not
/// here; it goes through [`convert_temperature`].
struct Unit {
    names: &'static [&'static str],
    dim: Dimension,
    /// How many base units one of these is.
    factor: f64,
}

use Dimension::*;

const UNITS: &[Unit] = &[
    // Length, base metre.
    Unit { names: &["mm", "millimetre", "millimetres", "millimeter", "millimeters"], dim: Length, factor: 0.001 },
    Unit { names: &["cm", "centimetre", "centimetres", "centimeter", "centimeters"], dim: Length, factor: 0.01 },
    Unit { names: &["m", "metre", "metres", "meter", "meters"], dim: Length, factor: 1.0 },
    Unit { names: &["km", "kilometre", "kilometres", "kilometer", "kilometers"], dim: Length, factor: 1000.0 },
    Unit { names: &["in", "inch", "inches"], dim: Length, factor: 0.0254 },
    Unit { names: &["ft", "foot", "feet"], dim: Length, factor: 0.3048 },
    Unit { names: &["yd", "yard", "yards"], dim: Length, factor: 0.9144 },
    Unit { names: &["mi", "mile", "miles"], dim: Length, factor: 1609.344 },
    Unit { names: &["furlong", "furlongs"], dim: Length, factor: 201.168 },
    Unit { names: &["nmi", "nauticalmile", "nauticalmiles"], dim: Length, factor: 1852.0 },
    Unit { names: &["thou", "mil"], dim: Length, factor: 0.0000254 },

    // Mass, base gram.
    Unit { names: &["mg", "milligram", "milligrams"], dim: Mass, factor: 0.001 },
    Unit { names: &["g", "gram", "grams"], dim: Mass, factor: 1.0 },
    Unit { names: &["kg", "kilogram", "kilograms"], dim: Mass, factor: 1000.0 },
    Unit { names: &["tonne", "tonnes", "t"], dim: Mass, factor: 1_000_000.0 },
    Unit { names: &["oz", "ounce", "ounces"], dim: Mass, factor: 28.349523125 },
    Unit { names: &["lb", "lbs", "pound", "pounds"], dim: Mass, factor: 453.59237 },
    Unit { names: &["st", "stone", "stones"], dim: Mass, factor: 6350.29318 },
    Unit { names: &["ton", "tons"], dim: Mass, factor: 907_184.74 }, // US short ton

    // Volume, base litre. US measures by default; see the module note.
    Unit { names: &["ml", "millilitre", "millilitres", "milliliter", "milliliters"], dim: Volume, factor: 0.001 },
    Unit { names: &["l", "litre", "litres", "liter", "liters"], dim: Volume, factor: 1.0 },
    Unit { names: &["tsp", "teaspoon", "teaspoons"], dim: Volume, factor: 0.00492892159375 },
    Unit { names: &["tbsp", "tablespoon", "tablespoons"], dim: Volume, factor: 0.01478676478125 },
    Unit { names: &["floz", "fluidounce", "fluidounces"], dim: Volume, factor: 0.0295735295625 },
    Unit { names: &["cup", "cups"], dim: Volume, factor: 0.2365882365 },
    Unit { names: &["pt", "pint", "pints"], dim: Volume, factor: 0.473176473 },
    Unit { names: &["qt", "quart", "quarts"], dim: Volume, factor: 0.946352946 },
    Unit { names: &["gal", "gallon", "gallons"], dim: Volume, factor: 3.785411784 },
    Unit { names: &["imppt", "imperialpint", "imperialpints"], dim: Volume, factor: 0.56826125 },
    Unit { names: &["impgal", "imperialgallon", "imperialgallons"], dim: Volume, factor: 4.54609 },

    // Time, base second.
    Unit { names: &["ms", "millisecond", "milliseconds"], dim: Time, factor: 0.001 },
    Unit { names: &["s", "sec", "secs", "second", "seconds"], dim: Time, factor: 1.0 },
    Unit { names: &["min", "mins", "minute", "minutes"], dim: Time, factor: 60.0 },
    Unit { names: &["h", "hr", "hrs", "hour", "hours"], dim: Time, factor: 3600.0 },
    Unit { names: &["day", "days"], dim: Time, factor: 86_400.0 },
    Unit { names: &["week", "weeks"], dim: Time, factor: 604_800.0 },
    // No month or year: they are not fixed lengths. Date arithmetic handles
    // those properly, on a calendar, where "a month" means something.

    // Data, base byte. Decimal by default, binary spelled out.
    Unit { names: &["b", "byte", "bytes"], dim: Data, factor: 1.0 },
    Unit { names: &["kb", "kilobyte", "kilobytes"], dim: Data, factor: 1_000.0 },
    Unit { names: &["mb", "megabyte", "megabytes"], dim: Data, factor: 1_000_000.0 },
    Unit { names: &["gb", "gigabyte", "gigabytes"], dim: Data, factor: 1_000_000_000.0 },
    Unit { names: &["tb", "terabyte", "terabytes"], dim: Data, factor: 1e12 },
    Unit { names: &["kib", "kibibyte", "kibibytes"], dim: Data, factor: 1024.0 },
    Unit { names: &["mib", "mebibyte", "mebibytes"], dim: Data, factor: 1_048_576.0 },
    Unit { names: &["gib", "gibibyte", "gibibytes"], dim: Data, factor: 1_073_741_824.0 },
    Unit { names: &["tib", "tebibyte", "tebibytes"], dim: Data, factor: 1_099_511_627_776.0 },

    // Angle, base degree.
    Unit { names: &["deg", "degree", "degrees"], dim: Angle, factor: 1.0 },
    Unit { names: &["rad", "radian", "radians"], dim: Angle, factor: 180.0 / std::f64::consts::PI },
    Unit { names: &["grad", "gradian", "gradians"], dim: Angle, factor: 0.9 },
];

/// Temperature names, kept apart because they do not scale.
const TEMPERATURES: &[(&[&str], char)] = &[
    (&["c", "celsius", "centigrade"], 'c'),
    (&["f", "fahrenheit"], 'f'),
    (&["k", "kelvin"], 'k'),
];

/// Normalise a written unit: lowercase, and strip the separators people put
/// inside multi-word units so that `nautical miles` and `nauticalmiles` agree.
fn normalise(s: &str) -> String {
    s.trim()
        .trim_end_matches('.')
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .collect()
}

fn find(name: &str) -> Option<&'static Unit> {
    let n = normalise(name);
    UNITS.iter().find(|u| u.names.contains(&n.as_str()))
}

fn temperature(name: &str) -> Option<char> {
    let n = normalise(name);
    // A bare degree sign is not enough to say which scale.
    let n = n.trim_start_matches('°').to_string();
    TEMPERATURES.iter().find(|(names, _)| names.contains(&n.as_str())).map(|(_, c)| *c)
}

/// Is this a unit at all? Used to decide whether a trailing word in an
/// expression is a unit or a typo.
pub fn is_unit(name: &str) -> bool {
    find(name).is_some() || temperature(name).is_some()
}

/// The dimension a unit belongs to, for error messages.
pub fn dimension_of(name: &str) -> Option<Dimension> {
    if temperature(name).is_some() {
        return Some(Temperature);
    }
    find(name).map(|u| u.dim)
}

fn convert_temperature(value: f64, from: char, to: char) -> f64 {
    // Via Celsius. Offsets, not factors -- 0 °C is 32 °F, and a conversion
    // written as a plain multiplication gets every temperature wrong except
    // absolute zero.
    let c = match from {
        'f' => (value - 32.0) * 5.0 / 9.0,
        'k' => value - 273.15,
        _ => value,
    };
    match to {
        'f' => c * 9.0 / 5.0 + 32.0,
        'k' => c + 273.15,
        _ => c,
    }
}

/// Convert `value` from one unit to another.
pub fn convert(value: f64, from: &str, to: &str) -> Result<f64, String> {
    if let (Some(a), Some(b)) = (temperature(from), temperature(to)) {
        return Ok(convert_temperature(value, a, b));
    }
    // Mixing a temperature with anything else is a category error, and saying
    // so is more useful than "unknown unit".
    if temperature(from).is_some() || temperature(to).is_some() {
        return Err("cannot convert between temperature and anything else".to_string());
    }

    let Some(a) = find(from) else { return Err(format!("`{}` is not a unit I know", from.trim())) };
    let Some(b) = find(to) else { return Err(format!("`{}` is not a unit I know", to.trim())) };
    if a.dim != b.dim {
        return Err(format!("{} and {} are {} and {}", from.trim(), to.trim(), a.dim.name(), b.dim.name()));
    }
    Ok(value * a.factor / b.factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * b.abs().max(1.0)
    }

    #[test]
    fn the_conversion_from_the_bug_report() {
        // 25 feet is 7.62 m, and a furlong is 201.168 m.
        let v = convert(25.0, "feet", "furlongs").unwrap();
        assert!(close(v, 7.62 / 201.168), "got {v}");
    }

    #[test]
    fn round_trips_return_the_original() {
        for (a, b, v) in [
            ("feet", "m", 25.0),
            ("lb", "kg", 180.0),
            ("gal", "l", 3.0),
            ("mib", "bytes", 4.0),
            ("rad", "deg", 1.5),
        ] {
            let there = convert(v, a, b).unwrap();
            let back = convert(there, b, a).unwrap();
            assert!(close(back, v), "{v} {a} -> {b} -> {a} gave {back}");
        }
    }

    #[test]
    fn known_length_conversions() {
        assert!(close(convert(1.0, "ft", "in").unwrap(), 12.0));
        assert!(close(convert(1.0, "mile", "ft").unwrap(), 5280.0));
        assert!(close(convert(1.0, "furlong", "yards").unwrap(), 220.0));
        assert!(close(convert(1.0, "km", "m").unwrap(), 1000.0));
    }

    #[test]
    fn temperature_uses_the_offset_not_a_factor() {
        // The failure a factor-only table produces: 0 C -> 0 F.
        assert!(close(convert(0.0, "c", "f").unwrap(), 32.0));
        assert!(close(convert(100.0, "c", "f").unwrap(), 212.0));
        assert!(close(convert(98.6, "f", "c").unwrap(), 37.0));
        assert!(close(convert(-40.0, "c", "f").unwrap(), -40.0), "the crossing point");
        assert!(close(convert(0.0, "c", "k").unwrap(), 273.15));
    }

    #[test]
    fn temperature_will_not_mix_with_other_dimensions() {
        assert!(convert(1.0, "c", "m").is_err());
        assert!(convert(1.0, "kg", "f").is_err());
    }

    #[test]
    fn mismatched_dimensions_say_which_is_which() {
        let e = convert(1.0, "kg", "m").unwrap_err();
        assert!(e.contains("mass") && e.contains("length"), "got {e}");
    }

    #[test]
    fn unknown_units_are_named_in_the_error() {
        let e = convert(1.0, "smoots", "m").unwrap_err();
        assert!(e.contains("smoots"), "got {e}");
    }

    #[test]
    fn us_and_imperial_volumes_are_separate_names() {
        let us = convert(1.0, "gal", "l").unwrap();
        let uk = convert(1.0, "impgal", "l").unwrap();
        assert!(close(us, 3.785411784));
        assert!(close(uk, 4.54609));
        assert!(us < uk, "they must not be the same unit under two names");
    }

    #[test]
    fn kilobytes_are_decimal_and_kibibytes_are_binary() {
        assert!(close(convert(1.0, "kb", "bytes").unwrap(), 1000.0));
        assert!(close(convert(1.0, "kib", "bytes").unwrap(), 1024.0));
        assert!(close(convert(1.0, "gib", "mib").unwrap(), 1024.0));
    }

    #[test]
    fn names_are_case_and_punctuation_insensitive() {
        assert!(close(convert(1.0, "FT", "IN").unwrap(), 12.0));
        assert!(close(convert(1.0, "Nautical Miles", "m").unwrap(), 1852.0));
        assert!(close(convert(1.0, "nautical-miles", "m").unwrap(), 1852.0));
    }

    #[test]
    fn months_and_years_are_not_units_of_time() {
        // They are not fixed lengths. Date arithmetic handles them on a
        // calendar, where the answer is defined.
        assert!(!is_unit("month"));
        assert!(!is_unit("year"));
        assert!(is_unit("week"));
    }

    #[test]
    fn every_name_in_the_table_resolves_and_is_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for u in UNITS {
            for n in u.names {
                assert!(find(n).is_some(), "{n} does not resolve");
                assert!(!seen.contains(n), "{n} is listed twice");
                seen.push(n);
            }
        }
        for (names, _) in TEMPERATURES {
            for n in *names {
                assert!(temperature(n).is_some(), "{n} does not resolve");
                assert!(!seen.contains(n), "{n} collides with a scaled unit");
            }
        }
    }
}
