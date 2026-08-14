//! Painting the panel: tab bar, tool content, hint row.
//!
//! The hint row is not a button bar. It is a reminder of the keys that already
//! work, which is what lets a newcomer learn the grammar without a manual and
//! lets everyone else ignore it.

use dropkick_core::app::{App, ToolId, TOOLS};
use dropkick_core::notepad::Mode;

use crate::font::{Canvas, TextRenderer};

pub const WIDTH: u32 = 760;
pub const HEIGHT: u32 = 440;

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

pub fn draw(app: &App, text: &mut TextRenderer, canvas: &mut Canvas) {
    canvas.fill(BG);
    draw_tabs(app, text, canvas);
    canvas.fill_rect(0, TAB_H, canvas.w, 1, RULE);

    let content_top = TAB_H + 1;
    let content_bottom = canvas.h - HINT_H;
    match app.active {
        ToolId::Calculator => draw_calculator(app, text, canvas, content_top, content_bottom),
        ToolId::Notepad => draw_notepad(app, text, canvas, content_top, content_bottom),
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
    let mut x = PAD;

    for tool in TOOLS.iter() {
        let label = tool.name;
        let chord = format!("alt+{}", tool.key);
        let w = text.width_of(label) + text.width_of(&chord) + text.advance() * 2.0;
        let active = app.active == tool.id;

        if active {
            canvas.fill_rect((x - 10.0) as i32, 4, (w + 20.0) as i32, TAB_H - 4, TAB_ACTIVE);
            // Accent underline: which tab owns the keyboard right now.
            canvas.fill_rect((x - 10.0) as i32, TAB_H - 3, (w + 20.0) as i32, 3, ACCENT);
        }

        let name_color = if !tool.built {
            FAINT
        } else if active {
            FG
        } else {
            DIM
        };
        let mut pen = text.draw(canvas, x, baseline, label, name_color);
        pen += text.advance();
        text.draw(canvas, pen, baseline, &chord, if active { ACCENT } else { FAINT });

        x += w + text.advance() * 2.0;
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

fn draw_notepad(app: &App, text: &mut TextRenderer, canvas: &mut Canvas, top: i32, bottom: i32) {
    let lh = text.line_height();
    let adv = text.advance();
    let mut y = top as f32 + lh;
    let n = app.notepad.lines.len();

    for (i, line) in app.notepad.lines.iter().enumerate() {
        if y > bottom as f32 {
            break;
        }
        if i == app.notepad.cy {
            let cursor_x = PAD + app.notepad.cx as f32 * adv;
            match app.notepad.mode {
                Mode::Command => {
                    // Block cursor sits *on* a character.
                    canvas.fill_rect(
                        cursor_x as i32,
                        (y - lh * 0.72) as i32,
                        adv as i32,
                        (lh * 0.85) as i32,
                        ACCENT,
                    );
                }
                Mode::Insert => {
                    canvas.fill_rect(
                        cursor_x as i32,
                        (y - lh * 0.72) as i32,
                        2,
                        (lh * 0.85) as i32,
                        ACCENT,
                    );
                }
            }
        }

        // Draw the character under a block cursor in the background colour so it
        // stays readable.
        let mut pen = PAD;
        for (ci, c) in line.chars().enumerate() {
            let under_block = i == app.notepad.cy
                && ci == app.notepad.cx
                && app.notepad.mode == Mode::Command;
            let color = if under_block { BG } else { FG };
            pen = text.draw(canvas, pen, y, &c.to_string(), color);
        }
        y += lh;
    }

    if n == 1 && app.notepad.lines[0].is_empty() && app.notepad.mode == Mode::Command {
        text.draw(canvas, PAD, top as f32 + lh * 2.4, "press i to start typing", FAINT);
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
