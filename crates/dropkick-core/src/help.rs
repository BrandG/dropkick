//! What `Alt+/` puts on screen.
//!
//! This file is a claim about what the other files do, which is the dangerous
//! kind of documentation: nothing breaks when it goes stale. The hint row has
//! already been wrong twice — it offered `Tab` for the eyedropper long after
//! `Tab` started refusing — and a whole page of it is a bigger surface for the
//! same mistake.
//!
//! So every row carries a [`Row::probe`]: the plain characters it claims work,
//! which a test presses and requires the tool to consume. A row that documents
//! a key nobody handles fails the build. Where a claim cannot be pressed — a
//! chord, a two-key sequence, free text — the probe is empty and says so,
//! rather than being quietly omitted.

use crate::app::ToolId;

pub struct Row {
    /// The keys, written the way the panel shows them.
    pub keys: &'static str,
    pub what: &'static str,
    /// Plain characters this row claims work in the tool's resting state.
    ///
    /// Empty means the claim is not mechanically checkable: a chord, a
    /// sequence like `gg`, a key that only exists inside a mode the tool does
    /// not rest in, or a tool whose input is free text and consumes
    /// everything. Empty is a statement, not an omission.
    pub probe: &'static [char],
}

const fn row(keys: &'static str, what: &'static str, probe: &'static [char]) -> Row {
    Row { keys, what, probe }
}

/// How many rows of a tool's table are on screen at once.
///
/// Declared here rather than in the renderer, and this is the direction the
/// dependency has to run: `dropkick-core` owns the scroll position and cannot
/// see a panel, so it states the contract and a render test proves the panel
/// honours it. If the panel ever gets taller, raise this and that test will
/// confirm it rather than the two drifting apart.
pub const VISIBLE_ROWS: usize = 13;

/// Where a scrolled column starts, clamped so it can never strand the list
/// past its end. Everything shorter than a screen pins to zero.
pub fn clamp_scroll(scroll: usize, len: usize) -> usize {
    scroll.min(len.saturating_sub(VISIBLE_ROWS))
}

/// The keys that work in every tool and every mode.
///
/// `Super+D` is the one line here that Dropkick does not implement: it is a
/// COSMIC shortcut pointed at `dropkickctl`. It is listed because from the
/// user's side it is the most important key there is, and a help page that
/// omitted how to open the thing would be a strange help page.
pub const GLOBAL: &[Row] = &[
    row("super+d", "summon, or dismiss", &[]),
    row("alt+q .. u", "jump straight to a tool", &[]),
    row("alt+h  alt+l", "previous, next tool", &[]),
    row("esc", "up one level, or out", &[]),
    row("esc esc", "dismiss from anywhere", &[]),
    row("ctrl+c", "the same as esc", &[]),
    row("alt+-  alt+=", "fainter, more solid", &[]),
    row("alt+/", "this page", &[]),
];

/// The Scratchpad rests in command mode, so that is what these describe.
/// Insert mode is `esc` away from all of it and takes every printable key.
const SCRATCHPAD: &[Row] = &[
    row("i  a  A", "insert here, after, at end", &['i', 'a', 'A']),
    row("o  O", "open a line below, above", &['o', 'O']),
    row("h j k l", "move (arrows work too)", &['h', 'j', 'k', 'l']),
    row("w  b", "word forward, word back", &['w', 'b']),
    row("e", "end of the word", &['e']),
    row("0  $", "start, end of the line", &['0', '$']),
    row("gg  G", "first line, last line", &['G']),
    row("v  V", "select, select whole lines", &['v', 'V']),
    row("x", "delete a character", &['x']),
    row("dd", "delete a line", &['d']),
    row("d then a move", "delete that far", &[]),
    row("yy", "copy a line", &[]),
    row("Y", "copy the whole buffer", &['Y']),
    row("p  P", "paste after, before", &['p', 'P']),
    row("ctrl+v", "paste, in any mode", &[]),
    row("u", "undo", &['u']),
];

/// Everything typed is part of the sum, so there are no command keys to list.
/// These are the things it can be asked, which is what someone opening a help
/// page for a calculator actually wants.
const CALCULATOR: &[Row] = &[
    row("enter", "work it out", &[]),
    row("ans", "reuse the last answer", &[]),
    row("2 + 2 * 3", "the ordinary kind", &[]),
    row("25 ft in m", "convert units", &[]),
    row("today + 90 days", "date maths", &[]),
    row("2026-03-01 - 2026-01-31", "days between two dates", &[]),
];

/// The list, once revealed. Arriving at the tool it is hidden, and the first
/// key you press reveals it rather than doing what it says.
const CLIPBOARD: &[Row] = &[
    row("j  k", "move (arrows, home, end too)", &['j', 'k']),
    row("enter  y", "paste it, and vanish", &['y']),
    row("/", "search", &['/']),
    row("p", "pin, so it survives a clear", &['p']),
    row("d", "delete this one", &['d']),
    row("x", "clear everything unpinned", &['x']),
    row("c", "pause or resume capture", &['c']),
    row("m", "hide the list again", &['m']),
];

const TIMER: &[Row] = &[
    row("25m  90s  1h30m", "type a length, then enter", &[]),
    row("space", "start, or pause", &[' ']),
    row("r", "reset", &['r']),
    row("w", "stopwatch instead", &['w']),
    row("", "it keeps running while hidden", &[]),
];

