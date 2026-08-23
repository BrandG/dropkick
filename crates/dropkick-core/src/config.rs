//! Settings.
//!
//! A plain `key = value` file, parsed here so it can be tested without touching
//! a disk. No TOML, no serde, no derive: the format is meant to be edited in
//! any text editor and understood without documentation, and it costs the
//! project no dependency to read.
//!
//! Unknown keys are **reported, not ignored**. A settings file that silently
//! swallows `timer.sond = false` is worse than one that has no settings at all,
//! because it looks like it worked.

/// Everything Dropkick can be told to do differently. Defaults are what the
/// tool does with no config file at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Post a desktop notification when a countdown finishes.
    pub timer_notify: bool,
    /// Ask the notification server to play the alarm sound.
    pub timer_sound: bool,
    /// Bring the panel up when a countdown finishes.
    ///
    /// Off by default, and worth knowing why before turning it on: an overlay
    /// mapping can take focus away from whatever you were using, even though
    /// Dropkick never receives the keyboard itself. It also covers the top of
    /// the screen. A critical notification already waits for you, so this is
    /// only for people who want an alarm that is unmissable rather than merely
    /// patient, and who can live with the interruption.
    pub timer_show_panel: bool,

    /// Default password length. Clamped to the generator's limits on use, so a
    /// nonsense value in the file cannot produce a nonsense password.
    pub generate_length: usize,
    pub generate_lower: bool,
    pub generate_upper: bool,
    pub generate_digits: bool,
    pub generate_symbols: bool,
    /// Leave out characters that are easy to misread: l I 1 O 0.
    pub generate_exclude_ambiguous: bool,

    /// How see-through the panel is, as a percentage. 100 is solid.
    ///
    /// Floored at 20 rather than 0: a panel you cannot see is one you cannot
    /// find your way out of, and the setting would look like a crash. Values
    /// outside the range are clamped rather than rejected, because unlike a
    /// misspelled key this one still says clearly what was meant.
    pub panel_opacity: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timer_notify: true,
            timer_sound: true,
            timer_show_panel: false,
            generate_length: 20,
            generate_lower: true,
            generate_upper: true,
            generate_digits: true,
            generate_symbols: true,
            generate_exclude_ambiguous: false,
            panel_opacity: 100,
        }
    }
}

/// A key that was not recognised, with the line it was on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    pub line: usize,
    pub key: String,
}

impl Config {
    /// Parse a settings file. Returns the config plus anything unrecognised,
    /// so the caller can complain rather than quietly doing the default.
    pub fn parse(text: &str) -> (Config, Vec<Unknown>) {
        let mut config = Config::default();
        let mut unknown = Vec::new();

        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                unknown.push(Unknown { line: i + 1, key: line.to_string() });
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            let on = matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            );

            match key {
                "timer.notify" => config.timer_notify = on,
                "timer.sound" => config.timer_sound = on,
                "timer.show_panel" => config.timer_show_panel = on,
                "generate.length" => match value.parse::<usize>() {
                    Ok(n) => config.generate_length = n,
                    // A number that is not a number is reported like an unknown
                    // key rather than silently left at the default -- same rule,
                    // same reason.
                    Err(_) => unknown.push(Unknown { line: i + 1, key: key.to_string() }),
                },
                "generate.lower" => config.generate_lower = on,
                "generate.upper" => config.generate_upper = on,
                "generate.digits" => config.generate_digits = on,
                "generate.symbols" => config.generate_symbols = on,
                "generate.exclude_ambiguous" => config.generate_exclude_ambiguous = on,
                "panel.opacity" => match value.parse::<u32>() {
                    Ok(n) => config.panel_opacity = n.clamp(20, 100),
                    Err(_) => unknown.push(Unknown { line: i + 1, key: key.to_string() }),
                },
                _ => unknown.push(Unknown { line: i + 1, key: key.to_string() }),
            }
        }
        (config, unknown)
    }

    /// The file written on first run: every setting, at its default, with a
    /// line saying what it does. Discoverability without a manual.
    pub fn default_file() -> &'static str {
        "\
# Dropkick settings. Plain key = value; '#' starts a comment.
# Delete this file to go back to the defaults below.

# Post a desktop notification when a countdown finishes.
timer.notify = true

# Ask the notification server to play a sound with it.
timer.sound = true

# Also bring the panel up when a countdown finishes. Off because it covers
# the top of the screen and can pull focus away from what you were doing,
# and the notification above already waits for you. Turn it on if you want
# an alarm that is unmissable rather than merely patient.
timer.show_panel = false

# Default password length in Generate, and which character types it draws
# from. Nothing generated is ever stored or copied into clipboard history.
generate.length = 20
generate.lower = true
generate.upper = true
generate.digits = true
generate.symbols = true

# Leave out characters that are easy to misread when typing a password
# somewhere else by hand: l I 1 O 0.
generate.exclude_ambiguous = false

# How see-through the panel is, as a percentage. 100 is solid. Lower lets you
# read what is behind it -- handy when you are copying something off the window
# underneath. Values below 20 are treated as 20, so it cannot become invisible.
panel.opacity = 100
"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults() {
        let (c, unknown) = Config::parse("");
        assert_eq!(c, Config::default());
        assert!(unknown.is_empty());
    }

    #[test]
    fn settings_can_be_turned_off() {
        let (c, unknown) = Config::parse("timer.sound = false\ntimer.notify = false");
        assert!(!c.timer_sound);
        assert!(!c.timer_notify);
        assert!(unknown.is_empty());
    }

    #[test]
    fn untouched_settings_keep_their_defaults() {
        let (c, _) = Config::parse("timer.sound = false");
        assert!(c.timer_notify, "notify is on by default");
        assert!(!c.timer_show_panel, "the panel is off by default");
    }

    #[test]
    fn the_panel_can_be_turned_on() {
        let (c, unknown) = Config::parse("timer.show_panel = true");
        assert!(c.timer_show_panel);
        assert!(unknown.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# a comment\n\n  timer.sound = false  # trailing comment\n";
        let (c, unknown) = Config::parse(text);
        assert!(!c.timer_sound);
        assert!(unknown.is_empty());
    }

    #[test]
    fn the_usual_spellings_of_yes_all_work() {
        for yes in ["true", "TRUE", "yes", "on", "1"] {
            let (c, _) = Config::parse(&format!("timer.sound = {yes}"));
            assert!(c.timer_sound, "{yes} should mean on");
        }
        for no in ["false", "no", "off", "0", "", "banana"] {
            let (c, _) = Config::parse(&format!("timer.sound = {no}"));
            assert!(!c.timer_sound, "{no:?} should mean off");
        }
    }

    #[test]
    fn a_typo_is_reported_rather_than_swallowed() {
        // The point of the Unknown list: `timer.sond = false` must not look
        // like it worked.
        let (c, unknown) = Config::parse("timer.sond = false");
        assert!(c.timer_sound, "the real setting keeps its default");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].key, "timer.sond");
        assert_eq!(unknown[0].line, 1);
    }

    #[test]
    fn a_line_with_no_equals_is_reported_with_its_number() {
        let (_, unknown) = Config::parse("timer.sound = true\nnonsense here\n");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].line, 2);
    }

    #[test]
    fn the_default_file_parses_to_the_defaults() {
        // Otherwise the file written on first run would not describe what the
        // program actually does.
        let (c, unknown) = Config::parse(Config::default_file());
        assert_eq!(c, Config::default());
        assert!(unknown.is_empty(), "the shipped file must not contain a typo: {unknown:?}");
    }
}
