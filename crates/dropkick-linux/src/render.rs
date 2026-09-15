//! Painting the panel: tab bar, tool content, hint row.
//!
//! The hint row is not a button bar. It is a reminder of the keys that already
//! work, which is what lets a newcomer learn the grammar without a manual and
//! lets everyone else ignore it.

use dropkick_core::app::{App, ToolId, TOOLS};
use dropkick_core::help;
use dropkick_core::clipboard::Mode as ClipMode;
use dropkick_core::scratchpad::Mode;
use dropkick_core::timer::{format_duration, Mode as TimerMode};
use dropkick_core::text::{cursor_cell, wrap_line};

use crate::font::{Canvas, TextRenderer};

// Seven tabs at "q Calculator" width need ~870px; the rest is headroom for
// the three unspent slots.
pub const WIDTH: u32 = 940;
pub const HEIGHT: u32 = 460;

/// How far the shadow reaches beyond the panel.
///
/// Left, right and below only. The panel is anchored to the top of the screen
/// with no margin, so a shadow above it would fall off the edge -- the same
/// reason the top corners are square.
pub const SHADOW: u32 = 32;

/// The surface Dropkick actually asks for: the panel plus room for its shadow.
pub const SURFACE_W: u32 = WIDTH + SHADOW * 2;
pub const SURFACE_H: u32 = HEIGHT + SHADOW;

/// Darkest the shadow gets, right against the panel edge.
const SHADOW_ALPHA: f32 = 150.0;

/// How quickly the shadow gives up as it moves away from the panel.
///
/// Was 2.0, and that was wrong: a square falloff puts nearly all of the shadow
/// in the first third of the margin, so past about 12px it was under 10% black
/// and simply not visible. 1.5 keeps the outer edge soft -- which is why the
/// falloff is curved at all -- while leaving something to see further out.
///
/// Worth knowing when tuning: **a black shadow on a dark desktop has very
/// little to work with.** Against a mid-grey window, 40% black moves a channel
/// by about 18 levels; against a light one it moves it by nearly 100. There is
/// no value here that makes it strong on a dark background without making it a
/// smear on a light one.
const SHADOW_FALLOFF: f32 = 1.5;

const TAB_H: i32 = 40;
const HINT_H: i32 = 30;
const PAD: f32 = 18.0;

const BG: u32 = 0x14_171C;
const TAB_BG: u32 = 0x0E_1115;
const TAB_ACTIVE: u32 = 0x1E_242E;
/// The Scratchpad's accent, and the colour the panel wore before the rest of
/// the tools had their own. Kept as the fallback.
const ACCENT: u32 = 0x6F_B3F2;

/// One accent per tool, so which tool you are in is readable from the corner of
/// your eye rather than by reading the tab bar.
///
/// Chosen with three constraints, not for prettiness:
///
/// * **Well clear of `ERR`** (`0xE06C75`, a salmon red). An accent that could be
///   mistaken for an error message would undo the point of having an error
///   colour. Nothing here is redder than 320 degrees of hue.
/// * **Light enough to read as text on `BG`**, and light enough that `BG` reads
///   as text on *it* -- the Scratchpad's block cursor fills with the accent and
///   draws the character in the background colour.
/// * **Separated in hue**, not just in name. Two tools a few degrees apart are
///   two tools you cannot tell apart in peripheral vision, which is the only
///   thing this is for.
fn accent(id: ToolId) -> u32 {
    match id {
        ToolId::Scratchpad => ACCENT,      // blue
        ToolId::Clipboard => 0xB4_8EF2,    // violet
        ToolId::Calculator => 0x7F_D69B,   // green
        ToolId::Calendar => 0xE8_B36F,     // amber
        ToolId::Timer => 0x5F_D3D6,        // cyan
        ToolId::Generate => 0xB8_D96F,     // lime
        ToolId::Color => 0xE8_8FD0,        // orchid
    }
}
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
/// Corner radius. Small on purpose: this is a utility panel, not a phone card.
const RADIUS: i32 = 10;
/// Whether the top corners are rounded.
///
/// **False, and tied to `Anchor::TOP` in `main.rs`.** The panel is anchored to
/// the top of the screen with no margin, so its top edge is flush at y=0.
/// Rounding a corner that sits against the screen edge cuts a notch out of
/// nothing and makes the panel look like it slid off the top; edge-docked
/// surfaces square the docked edge and round the free ones.
///
/// If the anchor ever gains a margin, or moves, set this true.
const ROUND_TOP: bool = false;

/// `now_ms` is passed rather than kept on `App`: a clock stored as state is a
/// clock that can be stale, and the timer readout is the one thing here that
/// must never be.
/// Paint the whole surface: the shadow, then the panel inside it.
///
/// The panel is drawn into a *view* of this canvas offset by [`SHADOW`], so
/// every drawing function still works in panel coordinates and still centres on
/// `canvas.w`. Nothing below this function knows the shadow exists.
pub fn draw_surface(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    now_ms: u64,
    canvas: &mut Canvas,
    opacity: u8,
) {
    canvas.clear();
    draw_shadow(canvas);
    {
        let mut panel = canvas.view(SHADOW as i32, 0, WIDTH as i32, HEIGHT as i32);
        draw(app, text, big, now_ms, &mut panel, 255);
    }
    // Applied over the whole surface so the shadow fades with the panel. A
    // solid shadow around a see-through panel looks like a rendering fault.
    apply_opacity(canvas, opacity);
}

