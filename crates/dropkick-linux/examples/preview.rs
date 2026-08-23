//! Renders the panel and composites it over a checkerboard, honouring alpha.
//!
//! `--shot` writes PPM, which has no alpha channel, so it cannot show the
//! rounded corners or `panel.opacity` at all -- a transparent pixel is
//! premultiplied to black and looks like a black pixel. This exists so those
//! two can be reviewed by looking, the same way everything else is.
//!
//! ```
//! cargo run --release --example preview -- 255 /tmp/solid.ppm
//! cargo run --release --example preview -- 200 /tmp/translucent.ppm
//! ```
fn main() {
    let (w, h) = (dropkick_linux::render::WIDTH as i32, dropkick_linux::render::HEIGHT as i32);
    let opacity: u8 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(255);
    let mut text = dropkick_linux::font::TextRenderer::new(16.0).unwrap();
    let mut big = dropkick_linux::font::TextRenderer::new(58.0).unwrap();
    let mut app = dropkick_core::App::new();
    app.on_key(dropkick_core::KeyPress::alt('u'), 0);
    for c in "tomato".chars() { app.on_key(dropkick_core::KeyPress::ch(c), 0); }
    app.on_key(dropkick_core::KeyPress::plain(dropkick_core::Key::Enter), 0);

    let mut buf = vec![0u8; (w * h * 4) as usize];
    {
        let mut canvas = dropkick_linux::font::Canvas { buf: &mut buf, w, h };
        dropkick_linux::render::draw(&app, &mut text, &mut big, 0, &mut canvas, opacity);
    }
    // Composite over a checker so both transparency and the corners show.
    let m = 40;
    let (cw, ch) = (w + m * 2, h + m * 2);
    let mut out = Vec::with_capacity((cw * ch * 3) as usize);
    for y in 0..ch {
        for x in 0..cw {
            let sq = (((x / 24) + (y / 24)) % 2) as u8;
            let bg = if sq == 0 { [0x3A, 0x3D, 0x42] } else { [0x55, 0x59, 0x60] };
            let (px, py) = (x - m, y - m);
            if px >= 0 && px < w && py >= 0 && py < h {
                let i = ((py * w + px) * 4) as usize;
                let (b, g, r, a) = (buf[i] as u32, buf[i+1] as u32, buf[i+2] as u32, buf[i+3] as u32);
                // Source is premultiplied: out = src + dst * (1 - a).
                let f = |s: u32, d: u8| (s + (d as u32) * (255 - a) / 255).min(255) as u8;
                out.extend_from_slice(&[f(r, bg[0]), f(g, bg[1]), f(b, bg[2])]);
            } else {
                out.extend_from_slice(&bg);
            }
        }
    }
    let path = std::env::args().nth(2).unwrap();
    let mut data = format!("P6\n{cw} {ch}\n255\n").into_bytes();
    data.extend_from_slice(&out);
    std::fs::write(path, data).unwrap();
}
