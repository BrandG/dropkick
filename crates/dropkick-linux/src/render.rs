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
        _ => {
            let y = (content_top + content_bottom) as f32 / 2.0;
            let msg = "not built yet";
            let x = (canvas.w as f32 - text.width_of(msg)) / 2.0;
            text.draw(canvas, x, y, msg, DIM);
        }
    }

    draw_hints(app, text, canvas);
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
    let input_baseline = bottom as f32 - lh * 0.6;

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

    if let Some(err) = &app.calculator.error {
        text.draw(canvas, PAD, input_baseline + lh, err, ERR);
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

    let visible = (((bottom - top) as f32 / lh).floor() as usize).max(1);
    // Scroll only as far as it takes to keep the cursor on screen.
    let scroll = cur_row.saturating_sub(visible.saturating_sub(1));

    let mut y = top as f32 + lh;
    for (idx, &(line_i, seg_start, seg_end)) in
        rows.iter().enumerate().skip(scroll).take(visible)
    {
        let chars: Vec<char> = app.scratchpad.lines[line_i].chars().collect();
        let on_cursor_row = idx == cur_row;

        if on_cursor_row {
            let cx_px = PAD + cur_col as f32 * adv;
            let (w, top_y, h) = match app.scratchpad.mode {
                Mode::Command => (adv as i32, y - lh * 0.72, lh * 0.85),
                Mode::Insert => (2, y - lh * 0.72, lh * 0.85),
            };
            canvas.fill_rect(cx_px as i32, top_y as i32, w, h as i32, ACCENT);
        }

        let mut pen = PAD;
        for (k, c) in chars[seg_start..seg_end].iter().enumerate() {
            // Keep the character under a block cursor legible.
            let under_block =
                on_cursor_row && k == cur_col && app.scratchpad.mode == Mode::Command;
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
