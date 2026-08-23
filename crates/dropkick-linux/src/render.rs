//! Painting the panel: tab bar, tool content, hint row.
//!
//! The hint row is not a button bar. It is a reminder of the keys that already
//! work, which is what lets a newcomer learn the grammar without a manual and
//! lets everyone else ignore it.

use dropkick_core::app::{App, ToolId, TOOLS};
use dropkick_core::clipboard::Mode as ClipMode;
use dropkick_core::scratchpad::Mode;
use dropkick_core::timer::{format_duration, Mode as TimerMode};
use dropkick_core::text::{cursor_cell, wrap_line};

use crate::font::{Canvas, TextRenderer};

// Seven tabs at "q Calculator" width need ~870px; the rest is headroom for
// the three unspent slots.
pub const WIDTH: u32 = 940;
pub const HEIGHT: u32 = 460;

const TAB_H: i32 = 40;
const HINT_H: i32 = 30;
const PAD: f32 = 18.0;

const BG: u32 = 0x14_171C;
const TAB_BG: u32 = 0x0E_1115;
const TAB_ACTIVE: u32 = 0x1E_242E;
const ACCENT: u32 = 0x6F_B3F2;
const FG: u32 = 0xD8_DEE9;
const DIM: u32 = 0x69_727F;
const FAINT: u32 = 0x3A_4250;
const ERR: u32 = 0xE0_6C75;
const RULE: u32 = 0x23_2932;
// Selection sits behind unmodified FG text, so it has to be dark enough to read
// through. Picked off TAB_ACTIVE rather than ACCENT for that reason -- an accent
// wash bright enough to notice made the glyphs on top of it worse.
const SELECT: u32 = 0x2E_3A4B;
// The panel's outer edge. Bright enough to read against a dark window behind
// it, which is the case that prompted this: two dark surfaces butted together
// with no seam look like one window.
const BORDER: u32 = 0x5A_6B82;
const BORDER_W: i32 = 1;

/// `now_ms` is passed rather than kept on `App`: a clock stored as state is a
/// clock that can be stale, and the timer readout is the one thing here that
/// must never be.
pub fn draw(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    now_ms: u64,
    canvas: &mut Canvas,
) {
    canvas.fill(BG);
    draw_tabs(app, text, canvas);
    canvas.fill_rect(0, TAB_H, canvas.w, 1, RULE);

    let content_top = TAB_H + 1;
    let content_bottom = canvas.h - HINT_H;
    match app.active {
        ToolId::Calculator => draw_calculator(app, text, canvas, content_top, content_bottom),
        ToolId::Scratchpad => draw_scratchpad(app, text, canvas, content_top, content_bottom),
        ToolId::Clipboard => draw_clipboard(app, text, canvas, content_top, content_bottom),
        ToolId::Timer => draw_timer(app, text, big, now_ms, canvas, content_top, content_bottom),
        ToolId::Generate => draw_generate(app, text, big, canvas, content_top, content_bottom),
        ToolId::Calendar => draw_calendar(app, text, canvas, content_top, content_bottom),
        ToolId::Color => draw_color(app, text, big, canvas, content_top, content_bottom),
    }

    draw_hints(app, text, canvas);
    draw_border(canvas);
}

/// A hairline around the whole panel, drawn last so nothing paints over it.
///
/// Dropkick is a layer surface with no decoration, no title bar and no shadow,
/// so against a window of a similar colour its edge simply is not there. This
/// is the cheapest thing that says where it stops.
fn draw_border(canvas: &mut Canvas) {
    let (w, h, t) = (canvas.w, canvas.h, BORDER_W);
    canvas.fill_rect(0, 0, w, t, BORDER);
    canvas.fill_rect(0, h - t, w, t, BORDER);
    canvas.fill_rect(0, 0, t, h, BORDER);
    canvas.fill_rect(w - t, 0, t, h, BORDER);
}

