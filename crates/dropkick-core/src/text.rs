//! Text layout helpers shared by every platform's renderer.
//!
//! Wrapping is pure arithmetic over character counts, so it lives here rather
//! than in the Wayland crate: the Windows renderer will need exactly the same
//! answers, and here it can be tested without a compositor or a font.
//!
//! Everything is in *characters*, not pixels. The renderer converts its content
//! width into a column count and asks for segments back.

/// Split one logical line into display segments of at most `cols` characters,
/// breaking at spaces where possible. Returns half-open `[start, end)` ranges
/// of character indices. Always returns at least one segment, so an empty line
/// still occupies a row.
pub fn wrap_line(line: &str, cols: usize) -> Vec<(usize, usize)> {
    let len = line.chars().count();
    if cols == 0 || len <= cols {
        return vec![(0, len)];
    }

    let chars: Vec<char> = line.chars().collect();
    let mut segments = Vec::new();
    let mut start = 0;

    while start < len {
        let hard_end = (start + cols).min(len);
        if hard_end == len {
            segments.push((start, len));
            break;
        }
        // Prefer the last space that fits. Scanning back from just past the
        // limit lets a break land on a space sitting exactly at the boundary.
        let break_at = (start..=hard_end).rev().find(|&i| i < len && chars[i] == ' ');
        match break_at {
            Some(space) if space > start => {
                segments.push((start, space));
                // Swallow the run of spaces at the break so a wrapped line does
                // not begin with a stray indent.
                let mut next = space;
                while next < len && chars[next] == ' ' {
                    next += 1;
                }
                start = next;
            }
            // A word longer than the whole line: break it mid-word rather than
            // running off the panel.
            _ => {
                segments.push((start, hard_end));
                start = hard_end;
            }
        }
    }

    if segments.is_empty() {
        segments.push((0, 0));
    }
    segments
}

/// Locate the cursor within wrapped segments: which display row, and how many
/// characters into it. A cursor sitting at end-of-line lands after the last
/// character of the final segment.
pub fn cursor_cell(segments: &[(usize, usize)], cx: usize) -> (usize, usize) {
    for (row, &(start, end)) in segments.iter().enumerate() {
        if cx >= start && cx < end {
            return (row, cx - start);
        }
        // End of a segment that is also the end of the line.
        if cx == end && row == segments.len() - 1 {
            return (row, cx - start);
        }
    }
    match segments.last() {
        Some(&(start, end)) => (segments.len() - 1, end.saturating_sub(start)),
        None => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg_text(line: &str, segs: &[(usize, usize)]) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        segs.iter().map(|&(s, e)| chars[s..e].iter().collect()).collect()
    }

    #[test]
    fn short_lines_are_one_segment() {
        assert_eq!(wrap_line("hello", 20), vec![(0, 5)]);
        assert_eq!(wrap_line("", 20), vec![(0, 0)]);
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let line = "the quick brown fox jumps";
        let segs = wrap_line(line, 10);
        let parts = seg_text(line, &segs);
        assert!(parts.iter().all(|p| p.chars().count() <= 10), "{parts:?}");
        assert!(!parts.iter().any(|p| p.starts_with(' ')), "{parts:?}");
        assert_eq!(parts.join(" "), line);
    }

    #[test]
    fn a_word_longer_than_the_line_is_broken() {
        let line = "supercalifragilistic";
        let segs = wrap_line(line, 8);
        assert_eq!(seg_text(line, &segs), vec!["supercal", "ifragili", "stic"]);
    }

    #[test]
    fn no_segment_exceeds_the_column_count() {
        let line = "aaa bbbbbbbbbbbbbbbb cc ddddd e";
        for cols in 1..25 {
            for (s, e) in wrap_line(line, cols) {
                assert!(e - s <= cols, "cols={cols} segment {s}..{e}");
            }
        }
    }

    #[test]
    fn every_character_survives_wrapping() {
        // Spaces at break points are swallowed, and a word too long for the
        // line is broken mid-word, so neither "join with spaces" nor "compare
        // word lists" is a valid invariant. What must hold is that no
        // non-space character is lost, duplicated, or reordered.
        for line in [
            "call Frank re: the staging server before the standup",
            "aaa bbbbbbbbbbbbbbbb cc ddddd e",
            "supercalifragilisticexpialidocious",
        ] {
            let expected: String = line.chars().filter(|c| *c != ' ').collect();
            for cols in 1..40 {
                let segs = wrap_line(line, cols);
                let rebuilt: String = seg_text(line, &segs)
                    .join("")
                    .chars()
                    .filter(|c| *c != ' ')
                    .collect();
                assert_eq!(rebuilt, expected, "line={line:?} cols={cols}");
            }
        }
    }

    #[test]
    fn cursor_maps_onto_the_right_row() {
        let line = "the quick brown fox";
        let segs = wrap_line(line, 10);
        assert_eq!(cursor_cell(&segs, 0), (0, 0));
        let (row, _) = cursor_cell(&segs, line.chars().count());
        assert_eq!(row, segs.len() - 1);
    }

    #[test]
    fn cursor_at_end_of_line_is_placed_not_lost() {
        let segs = wrap_line("hello", 20);
        assert_eq!(cursor_cell(&segs, 5), (0, 5));
    }

    #[test]
    fn cursor_never_panics_anywhere_in_a_wrapped_line() {
        let line = "aaa bbbbbbbbbbbbbbbb cc ddddd e";
        for cols in 1..25 {
            let segs = wrap_line(line, cols);
            for cx in 0..=line.chars().count() {
                let (row, _) = cursor_cell(&segs, cx);
                assert!(row < segs.len(), "cols={cols} cx={cx}");
            }
        }
    }
}