/// A soft edge around the panel, so it reads as sitting above the desktop
/// rather than being cut into it.
///
/// Quadratic falloff: linear looks like a grey band with a hard outer edge,
/// because the eye is far better at spotting the end of a gradient than its
/// middle.
fn draw_shadow(canvas: &mut Canvas) {
    let m = SHADOW as f32;
    let (x0, y0) = (SHADOW as i32, 0i32);
    let (x1, y1) = (x0 + WIDTH as i32 - 1, HEIGHT as i32 - 1);
    let r = RADIUS as f32;

    for y in 0..canvas.h {
        for x in 0..canvas.w {
            // Inside the panel proper: leave it, the panel paints there.
            if x >= x0 && x <= x1 && y <= y1 {
                let bottom_corner = (y as f32) > y1 as f32 - r
                    && ((x as f32) < x0 as f32 + r || (x as f32) > x1 as f32 - r);
                if !bottom_corner {
                    continue;
                }
            }
            let d = distance_outside_panel(x as f32, y as f32, x0 as f32, y0 as f32, x1 as f32, y1 as f32, r);
            if d <= 0.0 || d > m {
                continue;
            }
            let t = 1.0 - d / m;
            let a = (SHADOW_ALPHA * t.powf(SHADOW_FALLOFF)).round() as u8;
            if a > 0 {
                canvas.put_alpha(x, y, 0x00_0000, a);
            }
        }
    }
}