const GENERATE: &[Row] = &[
    row("p  u", "password, or uuid", &['p', 'u']),
    row("r  space", "make another", &['r', ' ']),
    row("y  enter", "copy it", &['y']),
    row("a  A  0  !", "which characters to use", &['a', 'A', '0', '!']),
    row("x", "drop lookalikes: l 1 I O 0", &['x']),
    row("+  -", "longer, shorter (arrows too)", &['+', '-']),
    row("", "copies skip clipboard history", &[]),
];

/// The month grid, which is where the Calendar rests.
const CALENDAR: &[Row] = &[
    row("h j k l", "move a day (arrows too)", &['h', 'j', 'k', 'l']),
    row("[  ]", "previous, next month", &['[', ']']),
    row("t", "back to today", &['t']),
    row("a", "add an entry here", &['a']),
    row("tab", "the entries on this day", &[]),
    row("j k  then d", "in that list: pick, delete", &[]),
    row("", "adding asks the text, then a time", &[]),
    row("", "blank time means all day", &[]),
];

/// With the eyedropper switched off, which is how it ships. See
/// `ColorTool::new` for why.
const COLOR: &[Row] = &[
    row("#6FB3F2", "hex, long or short", &[]),
    row("rgb(111, 179, 242)", "or rgb", &[]),
    row("hsl(210, 82%, 69%)", "or hsl", &[]),
    row("tomato", "or any CSS colour name", &[]),
    row("enter", "show it every other way", &[]),
    row("ctrl+y", "copy the hex", &[]),
];

/// The same, plus the key that only exists when the eyedropper is on.
const COLOR_EYEDROPPER: &[Row] = &[
    row("#6FB3F2", "hex, long or short", &[]),
    row("rgb(111, 179, 242)", "or rgb", &[]),
    row("hsl(210, 82%, 69%)", "or hsl", &[]),
    row("tomato", "or any CSS colour name", &[]),
    row("enter", "show it every other way", &[]),
    row("ctrl+y", "copy the hex", &[]),
    row("tab", "pick one off the screen", &[]),
];

/// `eyedropper` is passed in rather than read, so this stays a table and the
/// caller owns the one piece of state that changes what the page says.
pub fn rows(tool: ToolId, eyedropper: bool) -> &'static [Row] {
    match tool {
        ToolId::Scratchpad => SCRATCHPAD,
        ToolId::Calculator => CALCULATOR,
        ToolId::Clipboard => CLIPBOARD,
        ToolId::Timer => TIMER,
        ToolId::Generate => GENERATE,
        ToolId::Calendar => CALENDAR,
        ToolId::Color => {
            if eyedropper {
                COLOR_EYEDROPPER
            } else {
                COLOR
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Response};
    use crate::key::{Key, KeyPress};

    /// A tool sitting at whatever it rests at when you arrive, ready to be
    /// asked whether it really handles a key.
    ///
    /// A fresh `App` per probe, deliberately: `i` puts the Scratchpad in
    /// insert mode, and every later probe would then be swallowed as text and
    /// pass for the wrong reason.
    fn resting(tool: ToolId) -> App {
        let mut app = App::new();
        app.active = tool;
        if tool == ToolId::Generate {
            // The pool starts dry -- the platform fills it -- and `y` copies
            // nothing when nothing has been generated. That is the tool being
            // right, not the help being wrong, so the probe gets a real tool
            // to press keys on.
            app.generate.supply_entropy(&vec![0x5A; 1024]);
            app.generate.regenerate();
        }
        if tool == ToolId::Clipboard {
            // Two things it needs: something in the list, since keys that act
            // on a selection have nothing to act on when it is empty, and a
            // reveal, since the first key at a masked list only unmasks.
            app.clipboard.record("something to select", &[]);
            app.on_key(KeyPress::plain(Key::Char('m')), 0);
            assert!(app.clipboard.revealed, "the list has to be revealed to be driven");
        }
        app
    }

    /// The point of the file. Every character the help page claims works must
    /// be one the tool actually consumes.
    #[test]
    fn every_key_the_help_page_claims_is_a_key_that_works() {
        for tool in [
            ToolId::Scratchpad,
            ToolId::Calculator,
            ToolId::Clipboard,
            ToolId::Timer,
            ToolId::Generate,
            ToolId::Calendar,
            ToolId::Color,
        ] {
            for r in rows(tool, false) {
                for &c in r.probe {
                    let mut app = resting(tool);
                    let got = app.on_key(KeyPress::plain(Key::Char(c)), 0);
                    assert_ne!(
                        got,
                        Response::Ignored,
                        "{tool:?} help says `{}` ({}), but {c:?} does nothing",
                        r.keys,
                        r.what
                    );
                }
            }
        }
    }

    /// The eyedropper row is the one that has to disappear, since `Tab`
    /// answers "this desktop refuses colour picking" while it is off.
    #[test]
    fn the_eyedropper_is_only_documented_when_it_is_on() {
        let off = rows(ToolId::Color, false);
        let on = rows(ToolId::Color, true);
        assert!(!off.iter().any(|r| r.keys.contains("tab")));
        assert!(on.iter().any(|r| r.keys.contains("tab")));
    }

    /// A row with neither keys nor a probe is a note, and a note with no text
    /// is a blank line nobody meant to leave in.
    #[test]
    fn no_row_is_empty() {
        for tool in [ToolId::Scratchpad, ToolId::Calculator, ToolId::Clipboard, ToolId::Timer] {
            for r in rows(tool, false) {
                assert!(!r.what.is_empty(), "a row in {tool:?} says nothing");
            }
        }
        for r in GLOBAL {
            assert!(!r.keys.is_empty() && !r.what.is_empty());
        }
    }
}
