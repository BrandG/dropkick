//! Finding, loading and reloading the settings file.
//!
//! The parsing lives in `dropkick-core` where it can be tested; this is only
//! the part that needs a filesystem.

use std::path::PathBuf;
use std::time::SystemTime;

use dropkick_core::app::Nudge;
use dropkick_core::config::{Config, OPACITY_MAX, OPACITY_MIN};

/// How far one press of `Alt+-` / `Alt+=` moves `panel.opacity`, in percent.
///
/// The step lives on this side and the *bounds* live with the setting in
/// `config`. The core raises only a direction: it owns the keyboard, so it
/// has to know the panel has an opacity, but it cannot see a panel and has no
/// business deciding how much a press is worth.
pub const OPACITY_STEP: u32 = 5;

/// One step of opacity, clamped at both ends.
///
/// A free function so it can be tested: its only caller needs a live Wayland
/// connection to exist, and the arithmetic here is the part that has to be
/// right.
///
/// **The floor is the point.** A panel faded to nothing still holds the
/// keyboard and still swallows what you type, and the only way out would be
/// the summon key. Key repeat means a held `Alt+-` reaches the bottom in
/// under a second, so this cannot be left to the user noticing in time.
///
/// The current value is clamped *before* stepping, so a file that says `5`
/// moves in sensible increments up from the floor rather than from 5.
pub fn stepped_opacity(now: u32, dir: Nudge) -> u32 {
    let now = now.clamp(OPACITY_MIN, OPACITY_MAX);
    match dir {
        Nudge::Up => (now + OPACITY_STEP).min(OPACITY_MAX),
        Nudge::Down => now.saturating_sub(OPACITY_STEP).max(OPACITY_MIN),
    }
}

/// What the hint row says after a nudge.
///
/// It names the number, because the whole point is adjusting something by eye
/// — and it says when there is no further to go, so pressing again and seeing
/// nothing change reads as an edge rather than a dead key.
pub fn opacity_notice(n: u32) -> String {
    match n {
        OPACITY_MAX => "opacity 100% - solid".to_string(),
        OPACITY_MIN => format!("opacity {n}% - as faint as it goes"),
        _ => format!("opacity {n}%"),
    }
}

/// `$XDG_CONFIG_HOME/dropkick/config`, falling back to `~/.config`.
pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("dropkick").join("config"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("dropkick").join("config"))
}

/// The settings file plus enough state to notice it changing.
pub struct Settings {
    path: Option<PathBuf>,
    mtime: Option<SystemTime>,
    pub config: Config,
    /// Settings appended to the file because it predated them. Taken once, for
    /// the panel to mention.
    added: Vec<String>,
}

impl Settings {
    /// Load settings, writing a commented default file the first time so the
    /// options are discoverable by opening the file rather than reading a
    /// manual.
    pub fn load() -> Settings {
        let mut settings = Settings {
            path: config_path(),
            mtime: None,
            config: Config::default(),
            added: Vec::new(),
        };
        settings.read(true);
        settings
    }

    /// Settings that were just appended to the file, if any. Cleared by taking.
    pub fn take_added(&mut self) -> Vec<String> {
        std::mem::take(&mut self.added)
    }

    /// Bring a file written before some settings existed back up to date.
    ///
    /// The file's whole job is that opening it tells you what the options are.
    /// A config from before a setting shipped silently takes the default and
    /// gives no hint there was a choice, so the absent ones are appended with
    /// their comments.
    ///
    /// **Only ever appends.** Existing lines are untouched, so an edited value
    /// cannot be reverted, and nothing is rewritten or reordered. Written
    /// through the same atomic path as everything else, so an interrupted write
    /// cannot leave a half-file.
    fn top_up(&mut self, path: &std::path::Path, existing: &str) {
        let Some(addition) = Config::append_for(existing) else { return };
        let added = Config::missing_from(existing);
        let combined = format!("{existing}{addition}");

        // Refuse to write anything that would not parse. This edits a file the
        // user owns; the bar for that is higher than "it compiled".
        let (_, unknown) = Config::parse(&combined);
        if !unknown.is_empty() {
            eprintln!("dropkickd: not topping up {}: generated text does not parse", path.display());
            return;
        }

        match crate::storage::write_atomic(path, &combined) {
            Ok(()) => {
                eprintln!(
                    "dropkickd: added {} new setting(s) to {}: {}",
                    added.len(),
                    path.display(),
                    added.join(", ")
                );
                self.added = added;
                // Our own write moved the mtime. Record it, or the next summon
                // sees a changed file and re-reads for nothing.
                self.mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            }
            Err(e) => eprintln!("dropkickd: could not top up {}: {e}", path.display()),
        }
    }

    /// Re-read if the file changed since last time. Returns true if the
    /// settings are now different.
    ///
    /// Called on every summon rather than watched, because the daemon can live
    /// for days and a setting you edited but that does not apply until you
    /// restart something is a setting that looks broken. A `stat` costs a few
    /// microseconds against a summon budget measured in milliseconds, and the
    /// file is only re-read when its mtime actually moves.
    pub fn reload_if_changed(&mut self) -> bool {
        let Some(path) = self.path.clone() else { return false };
        let current = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if current == self.mtime {
            return false;
        }
        let before = self.config.clone();
        self.read(false);
        before != self.config
    }