fn draw_tabs(app: &App, text: &mut TextRenderer, canvas: &mut Canvas) {
    canvas.fill_rect(0, 0, canvas.w, TAB_H, TAB_BG);
    let baseline = 26.0;
    let adv = text.advance();
    let mut x = PAD;

    for tool in TOOLS.iter() {
        // "q Calculator" rather than "Calculator alt+q": the roster is seven
        // wide now, and the modifier is the same for every tab, so repeating it
        // seven times costs width without teaching anything the hint row and
        // help screen do not already say.
        let w = adv * 2.0 + text.width_of(tool.name);
        let active = app.active == tool.id;

        if active {
            canvas.fill_rect((x - 9.0) as i32, 4, (w + 18.0) as i32, TAB_H - 4, TAB_ACTIVE);
            // Accent underline: which tab owns the keyboard right now.
            canvas.fill_rect((x - 9.0) as i32, TAB_H - 3, (w + 18.0) as i32, 3, ACCENT);
        }

        // A running timer is visible from any tab: the point of a countdown is
        // that you stop thinking about it.
        let live = tool.id == ToolId::Timer && app.timer.is_running();

        let key_color = if !tool.built {
            FAINT
        } else if active {
            ACCENT
        } else {
            DIM
        };
        let name_color = if !tool.built {
            FAINT
        } else if active {
            FG
        } else if live {
            ACCENT
        } else {
            DIM
        };

        text.draw(canvas, x, baseline, &tool.key.to_string(), key_color);
        text.draw(canvas, x + adv * 2.0, baseline, tool.name, name_color);

        x += w + adv * 3.0;
    }
}

fn draw_calculator(
    app: &App,
    text: &mut TextRenderer,
    canvas: &mut Canvas,
    top: i32,
    bottom: i32,
) {
    let lh = text.line_height();
    // The error sits *below* the input, so the input has to move up to make
    // room for it. Without this the message is drawn past `bottom`, where the
    // hint row and the panel edge clip it: you get a couple of red pixels and
    // no idea what went wrong.
    let (input_baseline, err_baseline) =
        calculator_lines(bottom as f32, lh, app.calculator.error.is_some());

    // History grows upward from just above the input line, newest nearest.
    let mut y = input_baseline - lh * 1.8;
    for entry in app.calculator.history.iter().rev() {
        if y < top as f32 + lh {
            break;
        }
        text.draw(canvas, PAD, y, &entry.expr, DIM);
        let result = format!("= {}", entry.result);
        let rx = canvas.w as f32 - PAD - text.width_of(&result);
        text.draw(canvas, rx, y, &result, FG);
        y -= lh;
    }

    canvas.fill_rect(
        PAD as i32,
        (input_baseline - lh * 1.1) as i32,
        canvas.w - PAD as i32 * 2,
        1,
        RULE,
    );

    let prompt = "> ";
    let pen = text.draw(canvas, PAD, input_baseline, prompt, ACCENT);
    let pen = text.draw(canvas, pen, input_baseline, &app.calculator.input, FG);
    // Caret: a thin bar, because the calculator is always in text entry.
    canvas.fill_rect(
        pen as i32 + 1,
        (input_baseline - text.line_height() * 0.72) as i32,
        2,
        (text.line_height() * 0.85) as i32,
        ACCENT,
    );

    if let (Some(err), Some(y)) = (&app.calculator.error, err_baseline) {
        text.draw(canvas, PAD, y, err, ERR);
    }
}

