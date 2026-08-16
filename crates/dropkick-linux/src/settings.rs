//! Finding and loading the settings file.
//!
//! The parsing lives in `dropkick-core` where it can be tested; this is only
//! the part that needs a filesystem.

use std::path::PathBuf;

use dropkick_core::config::Config;

/// `$XDG_CONFIG_HOME/dropkick/config`, falling back to `~/.config`.
pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("dropkick").join("config"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("dropkick").join("config"))
}

/// Load settings, writing a commented default file the first time so the
/// options are discoverable by opening the file rather than reading a manual.
pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };

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
            config
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, Config::default_file());
            Config::default()
        }
        Err(e) => {
            eprintln!("dropkickd: cannot read {}: {e}", path.display());
            Config::default()
        }
    }
}
