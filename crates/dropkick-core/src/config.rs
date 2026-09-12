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

    /// Keep the Scratchpad's contents between runs.
    ///
    /// On, because a scratchpad that forgets what you wrote when the daemon
    /// stops is a scratchpad you cannot trust with anything you care about.
    pub scratchpad_persist: bool,

    /// How see-through the panel is, as a percentage. 100 is solid.
    ///
    /// Floored at 20 rather than 0: a panel you cannot see is one you cannot
    /// find your way out of, and the setting would look like a crash. Values
    /// outside the range are clamped rather than rejected, because unlike a
    /// misspelled key this one still says clearly what was meant.
    pub panel_opacity: u32,
}

/// The bounds on [`Config::panel_opacity`], defined here with the setting
/// rather than beside whatever changes it.
///
/// There are two ways in — the file at startup, and `Alt+-`/`Alt+=` while it
/// is running — and if each carried its own idea of the floor, raising one
/// would silently leave the other able to go below it.
pub const OPACITY_MIN: u32 = 20;
pub const OPACITY_MAX: u32 = 100;

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
            scratchpad_persist: true,
            panel_opacity: 100,
        }
    }
}

/// Every `key = value` name present in a settings file, ignoring comments.
fn keys_in(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.split('#').next().unwrap_or("").trim();
            line.split_once('=').map(|(k, _)| k.trim())
        })
        .collect()
}

/// The default file, split into (comment, keys) blocks on blank lines.
///
/// Derived from [`Config::default_file`] rather than duplicated, so there is
/// exactly one list of settings in this crate. Adding a setting to that string
/// is all it takes for an old config file to be offered it.
fn blocks() -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for chunk in Config::default_file().split("\n\n") {
        let mut comment = String::new();
        let mut keys = Vec::new();
        for line in chunk.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                comment.push_str(line);
                comment.push('\n');
            } else if let Some((k, _)) = t.split_once('=') {
                keys.push(k.trim().to_string());
            }
        }
        if !keys.is_empty() {
            out.push((comment, keys));
        }
    }
    out
}

