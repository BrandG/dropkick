//! Reading the local UTC offset out of `/etc/localtime`.
//!
//! `std::time` gives seconds since the epoch in UTC and nothing else, and the
//! difference is not cosmetic: on this machine, at the moment this was written,
//! it was 20:04 on Friday locally and 00:04 on Saturday in UTC. A calendar
//! built on UTC would have been showing the wrong day.
//!
//! The rules are already on the disk, in the TZif file the C library reads.
//! Rather than take a dependency or reach for `libc` and an `unsafe` block,
//! this reads the same file. That matches how the D-Bus client is handled: a
//! few hundred lines in the repository beats a crate nobody audits.
//!
//! Format: RFC 8536. A v1 header and 32-bit data block, then — for version 2
//! and up — the whole thing again with 64-bit transition times. The 64-bit
//! block is the one to use; the 32-bit one cannot represent dates past 2038 and
//! exists only so that ancient readers do not choke.

use dropkick_core::date::Date;
use std::time::{SystemTime, UNIX_EPOCH};

const PATH: &str = "/etc/localtime";

struct Counts {
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

fn be32(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?))
}

fn be64(b: &[u8]) -> Option<i64> {
    Some(i64::from_be_bytes(b.get(..8)?.try_into().ok()?))
}

/// Parse the six counts that follow the 20-byte preamble.
fn counts(b: &[u8], at: usize) -> Option<Counts> {
    let f = |i: usize| be32(b.get(at + i * 4..)?).map(|v| v as usize);
    Some(Counts {
        isutcnt: f(0)?,
        isstdcnt: f(1)?,
        leapcnt: f(2)?,
        timecnt: f(3)?,
        typecnt: f(4)?,
        charcnt: f(5)?,
    })
}

/// Size of a data block, given how wide its transition times are.
fn block_len(c: &Counts, time_width: usize) -> usize {
    c.timecnt * time_width
        + c.timecnt
        + c.typecnt * 6
        + c.charcnt
        + c.leapcnt * (time_width + 4)
        + c.isstdcnt
        + c.isutcnt
}

/// The UTC offset in seconds that applies at `now`, from TZif bytes.
pub fn offset_at(bytes: &[u8], now: i64) -> Option<i32> {
    if bytes.get(..4)? != b"TZif" {
        return None;
    }
    let version = *bytes.get(4)?;

    let head = counts(bytes, 20)?;
    // Version 1 files have only the 32-bit block. Anything later repeats the
    // header and data with 64-bit times, and that is the copy worth reading.
    let (start, c, width) = if version == b'2' || version == b'3' || version == b'4' {
        let second_header = 20 + 24 + block_len(&head, 4);
        if bytes.get(second_header..second_header + 4)? != b"TZif" {
            return None;
        }
        let c2 = counts(bytes, second_header + 20)?;
        (second_header + 44, c2, 8)
    } else {
        (44, head, 4)
    };

    let times = bytes.get(start..start + c.timecnt * width)?;
    let idx_at = start + c.timecnt * width;
    let indices = bytes.get(idx_at..idx_at + c.timecnt)?;
    let types_at = idx_at + c.timecnt;

    // Each ttinfo is a 4-byte offset, a DST flag, and an index into the
    // abbreviation strings. Only the offset matters here.
    let ttinfo = |i: usize| -> Option<(i32, bool)> {
        let at = types_at + i * 6;
        let off = be32(bytes.get(at..)?)? as i32;
        Some((off, *bytes.get(at + 4)? == 1))
    };

    // The applicable type is the one named by the last transition at or before
    // `now`. Transitions are sorted, so walking backwards finds it first.
    for i in (0..c.timecnt).rev() {
        let t = if width == 8 {
            be64(times.get(i * 8..)?)?
        } else {
            be32(times.get(i * 4..)?)? as i32 as i64
        };
        if t <= now {
            let ty = *indices.get(i)? as usize;
            return ttinfo(ty).map(|(off, _)| off);
        }
    }

    // Before the first recorded transition, or a zone that has never changed:
    // the spec says prefer the first non-DST type.
    for i in 0..c.typecnt {
        if let Some((off, is_dst)) = ttinfo(i) {
            if !is_dst {
                return Some(off);
            }
        }
    }
    ttinfo(0).map(|(off, _)| off)
}

