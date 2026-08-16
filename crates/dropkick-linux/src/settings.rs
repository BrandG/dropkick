//! Finding, loading and reloading the settings file.
//!
//! The parsing lives in `dropkick-core` where it can be tested; this is only
//! the part that needs a filesystem.

use std::path::PathBuf;
use std::time::SystemTime;

use dropkick_core::config::Config;

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
}

impl Settings {
    /// Load settings, writing a commented default file the first time so the
    /// options are discoverable by opening the file rather than reading a
    /// manual.
    pub fn load() -> Settings {
        let mut settings = Settings { path: config_path(), mtime: None, config: Config::default() };
        settings.read(true);
        settings
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