/// The `key = value` line for `key`, exactly as the default file writes it.
fn default_line_for(key: &str) -> Option<String> {
    Config::default_file().lines().find_map(|line| {
        let t = line.trim();
        if t.starts_with('#') {
            return None;
        }
        let (k, _) = t.split_once('=')?;
        (k.trim() == key).then(|| line.to_string())
    })
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
                "scratchpad.persist" => config.scratchpad_persist = on,
                "panel.opacity" => match value.parse::<u32>() {
                    Ok(n) => config.panel_opacity = n.clamp(20, 100),
                    Err(_) => unknown.push(Unknown { line: i + 1, key: key.to_string() }),
                },
                _ => unknown.push(Unknown { line: i + 1, key: key.to_string() }),
            }
        }
        (config, unknown)
    }

    /// Which settings the default file knows about but `existing` does not.
    ///
    /// The point of the settings file is that opening it tells you what the
    /// options are. That only holds for a file written today: a config from
    /// before a setting existed silently gets the default and offers no hint
    /// that there was anything to choose. This is how that property is
    /// restored.
    pub fn missing_from(existing: &str) -> Vec<String> {
        let have: Vec<&str> = keys_in(existing);
        blocks()
            .into_iter()
            .flat_map(|(_, keys)| keys)
            .filter(|k| !have.contains(&k.as_str()))
            .collect()
    }

    /// The text to append to `existing` so it lists every setting again, or
    /// `None` when it already does.
    ///
    /// Each missing key is carried across **with the comment that explains it**,
    /// because a bare `panel.opacity = 100` appearing in someone's file is not
    /// discoverability, it is a puzzle.
    pub fn append_for(existing: &str) -> Option<String> {
        let have = keys_in(existing);
        let mut out = String::new();

        for (comment, keys) in blocks() {
            // Only the absent keys. A block can hold several -- appending the
            // whole block would duplicate the ones already there, and a later
            // duplicate wins in `parse`, so it would quietly overwrite an edit
            // with the default. That is the opposite of the intent.
            let wanted: Vec<&String> = keys.iter().filter(|k| !have.contains(&k.as_str())).collect();
            if wanted.is_empty() {
                continue;
            }
            out.push('\n');
            out.push_str(&comment);
            for k in wanted {
                if let Some(line) = default_line_for(k) {
                    out.push_str(&line);
                    out.push('\n');
                }
            }
        }
        (!out.is_empty()).then(|| {
            format!(
                "\n# ---- added by Dropkick: settings that did not exist when this\n\
                 # ---- file was written. Values shown are the defaults.\n{out}"
            )
        })
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

# Keep what you type in the Scratchpad between runs. It is written to
# ~/.local/share/dropkick/scratchpad.txt, readable only by you. Turn this off
# and the Scratchpad forgets everything when the daemon stops.
scratchpad.persist = true

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

#[cfg(test)]
mod append_tests {
    use super::*;

    /// A config from before several settings existed -- the real one on this
    /// machine, dated before `generate.*` and `panel.opacity`.
    const OLD: &str = "# Dropkick settings. Plain key = value; '#' starts a comment.

# Post a desktop notification when a countdown finishes.
timer.notify = true

# Ask the notification server to play a sound with it.
timer.sound = true

timer.show_panel = true
";

    #[test]
    fn an_old_file_is_told_what_it_is_missing() {
        let missing = Config::missing_from(OLD);
        assert!(missing.contains(&"panel.opacity".to_string()));
        assert!(missing.contains(&"generate.length".to_string()));
        assert!(!missing.contains(&"timer.notify".to_string()));
    }

    #[test]
    fn the_default_file_is_missing_nothing() {
        assert!(Config::missing_from(Config::default_file()).is_empty());
        assert_eq!(Config::append_for(Config::default_file()), None);
    }

    #[test]
    fn appending_produces_a_file_that_parses_clean() {
        let add = Config::append_for(OLD).expect("something should be missing");
        let combined = format!("{OLD}{add}");
        let (_, unknown) = Config::parse(&combined);
        assert!(unknown.is_empty(), "appended text must parse: {unknown:?}");
    }

    #[test]
    fn appending_preserves_the_users_own_edits() {
        // The old file sets timer.show_panel = true, which is not the default.
        // Appending must not reintroduce the key and quietly undo that.
        let add = Config::append_for(OLD).unwrap();
        let combined = format!("{OLD}{add}");
        let (config, _) = Config::parse(&combined);
        assert!(config.timer_show_panel, "the user's own value was overwritten");
    }

    #[test]
    fn appending_only_adds_the_absent_keys_of_a_shared_block() {
        // generate.* live under one comment. If a file has some of them, the
        // present ones must not be repeated -- a later duplicate wins in
        // `parse`, so repeating one would overwrite an edit with the default.
        let partial = format!("{OLD}\ngenerate.length = 64\n");
        let add = Config::append_for(&partial).unwrap();
        assert!(!add.contains("generate.length"), "repeated a key that was present");
        assert!(add.contains("generate.symbols"), "skipped a key that was absent");

        let (config, _) = Config::parse(&format!("{partial}{add}"));
        assert_eq!(config.generate_length, 64, "the edited value did not survive");
    }

    #[test]
    fn appending_is_idempotent() {
        let once = format!("{}{}", OLD, Config::append_for(OLD).unwrap());
        assert_eq!(Config::append_for(&once), None, "a second pass would duplicate keys");
    }

    #[test]
    fn every_appended_key_arrives_with_its_explanation() {
        // A bare `panel.opacity = 100` turning up in someone's file is a
        // puzzle, not discoverability.
        let add = Config::append_for(OLD).unwrap();
        for key in Config::missing_from(OLD) {
            let at = add.find(&key).expect("key should be present");
            let before = &add[..at];
            let last_block = before.rsplit("\n\n").next().unwrap_or("");
            assert!(last_block.contains('#'), "{key} arrived with no comment");
        }
    }

    #[test]
    fn a_commented_out_key_still_counts_as_absent() {
        // Someone who commented a setting out chose the default deliberately,
        // but the file no longer lists it as an option, so offer it again.
        let commented = "# panel.opacity = 50\n";
        assert!(Config::missing_from(commented).contains(&"panel.opacity".to_string()));
    }

    #[test]
    fn an_empty_file_gets_everything() {
        let missing = Config::missing_from("");
        let total = Config::missing_from(Config::default_file()).len();
        assert_eq!(total, 0);
        assert!(missing.len() >= 10, "got {}", missing.len());
    }
}