/// Seconds since the Unix epoch, UTC.
pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Today's date in the local timezone.
///
/// Falls back to UTC if `/etc/localtime` is missing or unreadable. That is the
/// least-wrong fallback available — it is off by at most a day, near midnight,
/// rather than failing to show a calendar at all — and it is reported so the
/// panel can say the date may be off rather than quietly being wrong.
pub fn today_local() -> (Date, bool) {
    let now = now_unix();
    let offset = std::fs::read(PATH).ok().and_then(|b| offset_at(&b, now));
    let local = now + offset.unwrap_or(0) as i64;
    (Date::from_days(local.div_euclid(86_400)), offset.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a TZif v2 file with two types and one transition, so the parser is
    /// tested against bytes this test controls rather than whatever the machine
    /// happens to have installed.
    fn synthetic(transition: i64, before: i32, after: i32) -> Vec<u8> {
        fn header(timecnt: u32, typecnt: u32, charcnt: u32) -> Vec<u8> {
            let mut h = Vec::new();
            h.extend_from_slice(b"TZif");
            h.push(b'2');
            h.extend_from_slice(&[0u8; 15]);
            for v in [0u32, 0, 0, timecnt, typecnt, charcnt] {
                h.extend_from_slice(&v.to_be_bytes());
            }
            h
        }
        fn types(a: i32, b: i32) -> Vec<u8> {
            let mut t = Vec::new();
            for (off, dst) in [(a, 0u8), (b, 1u8)] {
                t.extend_from_slice(&off.to_be_bytes());
                t.push(dst);
                t.push(0);
            }
            t
        }

        let mut v = Vec::new();
        // v1 block: empty of transitions, but the types still have to be there
        // because the second header sits immediately after it.
        v.extend_from_slice(&header(0, 2, 1));
        v.extend_from_slice(&types(before, after));
        v.push(0); // one designation byte

        // v2 block: the real data, with a 64-bit transition.
        v.extend_from_slice(&header(1, 2, 1));
        v.extend_from_slice(&transition.to_be_bytes());
        v.push(1); // that transition selects type 1
        v.extend_from_slice(&types(before, after));
        v.push(0);
        v
    }

    const EST: i32 = -5 * 3600;
    const EDT: i32 = -4 * 3600;

    #[test]
    fn reads_the_offset_before_and_after_a_transition() {
        let t = 1_772_000_000; // some instant
        let f = synthetic(t, EST, EDT);
        assert_eq!(offset_at(&f, t - 1), Some(EST), "before the transition");
        assert_eq!(offset_at(&f, t), Some(EDT), "at the transition");
        assert_eq!(offset_at(&f, t + 1), Some(EDT), "after it");
    }

    #[test]
    fn falls_back_to_the_first_standard_type_before_any_transition() {
        let f = synthetic(1_772_000_000, EST, EDT);
        assert_eq!(offset_at(&f, 0), Some(EST), "not the DST type");
    }

    #[test]
    fn rejects_files_that_are_not_tzif() {
        assert_eq!(offset_at(b"not a tzif file at all", 0), None);
        assert_eq!(offset_at(b"", 0), None);
    }

    #[test]
    fn truncated_files_return_none_rather_than_panicking() {
        // A short read must not index past the end. Every prefix is tried.
        let f = synthetic(1_772_000_000, EST, EDT);
        for n in 0..f.len() {
            let _ = offset_at(&f[..n], 1_772_000_001);
        }
    }

    #[test]
    fn the_offset_converts_utc_to_the_right_local_day() {
        // The case that motivated all of this: 00:04 UTC on Saturday the 22nd
        // is 20:04 on Friday the 21st in New York.
        let utc = Date::new(2026, 8, 22).to_days() * 86_400 + 4 * 60;
        let local = utc + EDT as i64;
        assert_eq!(Date::from_days(utc.div_euclid(86_400)), Date::new(2026, 8, 22));
        assert_eq!(Date::from_days(local.div_euclid(86_400)), Date::new(2026, 8, 21));
    }

    #[test]
    fn the_real_system_file_parses_if_it_exists() {
        // A smoke test, not an assertion about this machine's timezone.
        if let Ok(bytes) = std::fs::read(PATH) {
            let off = offset_at(&bytes, now_unix());
            let off = off.expect("/etc/localtime should parse");
            assert!(
                (-12 * 3600..=14 * 3600).contains(&off),
                "offset {off} is outside every real timezone"
            );
            assert_eq!(off % 900, 0, "real offsets are whole quarter-hours");
        }
    }

    #[test]
    fn today_is_plausible() {
        let (d, from_file) = today_local();
        assert!(d.is_valid());
        assert!(d.year >= 2020 && d.year < 2100, "got {d:?}");
        let _ = from_file;
    }
}
