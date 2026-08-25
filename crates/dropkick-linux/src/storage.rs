//! Files Dropkick keeps, and how they are written.
//!
//! Two rules, both decided by the user on 2026-08-15 and recorded in
//! `docs/PROJECT_LOG.md`:
//!
//! **Where.** `$XDG_DATA_HOME/dropkick/`, which is `~/.local/share/dropkick/`
//! when that variable is unset. The directory is `0700` and the files are
//! `0600`, so no other account on the machine can read them or even list what
//! is in there. `/tmp` was considered and rejected: it is world-listable, so
//! filenames leak even when contents do not.
//!
//! **How.** Never write over a live file. Write a temporary file alongside it
//! and rename that into place; on Linux a rename either happens completely or
//! not at all, so the real file is always either the whole old version or the
//! whole new one. A crash halfway through a save cannot leave a half-file
//! behind, which is the difference between losing two seconds of work and
//! losing the lot.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

/// `$XDG_DATA_HOME/dropkick`, or `~/.local/share/dropkick`.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("dropkick"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/share/dropkick"))
}

pub fn calendar_path() -> Option<PathBuf> {
    Some(data_dir()?.join("calendar.txt"))
}

pub fn scratchpad_path() -> Option<PathBuf> {
    Some(data_dir()?.join("scratchpad.txt"))
}

/// Create a directory if needed and make sure it is `0700`.
///
/// The mode is applied whether or not this call created it: a directory left
/// group- or world-readable by an earlier version, or by the default umask,
/// should be tightened rather than left as found.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    let mut perms = fs::metadata(dir)?.permissions();
    if perms.mode() & 0o777 != DIR_MODE {
        perms.set_mode(DIR_MODE);
        fs::set_permissions(dir, perms)?;
    }
    Ok(())
}

/// Write `contents` to `path` so that a crash cannot corrupt it.
///
/// The temporary file is created `0600` from the start rather than tightened
/// afterwards — creating it with the default mode and fixing it later leaves a
/// window in which another user could open it.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    ensure_dir(dir)?;

    // Alongside the target, not in /tmp: rename only works within one
    // filesystem, and a cross-device rename would fail or silently become a
    // copy that is no longer atomic.
    let tmp = path.with_extension("tmp");
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(&tmp)?;
        f.write_all(contents.as_bytes())?;
        // Get the bytes to the disk before the rename. Without this the rename
        // can land while the contents are still only in the page cache, which
        // on a power cut leaves an intact filename over an empty file.
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;

    // Make the rename itself durable. Best effort: some filesystems refuse to
    // sync a directory, and failing the whole save over that would be worse
    // than the small risk it protects against.
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

pub fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Tighten a directory and the files directly in it to `0700`/`0600`.
///
/// Used on the config directory, which predates this module and was being
/// created with whatever the umask gave — `0775`/`0664` on this machine, so
/// world-readable. Nothing secret lives there today, but the project claims its
/// own files are private and that claim should be true everywhere.
pub fn tighten(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    ensure_dir(dir)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let mut perms = entry.metadata()?.permissions();
        if perms.mode() & 0o777 != FILE_MODE {
            perms.set_mode(FILE_MODE);
            fs::set_permissions(entry.path(), perms)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory under the runtime dir, or `/tmp` if there is none.
    /// Tests must not touch the user's real data.
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let d = base.join(format!("dropkick-test-{name}"));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_new_directory_is_private() {
        let d = scratch("dir-mode");
        ensure_dir(&d).unwrap();
        let mode = fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "got {mode:o}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_existing_loose_directory_is_tightened() {
        let d = scratch("loose-dir");
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir(&d).unwrap();
        assert_eq!(fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn written_files_are_owner_only() {
        let d = scratch("file-mode");
        let f = d.join("calendar.txt");
        write_atomic(&f, "hello\n").unwrap();
        let mode = fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got {mode:o}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn writing_round_trips_and_replaces_previous_contents() {
        let d = scratch("round-trip");
        let f = d.join("calendar.txt");
        write_atomic(&f, "first\n").unwrap();
        assert_eq!(read(&f).as_deref(), Some("first\n"));
        write_atomic(&f, "second\n").unwrap();
        assert_eq!(read(&f).as_deref(), Some("second\n"), "no leftovers from the first write");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let d = scratch("no-temp");
        let f = d.join("calendar.txt");
        write_atomic(&f, "x\n").unwrap();
        let leftovers: Vec<String> = fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?}");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_temp_file_lives_beside_the_target_so_rename_stays_atomic() {
        let d = scratch("same-dir");
        let f = d.join("calendar.txt");
        assert_eq!(f.with_extension("tmp").parent(), f.parent());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reading_a_missing_file_is_none_not_an_error() {
        assert_eq!(read(Path::new("/nonexistent/dropkick/calendar.txt")), None);
    }

    #[test]
    fn tighten_fixes_loose_files_in_place() {
        let d = scratch("tighten");
        fs::create_dir_all(&d).unwrap();
        let f = d.join("config");
        fs::write(&f, "timer.notify = true\n").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o664)).unwrap();
        tighten(&d).unwrap();
        assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::read_to_string(&f).unwrap(), "timer.notify = true\n", "contents intact");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_calendar_survives_a_real_write_and_read() {
        // The seam the unit tests on either side cannot cover: core serialises,
        // this module writes and reads, core parses it back.
        use dropkick_core::calendar::Calendar;
        use dropkick_core::date::Date;

        let d = scratch("calendar-seam");
        let f = d.join("calendar.txt");

        let mut c = Calendar::new();
        c.add(Date::new(2026, 8, 21), Some(15 * 60), "Dentist");
        c.add(Date::new(2026, 8, 21), None, "Bin day");
        c.add(Date::new(2026, 12, 25), Some(9 * 60 + 30), "Trains: check times");
        write_atomic(&f, &c.serialise()).unwrap();

        let mut back = Calendar::new();
        assert_eq!(back.load(&read(&f).unwrap()), 0, "nothing should fail to parse");
        assert_eq!(back.entries(), c.entries());

        // And it is readable by a human with `cat`, which is a stated rule.
        let raw = read(&f).unwrap();
        assert!(raw.contains("2026-08-21 15:00 Dentist"));
        assert!(raw.contains("2026-08-21 --:-- Bin day"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn data_dir_honours_xdg_then_falls_back_to_home() {
        // Reads the real environment rather than mutating it: setting env vars
        // in a test races every other test in the process.
        let d = data_dir().expect("HOME or XDG_DATA_HOME should be set");
        assert!(d.ends_with("dropkick"), "got {d:?}");
        assert!(d.is_absolute());
    }
}