fn draw_scratchpad(app: &App, text: &mut TextRenderer, canvas: &mut Canvas, top: i32, bottom: i32) {
    let lh = text.line_height();
    let adv = text.advance();
    let cols = (((canvas.w as f32 - PAD * 2.0) / adv).floor() as usize).max(1);

    // Flatten logical lines into wrapped display rows, noting where the cursor
    // lands. Wrapping is presentation only: the buffer keeps real lines, so a
    // wrapped note pasted elsewhere has no hard breaks in it.
    let mut rows: Vec<(usize, usize, usize)> = Vec::new();
    let mut cur_row = 0usize;
    let mut cur_col = 0usize;
    for (i, line) in app.scratchpad.lines.iter().enumerate() {
        let segments = wrap_line(line, cols);
        if i == app.scratchpad.cy {
            let (r, c) = cursor_cell(&segments, app.scratchpad.cx);
            cur_row = rows.len() + r;
            cur_col = c;
        }
        for (s, e) in segments {
            rows.push((i, s, e));
        }
    }

    let selection = app.scratchpad.selection();
    let visible = (((bottom - top) as f32 / lh).floor() as usize).max(1);
    // Scroll only as far as it takes to keep the cursor on screen.
    let scroll = cur_row.saturating_sub(visible.saturating_sub(1));

    let mut y = top as f32 + lh;
    for (idx, &(line_i, seg_start, seg_end)) in
        rows.iter().enumerate().skip(scroll).take(visible)
    {
        let chars: Vec<char> = app.scratchpad.lines[line_i].chars().collect();
        let on_cursor_row = idx == cur_row;

        // The selection is painted first, so the cursor and the glyphs land on
        // top of it. It arrives in logical (line, char) coordinates, which is
        // what makes a selection spanning a wrapped line come out right without
        // this loop knowing anything about the wrap.
        if let Some(sel) = selection {
            let in_range = line_i >= sel.lo.0 && line_i <= sel.hi.0;
            let band_y = (y - lh * 0.72) as i32;
            let band_h = (lh * 0.85) as i32;
            if sel.linewise && in_range {
                // Edge to edge, so that selecting whole lines does not look
                // identical to selecting the characters that happen to sit on
                // them -- and so an empty line in the range is visible at all.
                let w = (canvas.w as f32 - PAD * 2.0) as i32;
                canvas.fill_rect(PAD as i32, band_y, w, band_h, SELECT);
            } else if in_range {
                let from = if line_i == sel.lo.0 { sel.lo.1 } else { 0 };
                let to = if line_i == sel.hi.0 { sel.hi.1 + 1 } else { chars.len() };
                // Clip the logical range down to the slice this row shows.
                let (from, to) = (from.max(seg_start), to.min(seg_end));
                if from < to {
                    let w = ((to - from) as f32 * adv) as i32;
                    let x = PAD + (from - seg_start) as f32 * adv;
                    canvas.fill_rect(x as i32, band_y, w, band_h, SELECT);
                }
            }
        }

        if on_cursor_row {
            let cx_px = PAD + cur_col as f32 * adv;
            let (w, top_y, h) = if app.scratchpad.mode == Mode::Insert {
                (2, y - lh * 0.72, lh * 0.85)
            } else {
                (adv as i32, y - lh * 0.72, lh * 0.85)
            };
            canvas.fill_rect(cx_px as i32, top_y as i32, w, h as i32, ACCENT);
        }

        let mut pen = PAD;
        for (k, c) in chars[seg_start..seg_end].iter().enumerate() {
            // Keep the character under a block cursor legible.
            let under_block =
                on_cursor_row && k == cur_col && app.scratchpad.mode != Mode::Insert;
            let color = if under_block { BG } else { FG };
            pen = text.draw(canvas, pen, y, &c.to_string(), color);
        }
        y += lh;
    }

    if rows.len() > visible {
        let marker = format!("{}/{}", cur_row + 1, rows.len());
        let x = canvas.w as f32 - PAD - text.width_of(&marker);
        text.draw(canvas, x, bottom as f32 - lh * 0.3, &marker, FAINT);
    }

    if app.scratchpad.lines.len() == 1
        && app.scratchpad.lines[0].is_empty()
        && app.scratchpad.mode == Mode::Command
    {
        text.draw(canvas, PAD, top as f32 + lh * 2.4, "press i to start typing", FAINT);
    }
}