    fn read(&mut self, create_if_missing: bool) {
        let Some(path) = self.path.clone() else { return };
        self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let (config, unknown) = Config::parse(&text);
                for u in unknown {
                    eprintln!(
                        "dropkickd: {}:{}: unrecognised setting {:?}",
                        path.display(),
                        u.line,
                        u.key
                    );
                }
                self.config = config;
                self.top_up(&path, &text);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.config = Config::default();
                if create_if_missing {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(&path, Config::default_file());
                    self.mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                }
            }
            Err(e) => eprintln!("dropkickd: cannot read {}: {e}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam `--shot` cannot reach: shot mode exits before settings are ever
    /// loaded, so driving the binary proves nothing about this path.
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let d = base.join(format!("dropkick-settings-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("config")
    }

    fn settings_at(path: PathBuf) -> Settings {
        Settings { path: Some(path), mtime: None, config: Config::default(), added: Vec::new() }
    }

    const OLD: &str = "# Dropkick settings. Plain key = value; '#' starts a comment.

# Post a desktop notification when a countdown finishes.
timer.notify = true

timer.show_panel = true
";

    #[test]
    fn an_old_file_gains_the_settings_it_predates() {
        let path = scratch("top-up");
        std::fs::write(&path, OLD).unwrap();

        let mut s = settings_at(path.clone());
        s.read(false);

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.starts_with(OLD), "the original text must be left alone");
        assert!(after.contains("panel.opacity"), "did not gain panel.opacity");
        assert!(after.contains("generate.symbols"), "did not gain generate.symbols");
        assert!(!s.take_added().is_empty(), "nothing was reported as added");

        // The user's non-default choice survives.
        assert!(s.config.timer_show_panel);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn topping_up_happens_once_and_does_not_keep_rewriting() {
        let path = scratch("idempotent");
        std::fs::write(&path, OLD).unwrap();

        let mut s = settings_at(path.clone());
        s.read(false);
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(!s.take_added().is_empty());

        let mut t = settings_at(path.clone());
        t.read(false);
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, second, "a second load rewrote the file");
        assert!(t.take_added().is_empty(), "reported additions on an up-to-date file");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_complete_file_is_left_exactly_as_it_was() {
        let path = scratch("complete");
        std::fs::write(&path, Config::default_file()).unwrap();
        let mut s = settings_at(path.clone());
        s.read(false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), Config::default_file());
        assert!(s.take_added().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_topped_up_file_is_still_owner_only() {
        // It goes out through storage::write_atomic, so it must land at 0600
        // even though the file it replaces may have been looser.
        use std::os::unix::fs::PermissionsExt;
        let path = scratch("mode");
        std::fs::write(&path, OLD).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut s = settings_at(path.clone());
        s.read(false);

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got {mode:o}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn opacity_steps_by_five_each_way() {
        assert_eq!(stepped_opacity(60, Nudge::Up), 65);
        assert_eq!(stepped_opacity(60, Nudge::Down), 55);
    }

    /// The one that matters. A panel faded to nothing still holds the
    /// keyboard, so the only way out would be the summon key — and key repeat
    /// on a held `Alt+-` gets there in well under a second.
    #[test]
    fn holding_the_key_down_can_never_make_the_panel_invisible() {
        let mut n = OPACITY_MAX;
        for _ in 0..500 {
            n = stepped_opacity(n, Nudge::Down);
            assert!(n >= OPACITY_MIN, "fell through the floor to {n}");
        }
        assert_eq!(n, OPACITY_MIN);
    }

    #[test]
    fn it_stops_at_solid_rather_than_climbing_past_it() {
        let mut n = OPACITY_MIN;
        for _ in 0..500 {
            n = stepped_opacity(n, Nudge::Up);
            assert!(n <= OPACITY_MAX, "went past solid to {n}");
        }
        assert_eq!(n, OPACITY_MAX);
    }

    #[test]
    fn a_value_near_the_floor_lands_on_it_rather_than_under_it() {
        // 22 - 5 is 17, which is below the floor. It has to clamp, not wrap
        // and not refuse.
        assert_eq!(stepped_opacity(OPACITY_MIN + 2, Nudge::Down), OPACITY_MIN);
        assert_eq!(stepped_opacity(OPACITY_MAX - 2, Nudge::Up), OPACITY_MAX);
    }

    #[test]
    fn a_nonsense_value_in_the_file_is_dragged_into_range_first() {
        // The file is hand-edited, so it can say anything. Stepping from 3
        // must not produce 8.
        assert_eq!(stepped_opacity(3, Nudge::Up), OPACITY_MIN + OPACITY_STEP);
        assert_eq!(stepped_opacity(0, Nudge::Down), OPACITY_MIN);
        assert_eq!(stepped_opacity(9999, Nudge::Up), OPACITY_MAX);
    }

    #[test]
    fn the_notice_names_the_number_and_says_when_there_is_no_further_to_go() {
        assert!(opacity_notice(65).contains("65%"));
        assert!(opacity_notice(OPACITY_MAX).contains("solid"));
        assert!(opacity_notice(OPACITY_MIN).contains("faint"));
    }

    /// Changing the opacity must never touch the file. The settings file is
    /// hand-edited and `top_up` promises never to rewrite a line in it; a
    /// session-only nudge is what keeps that promise true.
    #[test]
    fn nudging_opacity_writes_nothing_to_disk() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/settings.rs"))
            .expect("read own source");
        let after = src.split("pub fn stepped_opacity").nth(1).expect("the function");
        let body = after.split("\n}").next().unwrap();
        assert!(
            !body.contains("write") && !body.contains("save"),
            "stepped_opacity must stay pure; it grew a write"
        );
    }
}