/// Distance from a point to the panel's outline, zero or less when inside.
///
/// The panel is a rectangle with two rounded bottom corners, so the bottom
/// corners measure to the arc and everything else measures to the box.
fn distance_outside_panel(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    if y > y1 - r {
        if x < x0 + r {
            let (cx, cy) = (x0 + r, y1 - r);
            return ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r;
        }
        if x > x1 - r {
            let (cx, cy) = (x1 - r, y1 - r);
            return ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r;
        }
    }
    let dx = (x0 - x).max(x - x1).max(0.0);
    let dy = (y0 - y).max(y - y1).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// `opacity` is 0..=255 and applies to the whole panel, background and text
/// alike, which is what "make the window see-through" means everywhere else.
pub fn draw(
    app: &App,
    text: &mut TextRenderer,
    big: &mut TextRenderer,
    now_ms: u64,
    canvas: &mut Canvas,
    opacity: u8,
) {
    canvas.fill(BG);
    draw_tabs(app, text, canvas);
    canvas.fill_rect(0, TAB_H, canvas.w, 1, RULE);

    let content_top = TAB_H + 1;
    let content_bottom = canvas.h - HINT_H;
    if app.help {
        // Over the content, not over the panel. The tab bar stays lit so you
        // can see whose keys you are reading, and alt+h/l pages through them.
        draw_help(app, text, canvas, content_top, content_bottom);
        draw_hints(app, text, canvas);
        draw_border(canvas);
        round_corners(canvas);
        apply_opacity(canvas, opacity);
        return;
    }
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
    // Order matters. The corners have to be cut after everything opaque is
    // painted, or the tab bar and hint row -- which both run to the very edge
    // -- would square them off again. Opacity comes last so it scales what is
    // actually on screen, corners included.
    round_corners(canvas);
    apply_opacity(canvas, opacity);
}

/// Cut the four corners to a radius, carrying the border around the arc.
///
/// Antialiased by coverage rather than stepped: at this radius a hard edge
/// reads as a staircase, and the whole point is that the panel look deliberate.
fn round_corners(canvas: &mut Canvas) {
    let (w, h, r) = (canvas.w, canvas.h, RADIUS);
    if r <= 0 {
        return;
    }
    // Arc centres, one per corner, each inset by the radius.
    let bottom = [(r, h - 1 - r, -1, 1), (w - 1 - r, h - 1 - r, 1, 1)];
    let top = [(r, r, -1, -1), (w - 1 - r, r, 1, -1)];
    let corners: Vec<_> =
        if ROUND_TOP { top.iter().chain(bottom.iter()).copied().collect() } else { bottom.to_vec() };

    for (cx, cy, sx, sy) in corners {
        // Inclusive of r: the arc centre is inset by exactly the radius, so
        // `0..r` stops one pixel short and leaves the actual corner square.
        for i in 0..=r {
            for j in 0..=r {
                let x = cx + sx * i;
                let y = cy + sy * j;
                let (dx, dy) = ((x - cx) as f32, (y - cy) as f32);
                let d = (dx * dx + dy * dy).sqrt();
                let rf = r as f32;

                // Coverage: 1 well inside the arc, 0 well outside, a soft ramp
                // across the single pixel that straddles it.
                let coverage = (rf + 0.5 - d).clamp(0.0, 1.0);
                if coverage >= 1.0 && d <= rf - BORDER_W as f32 {
                    continue; // Untouched interior.
                }
                let colour = if d > rf - BORDER_W as f32 - 0.5 {
                    BORDER
                } else {
                    canvas.get(x, y)
                };
                canvas.put_alpha(x, y, colour, (coverage * 255.0).round() as u8);
            }
        }
    }
}

/// Scale the whole panel's alpha.
///
/// A single pass over the buffer, which is cheaper than it sounds: the pixels
/// were already being written once each, and this reads and writes them once
/// more. Skipped entirely when opaque, which is the default.
fn apply_opacity(canvas: &mut Canvas, opacity: u8) {
    if opacity == 255 {
        return;
    }
    let a = opacity as u32;
    for chunk in canvas.buf.chunks_exact_mut(4) {
        let px = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let existing = (px >> 24) & 0xFF;
        if existing == 0 {
            continue; // Already cut away by the corners.
        }
        // Everything here is premultiplied already, so scaling all four
        // channels by the same factor keeps it consistent.
        let scale = |c: u32| ((c * a + 127) / 255) & 0xFF;
        let out = (scale(existing) << 24)
            | (scale((px >> 16) & 0xFF) << 16)
            | (scale((px >> 8) & 0xFF) << 8)
            | scale(px & 0xFF);
        chunk.copy_from_slice(&out.to_le_bytes());
    }
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
            canvas.fill_rect((x - 9.0) as i32, TAB_H - 3, (w + 18.0) as i32, 3, accent(tool.id));
        }

        // A running timer is visible from any tab: the point of a countdown is
        // that you stop thinking about it.
        let live = tool.id == ToolId::Timer && app.timer.is_running();

        let key_color = if !tool.built {
            FAINT
        } else if active {
            accent(tool.id)
        } else {
            DIM
        };
        let name_color = if !tool.built {
            FAINT
        } else if active {
            FG
        } else if live {
            accent(tool.id)
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
    let pen = text.draw(canvas, PAD, input_baseline, prompt, accent(app.active));
    let pen = text.draw(canvas, pen, input_baseline, &app.calculator.input, FG);
    // Caret: a thin bar, because the calculator is always in text entry.
    canvas.fill_rect(
        pen as i32 + 1,
        (input_baseline - text.line_height() * 0.72) as i32,
        2,
        (text.line_height() * 0.85) as i32,
        accent(app.active),
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
            canvas.fill_rect(cx_px as i32, top_y as i32, w, h as i32, accent(app.active));
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
        let pen = text.draw(canvas, PAD, y, "/", accent(app.active));
        let pen = text.draw(canvas, pen, y, &clip.query, FG);
        canvas.fill_rect(pen as i32 + 1, (y - lh * 0.72) as i32, 2, (lh * 0.85) as i32, accent(app.active));
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
        text.draw(canvas, PAD, y, marker, if entry.pinned { accent(app.active) } else { FAINT });

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
/// `31` -> `31st`. Only ever called with a day of the month.
fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// The "what was hashed" line: how much text, and the start of it.
///
/// Both halves earn their place. The **count** is what catches a trailing
/// newline — the single most common reason a checksum does not match, and
/// completely invisible in a preview. The **preview** is what catches having
/// copied the wrong thing entirely.
///
/// Deliberately short. This is a tool that does not show clipboard contents
/// without being asked, and a full dump of what you hashed would be exactly
/// that by another route.
fn hashed_from(src: &str) -> String {
    const PREVIEW: usize = 32;
    let chars = src.chars().count();
    let unit = if chars == 1 { "character" } else { "characters" };

    // Control characters would draw as blanks or nothing at all, which would
    // make a preview that quietly lies about what it hashed.
    let shown: String = src
        .chars()
        .take(PREVIEW)
        .map(|c| if c.is_control() { '·' } else { c })
        .collect();
    let ellipsis = if chars > PREVIEW { "…" } else { "" };
    format!("{chars} {unit} of \"{shown}{ellipsis}\"")
}

/// The help page: the active tool's keys on the left, the ones that work
/// everywhere on the right.
///
/// Two columns rather than one long list, because the whole reason this is an
/// overlay and not a tab is that it fits on one screen. A help page that has
/// to be scrolled needs navigation keys, and those keys then need explaining.
fn draw_help(app: &App, text: &mut TextRenderer, canvas: &mut Canvas, top: i32, bottom: i32) {
    let lh = text.line_height();
    let adv = text.advance();
    let tool = app.active_tool();

    // Where the description starts within a column, in characters. The widest
    // key string decides it, so nothing collides and the descriptions line up.
    let column = |rows: &[help::Row]| {
        rows.iter().map(|r| r.keys.chars().count()).max().unwrap_or(0) as f32 + 2.0
    };

    let left_x = PAD;
    let right_x = (canvas.w / 2) as f32 + PAD;
    let rows = app.help_rows();
    let from = help::clamp_scroll(app.help_scroll, rows.len());

    let draw_column =
        |text: &mut TextRenderer, canvas: &mut Canvas, x, title: &str, rows: &[help::Row], tint| {
            text.draw(canvas, x, top as f32 + lh * HELP_TITLE, title, tint);
            // The gap is measured over the *whole* table, not the visible
            // slice, so the description column does not jump left and right
            // as it scrolls past a long key string.
            let gap = adv * column(rows);
            for (i, r) in rows.iter().take(help::VISIBLE_ROWS).enumerate() {
                let y = help_row_y(lh, top, i);
                // A row with no keys is a note about the tool rather than a
                // key, so it sits in the description column on its own.
                text.draw(canvas, x, y, r.keys, FG);
                text.draw(canvas, x + gap, y, r.what, DIM);
            }
        };

    let title = tool.name.to_uppercase();
    draw_column(text, canvas, left_x, &title, &rows[from..], accent(app.active));
    draw_column(text, canvas, right_x, "EVERYWHERE", help::GLOBAL, DIM);

    // Which slice of a long table is on screen. Beside the heading rather than
    // on a row of its own, since a row is the thing in short supply -- and it
    // only appears when there is something off screen, so a page that fits
    // never grows a counter telling you it fits.
    if rows.len() > help::VISIBLE_ROWS {
        let last = (from + help::VISIBLE_ROWS).min(rows.len());
        let label = format!("{}-{} of {}", from + 1, last, rows.len());
        let x = left_x + adv * (title.chars().count() as f32 + 3.0);
        text.draw(canvas, x, top as f32 + lh * HELP_TITLE, &label, FAINT);
    }

    // A hairline between the columns, so the eye does not read across.
    let mid = canvas.w / 2 + PAD as i32 / 2;
    canvas.fill_rect(mid, top + (lh * 1.2) as i32, 1, bottom - top - (lh * 2.0) as i32, RULE);
}

/// The column heading, in line-heights below the top of the content area.
const HELP_TITLE: f32 = 1.2;
/// The first row, and the step between them. Tight: the Scratchpad has
/// thirteen rows and 389px of panel to put them in, and the alternative to
/// fitting is a page that scrolls.
const HELP_FIRST: f32 = 2.6;
const HELP_STEP: f32 = 1.1;

/// Baseline of row `i`. Shared with the test that checks the longest table
/// fits, so the check cannot drift from the drawing.
fn help_row_y(lh: f32, top: i32, i: usize) -> f32 {
    top as f32 + lh * HELP_FIRST + lh * HELP_STEP * i as f32
}

/// Said under the opening swatch, which nobody asked for and which would
/// otherwise look like an answer to a question they do not remember asking.
///
/// It does not repeat the syntax -- the hint row along the bottom is already
/// listing every form the input accepts, and saying it twice on one screen
/// makes both look like noise.
const COLOR_SEED_NOTE: &str = "a starting colour, so the tab is not empty -- type your own below";

/// Said when there is no colour at all. Only reachable through
/// `ColorTool::default`, but it is what the pane says if the seed ever goes
/// away, so it has to be right and it has to fit.
///
/// It follows the eyedropper switch: offering `tab` while the key answers
/// "the eyedropper is off" is how a panel that explains itself stops being
/// believed.
fn color_prompt(eyedropper: bool) -> &'static str {
    if eyedropper {
        "type a colour and press enter, or press tab to pick one off the screen"
    } else {
        "type a colour and press enter -- #6FB3F2, rgb(111,179,242), hsl(210,82%,69%) or a name"
    }
}

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

        // The opening colour is scenery, not an answer. Say so underneath it,
        // or the tool looks like it has already been asked something.
        if c.seeded && !c.picking {
            let below = (sw_y + sw_h) as f32 + lh * 1.6;
            text.draw(canvas, PAD, below, COLOR_SEED_NOTE, FAINT);
        }
    } else if !c.picking {
        text.draw(canvas, PAD, top as f32 + lh * 3.0, color_prompt(c.eyedropper), FAINT);
    }

    if c.picking {
        let msg = "click anywhere on screen to pick a colour";
        text.draw(canvas, PAD, top as f32 + lh * 3.0, msg, accent(app.active));
    }

    canvas.fill_rect(
        PAD as i32,
        (input_baseline - lh * 1.1) as i32,
        canvas.w - PAD as i32 * 2,
        1,
        RULE,
    );
    let pen = text.draw(canvas, PAD, input_baseline, "> ", accent(app.active));
    let pen = text.draw(canvas, pen, input_baseline, &c.input, FG);
    canvas.fill_rect(pen as i32 + 1, (input_baseline - lh * 0.72) as i32, 2, (lh * 0.85) as i32, accent(app.active));

    if let Some(p) = c.problem {
        text.draw(canvas, PAD, input_baseline - lh * 1.8, p, ERR);
    } else if let Some(n) = &c.notice {
        text.draw(canvas, PAD, input_baseline - lh * 1.8, n, accent(app.active));
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
    use dropkick_core::calendar::{Mode, Repeat};
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
                if c.mode == Mode::Grid { accent(app.active) } else { SELECT },
            );
        }
        let colour = if focused && c.mode == Mode::Grid {
            BG
        } else if date == c.today {
            accent(app.active)
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
    text.draw(canvas, list_x, ly, &heading, if c.focus == c.today { accent(app.active) } else { FG });
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
        text.draw(canvas, list_x, ly, &when, if e.time.is_some() { accent(app.active) } else { FAINT });
        let pen = text.draw(canvas, list_x + adv * 6.5, ly, &e.text, FG);
        // How often it comes back, after the text and dimmer than it. An
        // entry that happens once says nothing at all rather than "never".
        if !e.repeat.label().is_empty() {
            text.draw(canvas, pen + adv * 1.5, ly, e.repeat.label(), DIM);
        }
        ly += lh * 1.2;
    }

    // The rule the user has to know before trusting a monthly entry, said
    // where they are choosing it rather than buried in the help page.
    if c.mode == Mode::List {
        if let Some(e) = entries.get(c.selected) {
            if e.repeat == Repeat::Monthly && e.date.day > 28 {
                let note = format!(
                    "the {} is skipped in months without one",
                    ordinal(e.date.day)
                );
                ly += lh * 0.3;
                text.draw(canvas, list_x, ly, &note, FAINT);
                ly += lh * 1.1;
            }
        }
    }

    // Whatever is being typed, and the prompt for it.
    ly += lh * 0.6;
    match c.mode {
        Mode::Text => {
            let line = format!("what: {}", c.draft_text);
            let pen = text.draw(canvas, list_x, ly, &line, FG);
            canvas.fill_rect(pen as i32 + 1, (ly - lh * 0.72) as i32, 2, (lh * 0.85) as i32, accent(app.active));
            ly += lh * 1.3;
            text.draw(canvas, list_x, ly, "enter for the time, esc to cancel", FAINT);
        }
        Mode::Time => {
            text.draw(canvas, list_x, ly, &format!("what: {}", c.draft_text), DIM);
            ly += lh * 1.3;
            let line = format!("when: {}", c.draft_time);
            let pen = text.draw(canvas, list_x, ly, &line, FG);
            canvas.fill_rect(pen as i32 + 1, (ly - lh * 0.72) as i32, 2, (lh * 0.85) as i32, accent(app.active));
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
            text.draw(canvas, list_x, ly, &opts, accent(app.active));
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
        let msg = if g.kind == Kind::Hash {
            "press h to hash the clipboard"
        } else {
            "press r for a new one"
        };
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
        Kind::Password => "password".to_string(),
        Kind::Uuid => "UUID v4".to_string(),
        Kind::Hash => g.algo.name().to_string(),
    };
    let tx = (canvas.w as f32 - text.width_of(&title)) / 2.0;
    text.draw(canvas, tx, top as f32 + lh * 1.6, &title, DIM);

    // What was hashed, under the title. A digest on its own is unverifiable:
    // a trailing newline changes it completely and the wrong answer looks
    // exactly as plausible as the right one.
    if g.kind == Kind::Hash {
        if let Some(src) = &g.source {
            let line = hashed_from(src);
            let x = (canvas.w as f32 - text.width_of(&line)) / 2.0;
            text.draw(canvas, x.max(PAD), top as f32 + lh * 3.0, &line, FAINT);
        }
    }

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
    } else if g.kind == Kind::Hash {
        // The caution, on the two that have one. Drawn in the error colour
        // because it is the one thing on this panel somebody could come to
        // harm by not reading -- and only on the algorithms it applies to,
        // since a warning that is always there is a warning nobody sees.
        if let Some(caution) = g.algo.caution() {
            let cx = (canvas.w as f32 - text.width_of(caution)) / 2.0;
            text.draw(canvas, cx.max(PAD), y, caution, ERR);
        }
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
    text.draw(canvas, px, y, promise, if g.copied { accent(app.active) } else { FAINT });
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
        accent(app.active)
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
        canvas.fill_rect(pen as i32 + 1, (y - lh * 0.72) as i32, 2, (lh * 0.85) as i32, accent(app.active));
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
            text.draw(canvas, PAD, baseline, notice, accent(app.active));
        }
        None => {
            text.draw(canvas, PAD, baseline, app.hints(), DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dropkick_core::hash::Algo;

    fn alpha_at(buf: &[u8], w: i32, x: i32, y: i32) -> u8 {
        buf[((y * w + x) * 4 + 3) as usize]
    }

    /// Render a real panel into a buffer, so the corner and opacity passes are
    /// checked against what actually reaches the compositor rather than against
    /// a hand-made bitmap.
    fn rendered(opacity: u8) -> (Vec<u8>, i32, i32) {
        let (w, h) = (WIDTH as i32, HEIGHT as i32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        {
            let mut canvas = Canvas::new(&mut buf, w, h);
            canvas.fill(BG);
            draw_border(&mut canvas);
            round_corners(&mut canvas);
            apply_opacity(&mut canvas, opacity);
        }
        (buf, w, h)
    }

    /// Perceived brightness, the same WCAG relative luminance the Color tool
    /// uses. Duplicated rather than imported so this test does not depend on a
    /// tool's implementation to check the chrome.
    fn luminance(c: u32) -> f64 {
        let f = |v: u32| {
            let x = v as f64 / 255.0;
            if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f((c >> 16) & 0xFF) + 0.7152 * f((c >> 8) & 0xFF) + 0.0722 * f(c & 0xFF)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn hue(c: u32) -> f64 {
        let (r, g, b) =
            (((c >> 16) & 0xFF) as f64, ((c >> 8) & 0xFF) as f64, (c & 0xFF) as f64);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d == 0.0 {
            return 0.0;
        }
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h + 360.0) % 360.0
    }

    /// `help::VISIBLE_ROWS` is a promise core makes on the renderer's behalf:
    /// core owns the scroll position and cannot see a panel, so it states how
    /// many rows are on screen and this proves the panel actually has room for
    /// that many.
    ///
    /// It is the load-bearing half of scrolling. Claim one row too many and
    /// the last one is drawn under the hint row, where scrolling can never
    /// reach it.
    #[test]
    fn the_panel_has_room_for_as_many_rows_as_core_claims() {
        let text = match TextRenderer::new(16.0) {
            Ok(t) => t,
            Err(_) => return,
        };
        let lh = text.line_height();
        let (top, bottom) = (TAB_H + 1, HEIGHT as i32 - HINT_H);
        // A baseline, so allow for what hangs below it.
        let floor = bottom as f32 - lh * 0.4;
        let last = help_row_y(lh, top, help::VISIBLE_ROWS - 1);
        assert!(last <= floor, "{} rows reach {last}, past {floor}", help::VISIBLE_ROWS);

        // And one more would not fit, so the constant is the real maximum
        // rather than a number someone rounded down and forgot.
        let over = help_row_y(lh, top, help::VISIBLE_ROWS);
        assert!(over > floor, "there is room for more than VISIBLE_ROWS; raise it");
    }

    /// The line that makes a digest verifiable. A trailing newline changes a
    /// hash completely and is invisible in a preview, so the count is not
    /// decoration.
    #[test]
    fn ordinals_read_correctly_including_the_teens() {
        for (n, want) in [
            (1u32, "1st"), (2, "2nd"), (3, "3rd"), (4, "4th"),
            (11, "11th"), (12, "12th"), (13, "13th"),
            (21, "21st"), (22, "22nd"), (23, "23rd"), (29, "29th"),
            (30, "30th"), (31, "31st"),
        ] {
            assert_eq!(ordinal(n), want);
        }
    }

    #[test]
    fn the_hashed_from_line_shows_the_length_and_the_start() {
        assert_eq!(hashed_from("abc"), "3 characters of \"abc\"");
        assert_eq!(hashed_from("x"), "1 character of \"x\"");
        // The case this is for: same visible text, different hash.
        assert_eq!(hashed_from("abc\n"), "4 characters of \"abc·\"");
    }

    #[test]
    fn a_long_clipboard_is_previewed_not_dumped() {
        let line = hashed_from(&"a".repeat(500));
        assert!(line.starts_with("500 characters of"));
        assert!(line.contains('…'), "it says there is more: {line}");
        assert!(line.chars().count() < 60, "not a dump: {line}");
    }

    #[test]
    fn control_characters_in_a_preview_are_made_visible() {
        // A tab or a newline drawn as itself would be a preview that lies
        // about what was hashed.
        let line = hashed_from("a\tb\rc");
        assert!(!line.contains('\t') && !line.contains('\r'), "{line}");
        assert_eq!(line, "5 characters of \"a·b·c\"");
    }

    /// The caution is the one thing on that panel somebody could come to harm
    /// by not reading, so it must not be drawn off the edge — which is
    /// exactly what happened to the calculator's error line once.
    #[test]
    fn the_hash_cautions_fit_inside_the_panel() {
        let text = match TextRenderer::new(16.0) {
            Ok(t) => t,
            Err(_) => return,
        };
        for algo in [Algo::Sha256, Algo::Sha1, Algo::Md5] {
            if let Some(caution) = algo.caution() {
                let w = text.width_of(caution);
                assert!(
                    w <= WIDTH as f32 - PAD * 2.0,
                    "{} caution is {w}px, panel is {}px",
                    algo.name(),
                    WIDTH
                );
            }
        }
    }

    /// A digest has to be readable, which is the entire point of it being on
    /// screen. SHA-256 is the long one at 64 hex characters.
    #[test]
    fn a_sha256_digest_fits_on_one_line() {
        let big = match TextRenderer::new(34.0) {
            Ok(t) => t,
            Err(_) => return,
        };
        let text = match TextRenderer::new(16.0) {
            Ok(t) => t,
            Err(_) => return,
        };
        let digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let avail = WIDTH as f32 - PAD * 4.0;
        // The renderer falls back to the small font when the big one will not
        // fit; what must never happen is neither fitting.
        assert!(
            big.width_of(digest) <= avail || text.width_of(digest) <= avail,
            "a sha-256 digest fits in neither font"
        );
    }

    /// The global column is never scrolled, so it has to fit outright.
    #[test]
    fn the_everywhere_column_needs_no_scrolling() {
        assert!(
            help::GLOBAL.len() <= help::VISIBLE_ROWS,
            "the global column is drawn beside every page and cannot be scrolled"
        );
    }

    /// The two columns must not run into each other. The left one is as wide
    /// as its widest key string plus its longest description, and that is data
    /// in `help.rs` that can grow without anyone re-rendering the page.
    #[test]
    fn the_help_columns_do_not_collide() {
        let text = match TextRenderer::new(16.0) {
            Ok(t) => t,
            Err(_) => return,
        };
        let adv = text.advance();
        let right_x = (WIDTH / 2) as f32 + PAD;
        for tool in ALL_TOOLS {
            for eyedropper in [true, false] {
                let rows = help::rows(tool, eyedropper);
                let keys = rows.iter().map(|r| r.keys.chars().count()).max().unwrap_or(0);
                let gap = adv * (keys as f32 + 2.0);
                for r in rows {
                    let end = PAD + gap + text.width_of(r.what);
                    assert!(
                        end < right_x,
                        "{tool:?} row `{}` reaches {end}, past the second column at {right_x}",
                        r.keys
                    );
                }
            }
        }
        // The right column has the panel's edge to worry about instead.
        let keys = help::GLOBAL.iter().map(|r| r.keys.chars().count()).max().unwrap_or(0);
        let gap = adv * (keys as f32 + 2.0);
        for r in help::GLOBAL {
            let end = right_x + gap + text.width_of(r.what);
            assert!(end < WIDTH as f32 - PAD, "global row `{}` runs off the panel", r.keys);
        }
    }

    /// Both prompts have to fit between the margins. The calculator's error
    /// line was drawn off the right-hand edge for exactly this reason, and the
    /// font is fixed-width, so the check is arithmetic rather than a guess.
    #[test]
    fn the_colour_prompts_fit_inside_the_panel() {
        let text = match TextRenderer::new(16.0) {
            Ok(t) => t,
            Err(_) => return, // no font on this machine; the other tests still run
        };
        let room = WIDTH as f32 - PAD * 2.0;
        let lines = [color_prompt(true), color_prompt(false), COLOR_SEED_NOTE];
        for msg in lines {
            let w = text.width_of(msg);
            assert!(w <= room, "prompt is {w}px wide, only {room}px of panel: {msg}");
        }
    }

    const ALL_TOOLS: [ToolId; 7] = [
        ToolId::Scratchpad,
        ToolId::Clipboard,
        ToolId::Calculator,
        ToolId::Calendar,
        ToolId::Timer,
        ToolId::Generate,
        ToolId::Color,
    ];

    fn surface() -> (Vec<u8>, i32, i32) {
        let (w, h) = (SURFACE_W as i32, SURFACE_H as i32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        {
            let mut canvas = Canvas::new(&mut buf, w, h);
            canvas.clear();
            draw_shadow(&mut canvas);
        }
        (buf, w, h)
    }

    #[test]
    fn the_shadow_fades_out_before_the_surface_edge() {
        let (buf, w, h) = surface();
        // Hard against the left edge of the surface there should be nothing
        // left, or the shadow would end in a visible straight line.
        assert_eq!(alpha_at(&buf, w, 0, h / 2), 0, "shadow reaches the surface edge");
        assert_eq!(alpha_at(&buf, w, w - 1, h / 2), 0);
        assert_eq!(alpha_at(&buf, w, w / 2, h - 1), 0, "shadow reaches the bottom edge");
    }

    #[test]
    fn the_shadow_is_darkest_against_the_panel() {
        let (buf, w, h) = surface();
        let y = h / 2;
        let near = alpha_at(&buf, w, SHADOW as i32 - 1, y);
        let mid = alpha_at(&buf, w, SHADOW as i32 / 2, y);
        let far = alpha_at(&buf, w, 1, y);
        assert!(near > mid && mid > far, "not monotonic: {near} {mid} {far}");
        assert!(near > 60, "barely visible at the panel edge: {near}");
    }

    #[test]
    fn there_is_no_shadow_above_the_panel() {
        // Anchored to the top of the screen, so anything drawn above the panel
        // falls off the edge. The surface is not taller for nothing.
        let (buf, w, _h) = surface();
        assert_eq!(alpha_at(&buf, w, w / 2, 0), 0);
        assert_eq!(SURFACE_H, HEIGHT + SHADOW, "surface is only extended downward");
    }

    #[test]
    fn the_panel_area_is_left_for_the_panel() {
        let (buf, w, _h) = surface();
        // Well inside the panel, the shadow pass must not have painted.
        assert_eq!(alpha_at(&buf, w, SHADOW as i32 + 100, 100), 0);
    }

    #[test]
    fn the_shadow_follows_the_rounded_corner() {
        // Diagonally outside a bottom corner should still be shadowed; if the
        // corner were treated as square, this point would be inside the panel
        // and left blank.
        let (buf, w, _h) = surface();
        let x = SHADOW as i32 + 2;
        let y = HEIGHT as i32 - 3;
        assert!(alpha_at(&buf, w, x, y) > 0, "no shadow under the cut corner");
    }

    #[test]
    fn a_panel_view_draws_into_the_right_part_of_the_surface() {
        // The whole reason Canvas gained views: panel coordinates must land
        // offset inside the larger buffer, or everything is drawn 24px left.
        let (w, h) = (SURFACE_W as i32, SURFACE_H as i32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        {
            let mut canvas = Canvas::new(&mut buf, w, h);
            canvas.clear();
            let mut panel = canvas.view(SHADOW as i32, 0, WIDTH as i32, HEIGHT as i32);
            panel.fill_rect(0, 0, 1, 1, 0xFF_FFFF);
        }
        assert_eq!(alpha_at(&buf, w, 0, 0), 0, "painted outside the view");
        assert_eq!(alpha_at(&buf, w, SHADOW as i32, 0), 255, "view origin is wrong");
    }

    #[test]
    fn every_tool_has_its_own_accent() {
        let mut seen: Vec<u32> = Vec::new();
        for id in ALL_TOOLS {
            let a = accent(id);
            assert!(!seen.contains(&a), "{id:?} reuses an accent");
            seen.push(a);
        }
    }

    #[test]
    fn accents_are_readable_both_ways_round() {
        // As text on the panel, and as a block cursor with BG text on top of it.
        for id in ALL_TOOLS {
            let c = contrast(accent(id), BG);
            assert!(c >= 4.5, "{id:?} is only {c:.1}:1 against the background");
        }
    }

    #[test]
    fn no_accent_can_be_mistaken_for_an_error() {
        // ERR is a salmon red. An accent sitting next to it in hue would make
        // "something went wrong" and "you are in this tool" look alike.
        for id in ALL_TOOLS {
            let d = (hue(accent(id)) - hue(ERR)).abs().min(360.0 - (hue(accent(id)) - hue(ERR)).abs());
            assert!(d > 30.0, "{id:?} is {d:.0} degrees from the error colour");
        }
    }

    #[test]
    fn accents_are_separated_enough_to_tell_apart() {
        // Peripheral vision is the whole point; two tools a few degrees apart
        // are two tools that look the same when you are not looking at them.
        for (i, a) in ALL_TOOLS.iter().enumerate() {
            for b in ALL_TOOLS.iter().skip(i + 1) {
                let raw = (hue(accent(*a)) - hue(accent(*b))).abs();
                let d = raw.min(360.0 - raw);
                assert!(d > 25.0, "{a:?} and {b:?} are only {d:.0} degrees apart");
            }
        }
    }

    #[test]
    fn the_bottom_corners_are_cut_away() {
        let (buf, w, h) = rendered(255);
        for (x, y) in [(0, h - 1), (w - 1, h - 1)] {
            assert_eq!(alpha_at(&buf, w, x, y), 0, "corner ({x},{y}) should be transparent");
        }
        assert_eq!(alpha_at(&buf, w, w / 2, h / 2), 255, "and the middle is not");
    }

    #[test]
    fn the_top_corners_stay_square_because_the_panel_is_docked_there() {
        // Anchor::TOP with no margin puts the top edge at y=0. A rounded corner
        // against the screen edge is a notch cut out of nothing.
        let (buf, w, _h) = rendered(255);
        for (x, y) in [(0, 0), (w - 1, 0)] {
            assert_eq!(alpha_at(&buf, w, x, y), 255, "corner ({x},{y}) should be square");
        }
    }

    #[test]
    fn the_corner_edge_is_softened_rather_than_stepped() {
        // Somewhere along each arc there must be a partially covered pixel, or
        // the curve is a staircase.
        let (buf, w, _h) = rendered(255);
        let h = HEIGHT as i32;
        let soft = (0..RADIUS)
            .flat_map(|i| (0..RADIUS).map(move |j| (i, h - 1 - j)))
            .filter(|(i, j)| {
                let a = alpha_at(&buf, w, *i, *j);
                a > 0 && a < 255
            })
            .count();
        assert!(soft > 0, "no antialiased pixels on the arc");
    }

    #[test]
    fn opacity_scales_the_whole_panel_and_leaves_the_cut_alone() {
        let (buf, w, h) = rendered(128);
        assert_eq!(alpha_at(&buf, w, w / 2, h / 2), 128, "interior should be half opaque");
        assert_eq!(alpha_at(&buf, w, 0, h - 1), 0, "a cut corner cannot become visible");
    }

    #[test]
    fn full_opacity_leaves_the_buffer_untouched() {
        let (a, _, _) = rendered(255);
        let (b, w, h) = rendered(255);
        assert_eq!(a, b);
        assert_eq!(alpha_at(&b, w, w / 2, h / 2), 255);
    }

    #[test]
    fn translucent_pixels_stay_premultiplied() {
        // Wayland expects premultiplied ARGB. If the colour is not scaled with
        // the alpha, the panel is visibly too bright wherever it is see-through
        // -- and it looks like a compositor bug rather than ours.
        let (buf, w, h) = rendered(128);
        let i = (((h / 2) * w + w / 2) * 4) as usize;
        let (b, g, r, a) = (buf[i] as u32, buf[i + 1] as u32, buf[i + 2] as u32, buf[i + 3] as u32);
        assert_eq!(a, 128);
        for (chan, full) in [(r, (BG >> 16) & 0xFF), (g, (BG >> 8) & 0xFF), (b, BG & 0xFF)] {
            let want = (full * a + 127) / 255;
            assert_eq!(chan, want, "channel not premultiplied: {chan} vs {want}");
        }
    }

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