fn draw_clipboard(app: &App, text: &mut TextRenderer, canvas: &mut Canvas, top: i32, bottom: i32) {
    let lh = text.line_height();
    let adv = text.advance();
    let clip = &app.clipboard;
    let mut y = top as f32 + lh * 1.2;

    if clip.capture_paused {
        text.draw(canvas, PAD, y, "capture paused - c resumes", ERR);
        y += lh * 1.4;
    }

    if clip.mode == ClipMode::Search {
        let pen = text.draw(canvas, PAD, y, "/", ACCENT);
        let pen = text.draw(canvas, pen, y, &clip.query, FG);
        canvas.fill_rect(pen as i32 + 1, (y - lh * 0.72) as i32, 2, (lh * 0.85) as i32, ACCENT);
        y += lh * 1.4;
    }

    let visible = clip.visible();
    if visible.is_empty() {
        let msg = if clip.entries().is_empty() {
            "nothing captured yet - copy something"
        } else {
            "nothing matches"
        };
        text.draw(canvas, PAD, y + lh, msg, FAINT);
        return;
    }

    // Reserve room for the mask banner so the list does not jump when revealed.
    if !clip.revealed {
        text.draw(
            canvas,
            PAD,
            y,
            "hidden - press any key to reveal, esc to leave without looking",
            DIM,
        );
        y += lh * 1.4;
    }

    let cols = ((canvas.w as f32 - PAD * 2.0 - adv * 4.0) / adv) as usize;
    let rows = (((bottom as f32 - y) / lh).floor() as usize).max(1);
    // Scroll so the selection stays on screen.
    let start = clip.selected.saturating_sub(rows.saturating_sub(1));

    for (row, &entry_i) in visible.iter().enumerate().skip(start).take(rows) {
        let entry = &clip.entries()[entry_i];
        let selected = row == clip.selected;

        if selected {
            canvas.fill_rect(
                (PAD - 8.0) as i32,
                (y - lh * 0.78) as i32,
                canvas.w - (PAD as i32 - 8) * 2,
                lh as i32,
                TAB_ACTIVE,
            );
        }

        let marker = if entry.pinned { "*" } else { " " };
        text.draw(canvas, PAD, y, marker, if entry.pinned { ACCENT } else { FAINT });

        let body = if clip.revealed {
            entry.preview(cols)
        } else {
            entry.masked(cols)
        };
        let color = if !clip.revealed {
            DIM
        } else if selected {
            FG
        } else {
            DIM
        };
        text.draw(canvas, PAD + adv * 2.0, y, &body, color);
        y += lh;
    }

    if visible.len() > rows {
        let marker = format!("{}/{}", clip.selected + 1, visible.len());
        let x = canvas.w as f32 - PAD - text.width_of(&marker);
        text.draw(canvas, x, bottom as f32 - lh * 0.3, &marker, FAINT);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_color(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    canvas: &mut Canvas,
    top: i32,
    bottom: i32,
) {
    let c = &app.color;
    let lh = text.line_height();
    let input_baseline = bottom as f32 - lh * 0.6;

    if let Some(col) = c.current {
        // The swatch is the answer; everything else is a description of it.
        let sw_x = PAD as i32;
        let sw_y = top + 24;
        let sw_w = 300;
        let sw_h = 150;
        let packed = (col.r as u32) << 16 | (col.g as u32) << 8 | col.b as u32;
        canvas.fill_rect(sw_x, sw_y, sw_w, sw_h, packed);

        // The hex, written on the swatch. Black or white by luminance, because
        // one fixed colour is unreadable against half the spectrum.
        let label = col.to_hex();
        let on_swatch = if col.prefers_dark_text() { 0x00_0000 } else { 0xFF_FFFF };
        // Drop to the small font when the big one will not fit inside the
        // swatch. At 58px, seven characters are wider than 220px, so the label
        // ran off the left edge and out of the panel.
        let fits = big.width_of(&label) <= (sw_w - 16) as f32;
        let (w, baseline_nudge) = if fits {
            (big.width_of(&label), big.line_height() * 0.32)
        } else {
            (text.width_of(&label), text.line_height() * 0.32)
        };
        let lx = sw_x as f32 + ((sw_w as f32 - w) / 2.0).max(8.0);
        let by = (sw_y + sw_h / 2) as f32 + baseline_nudge;
        if fits {
            big.draw(canvas, lx, by, &label, on_swatch);
        } else {
            text.draw(canvas, lx, by, &label, on_swatch);
        }

        let tx = (sw_x + sw_w) as f32 + PAD * 1.5;
        let mut y = sw_y as f32 + lh * 1.4;
        let (name, exact) = col.nearest_name();
        for (label, value) in [
            ("hex", col.to_hex()),
            ("rgb", col.to_rgb_string()),
            ("hsl", col.to_hsl_string()),
            (if exact { "name" } else { "nearest" }, name.to_string()),
        ] {
            text.draw(canvas, tx, y, label, FAINT);
            text.draw(canvas, tx + text.advance() * 9.0, y, &value, FG);
            y += lh * 1.35;
        }
    } else if !c.picking {
        let msg = "type a colour, or press tab to pick one off the screen";
        text.draw(canvas, PAD, top as f32 + lh * 3.0, msg, FAINT);
    }

    if c.picking {
        let msg = "click anywhere on screen to pick a colour";
        text.draw(canvas, PAD, top as f32 + lh * 3.0, msg, ACCENT);
    }

    canvas.fill_rect(
        PAD as i32,
        (input_baseline - lh * 1.1) as i32,
        canvas.w - PAD as i32 * 2,
        1,
        RULE,
    );
    let pen = text.draw(canvas, PAD, input_baseline, "> ", ACCENT);
    let pen = text.draw(canvas, pen, input_baseline, &c.input, FG);
    canvas.fill_rect(pen as i32 + 1, (input_baseline - lh * 0.72) as i32, 2, (lh * 0.85) as i32, ACCENT);

    if let Some(p) = c.problem {
        text.draw(canvas, PAD, input_baseline - lh * 1.8, p, ERR);
    } else if let Some(n) = &c.notice {
        text.draw(canvas, PAD, input_baseline - lh * 1.8, n, ACCENT);
    }
}

/// Where the calculator's input line and error line sit, given the bottom of
/// the content area. Split out from the drawing so the arithmetic can be
/// tested: an error drawn past `bottom` lands under the hint row and off the
/// panel, which is invisible in every test that only checks state.
fn calculator_lines(bottom: f32, lh: f32, has_error: bool) -> (f32, Option<f32>) {
    let err_h = if has_error { lh } else { 0.0 };
    let input = bottom - lh * 0.6 - err_h;
    (input, has_error.then_some(input + lh))
}

fn draw_calendar(app: &App, text: &mut TextRenderer, canvas: &mut Canvas, top: i32, _bottom: i32) {
    use dropkick_core::calendar::Mode;
    use dropkick_core::date::{format_time, Date, MONTH_NAMES, WEEKDAY_NAMES};

    let c = &app.calendar;
    let lh = text.line_height();
    let adv = text.advance();

    // Grid on the left, the focused day's entries on the right. The split is
    // fixed rather than proportional so the grid does not reflow as entries
    // come and go.
    let grid_w = adv * 30.0;
    let cell_w = grid_w / 7.0;
    let list_x = PAD + grid_w + adv * 3.0;

    let month = &c.focus;
    let title = format!("{} {}", MONTH_NAMES[(month.month - 1) as usize], month.year);
    let mut y = top as f32 + lh * 1.6;
    text.draw(canvas, PAD, y, &title, FG);
    y += lh * 1.4;

    for (i, name) in WEEKDAY_NAMES.iter().enumerate() {
        let x = PAD + i as f32 * cell_w;
        text.draw(canvas, x, y, name, FAINT);
    }
    y += lh * 0.4;

    let first = month.first_of_month();
    let lead = first.weekday(); // Monday-first
    let days = Date::days_in_month(month.year, month.month);
    let mut row_y = y + lh;

    for day in 1..=days {
        let col = (lead + day - 1) % 7;
        if col == 0 && day > 1 {
            row_y += lh * 1.25;
        }
        let date = Date::new(month.year, month.month, day);
        let x = PAD + col as f32 * cell_w;
        let label = format!("{day:>2}");

        let focused = date == c.focus;
        if focused {
            canvas.fill_rect(
                (x - adv * 0.4) as i32,
                (row_y - lh * 0.78) as i32,
                (adv * 2.9) as i32,
                (lh * 0.95) as i32,
                if c.mode == Mode::Grid { ACCENT } else { SELECT },
            );
        }
        let colour = if focused && c.mode == Mode::Grid {
            BG
        } else if date == c.today {
            ACCENT
        } else {
            FG
        };
        text.draw(canvas, x, row_y, &label, colour);

        // A dot under any day holding something, so a month reads at a glance.
        if c.has_entries(date) {
            let dot_x = (x + adv * 0.6) as i32;
            canvas.fill_rect(dot_x, (row_y + lh * 0.18) as i32, 3, 3, if focused && c.mode == Mode::Grid { BG } else { DIM });
        }
    }

    // The focused day, and what is on it.
    let heading = format!(
        "{} {} {}",
        WEEKDAY_NAMES[c.focus.weekday() as usize],
        c.focus.day,
        MONTH_NAMES[(c.focus.month - 1) as usize]
    );
    let mut ly = top as f32 + lh * 1.6;
    text.draw(canvas, list_x, ly, &heading, if c.focus == c.today { ACCENT } else { FG });
    ly += lh * 1.8;

    let entries = c.on(c.focus);
    if entries.is_empty() {
        text.draw(canvas, list_x, ly, "nothing scheduled", FAINT);
        ly += lh;
    }
    for (i, e) in entries.iter().enumerate() {
        let selected = c.mode == Mode::List && i == c.selected;
        if selected {
            canvas.fill_rect(
                (list_x - adv * 0.4) as i32,
                (ly - lh * 0.78) as i32,
                (canvas.w as f32 - list_x - PAD) as i32,
                (lh * 0.95) as i32,
                SELECT,
            );
        }
        // All-day entries line up under the times rather than jutting left.
        let when = e.time.map_or_else(|| "  -  ".to_string(), format_time);
        text.draw(canvas, list_x, ly, &when, if e.time.is_some() { ACCENT } else { FAINT });
        text.draw(canvas, list_x + adv * 6.5, ly, &e.text, FG);
        ly += lh * 1.2;
    }

    // Whatever is being typed, and the prompt for it.
    ly += lh * 0.6;
    match c.mode {
        Mode::Text => {
            let line = format!("what: {}", c.draft_text);
            let pen = text.draw(canvas, list_x, ly, &line, FG);
            canvas.fill_rect(pen as i32 + 1, (ly - lh * 0.72) as i32, 2, (lh * 0.85) as i32, ACCENT);
            ly += lh * 1.3;
            text.draw(canvas, list_x, ly, "enter for the time, esc to cancel", FAINT);
        }
        Mode::Time => {
            text.draw(canvas, list_x, ly, &format!("what: {}", c.draft_text), DIM);
            ly += lh * 1.3;
            let line = format!("when: {}", c.draft_time);
            let pen = text.draw(canvas, list_x, ly, &line, FG);
            canvas.fill_rect(pen as i32 + 1, (ly - lh * 0.72) as i32, 2, (lh * 0.85) as i32, ACCENT);
            ly += lh * 1.3;
            match c.problem {
                Some(msg) => text.draw(canvas, list_x, ly, msg, ERR),
                None => text.draw(canvas, list_x, ly, "15:00 or 3pm; blank for all day", FAINT),
            };
        }
        Mode::Disambiguate(h) => {
            text.draw(canvas, list_x, ly, &format!("what: {}", c.draft_text), DIM);
            ly += lh * 1.3;
            // Never guess: show both readings and make the user pick one.
            let q = format!("\"{}\" - which one?", c.draft_time.trim());
            text.draw(canvas, list_x, ly, &q, FG);
            ly += lh * 1.3;
            let am = if h == 12 { 0 } else { h };
            let pm = if h == 12 { 12 } else { h + 12 };
            let opts = format!("a  {}        p  {}", format_time(am * 60), format_time(pm * 60));
            text.draw(canvas, list_x, ly, &opts, ACCENT);
        }
        _ => {}
    }
}

fn draw_generate(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    canvas: &mut Canvas,
    top: i32,
    bottom: i32,
) {
    use dropkick_core::generate::Kind;
    let g = &app.generate;
    let lh = text.line_height();
    let middle = (top + bottom) as f32 / 2.0;

    // The value itself. A password is drawn at whatever size actually fits --
    // 128 characters at the headline size would run off the panel, and a
    // password you cannot read is a password you will get wrong.
    let value_y = middle + lh * 0.3;
    if let Some(problem) = g.problem {
        let x = (canvas.w as f32 - text.width_of(problem)) / 2.0;
        text.draw(canvas, x, value_y, problem, ERR);
    } else if g.value.is_empty() {
        let msg = "press r for a new one";
        let x = (canvas.w as f32 - text.width_of(msg)) / 2.0;
        text.draw(canvas, x, value_y, msg, FAINT);
    } else {
        let avail = canvas.w as f32 - PAD * 4.0;
        let renderer: &mut TextRenderer =
            if big.width_of(&g.value) <= avail { big } else { text };
        let x = ((canvas.w as f32 - renderer.width_of(&g.value)) / 2.0).max(PAD);
        renderer.draw(canvas, x, value_y, &g.value, FG);
    }

    // What it is, above.
    let title = match g.kind {
        Kind::Password => "password",
        Kind::Uuid => "UUID v4",
    };
    let tx = (canvas.w as f32 - text.width_of(title)) / 2.0;
    text.draw(canvas, tx, top as f32 + lh * 1.6, title, DIM);

    // The settings, below, spelled out rather than shown as toggles: the state
    // of the thing on screen should be readable without knowing the keymap.
    let mut y = middle + lh * 2.2;
    if g.kind == Kind::Password {
        let mut parts: Vec<String> = vec![format!("{} characters", g.length)];
        let mut kinds = Vec::new();
        for (on, name) in [
            (g.classes.lower, "a-z"),
            (g.classes.upper, "A-Z"),
            (g.classes.digits, "0-9"),
            (g.classes.symbols, "symbols"),
        ] {
            if on {
                kinds.push(name);
            }
        }
        parts.push(if kinds.is_empty() { "nothing selected".into() } else { kinds.join(" ") });
        if g.exclude_ambiguous {
            parts.push("no lookalikes".into());
        }
        let line = parts.join("   ");
        let sx = (canvas.w as f32 - text.width_of(&line)) / 2.0;
        text.draw(canvas, sx, y, &line, DIM);
        y += lh * 1.6;
    } else {
        y += lh * 0.6;
    }

    // The promise, stated on the tool that makes it. Rule 1 of the Clipboard is
    // that it keeps things; this is the tool that does not, and saying so is
    // the difference between a claim and a footnote in a README.
    let promise = if g.copied {
        "copied - and not saved to clipboard history"
    } else {
        "nothing here is stored, or kept after you close the panel"
    };
    let px = (canvas.w as f32 - text.width_of(promise)) / 2.0;
    text.draw(canvas, px, y, promise, if g.copied { ACCENT } else { FAINT });
}

fn draw_timer(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    now_ms: u64,
    canvas: &mut Canvas,
    top: i32,
    bottom: i32,
) {
    let t = &app.timer;
    let lh = text.line_height();
    let middle = (top + bottom) as f32 / 2.0;

    let readout = format_duration(t.display_ms(now_ms));
    let x = (canvas.w as f32 - big.width_of(&readout)) / 2.0;
    let color = if t.finished {
        ACCENT
    } else if t.is_running() {
        FG
    } else {
        DIM
    };
    big.draw(canvas, x, middle + big.line_height() * 0.3, &readout, color);

    let label = match (t.mode, t.is_running(), t.finished) {
        (_, _, true) => "finished".to_string(),
        (TimerMode::Countdown, true, _) => format!("counting down from {}", format_duration(t.duration_ms())),
        (TimerMode::Countdown, false, _) => "paused".to_string(),
        (TimerMode::Stopwatch, true, _) => "stopwatch".to_string(),
        (TimerMode::Stopwatch, false, _) => "stopped".to_string(),
    };
    let lx = (canvas.w as f32 - text.width_of(&label)) / 2.0;
    text.draw(canvas, lx, middle - big.line_height() * 0.75, &label, DIM);

    let mut y = middle + big.line_height() * 0.9 + lh;
    if !t.input.is_empty() {
        let entry = format!("> {}", t.input);
        let ex = (canvas.w as f32 - text.width_of(&entry)) / 2.0;
        let pen = text.draw(canvas, ex, y, &entry, FG);
        canvas.fill_rect(pen as i32 + 1, (y - lh * 0.72) as i32, 2, (lh * 0.85) as i32, ACCENT);
        y += lh;
    }
    if let Some(err) = &t.error {
        let ex = (canvas.w as f32 - text.width_of(err)) / 2.0;
        text.draw(canvas, ex, y, err, ERR);
    }
}

fn draw_hints(app: &App, text: &mut TextRenderer, canvas: &mut Canvas) {
    let y = canvas.h - HINT_H;
    canvas.fill_rect(0, y, canvas.w, HINT_H, TAB_BG);
    canvas.fill_rect(0, y, canvas.w, 1, RULE);
    let baseline = (canvas.h - 10) as f32;

    match &app.notice {
        Some(notice) => {
            text.draw(canvas, PAD, baseline, notice, ACCENT);
        }
        None => {
            text.draw(canvas, PAD, baseline, app.hints(), DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this pins: the error was drawn at `input + lh`, which is past
    /// the bottom of the content area, so it rendered under the hint row and
    /// was clipped by the panel edge. Reported from real use -- every unit test
    /// passed, because none of them knew where anything was painted.
    #[test]
    fn the_calculator_error_stays_inside_the_content_area() {
        let bottom = (HEIGHT as i32 - HINT_H) as f32;
        let lh = 23.0;
        let (input, err) = calculator_lines(bottom, lh, true);
        let err = err.expect("an error must get a line");
        assert!(err <= bottom, "error baseline {err} is past the content bottom {bottom}");
        assert!(input < err, "the error belongs below the input");
        assert!(input > 0.0);
    }

    #[test]
    fn without_an_error_the_input_keeps_its_place_at_the_bottom() {
        let bottom = (HEIGHT as i32 - HINT_H) as f32;
        let lh = 23.0;
        let (input, err) = calculator_lines(bottom, lh, false);
        assert!(err.is_none());
        assert!(input <= bottom);
        // Only the error case shifts things; nothing moves otherwise.
        assert_eq!(input, bottom - lh * 0.6);
    }

    #[test]
    fn an_error_moves_the_input_up_rather_than_overflowing() {
        let bottom = (HEIGHT as i32 - HINT_H) as f32;
        let lh = 23.0;
        let (with, _) = calculator_lines(bottom, lh, true);
        let (without, _) = calculator_lines(bottom, lh, false);
        assert!(with < without, "the input has to make room");
        assert_eq!(without - with, lh);
    }
}
