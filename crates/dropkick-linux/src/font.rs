//! Glyph rasterisation and a 32-bit ARGB canvas.
//!
//! Deliberately small: a cached glyph rasteriser over one monospace face. No
//! shaping engine, no font database, no fallback chain. When Dropkick needs
//! real text layout it can adopt one, but not before it needs it.

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

/// Candidate faces, in preference order. Absolute paths rather than a font
/// database: fewer moving parts, and the failure is loud instead of subtle.
const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
];

pub struct TextRenderer {
    font: Font,
    px: f32,
    cache: HashMap<char, (Metrics, Vec<u8>)>,
    advance: f32,
}

impl TextRenderer {
    pub fn new(px: f32) -> Result<Self, String> {
        for path in CANDIDATES {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let font = Font::from_bytes(bytes, FontSettings::default())
                .map_err(|e| format!("{path}: {e}"))?;
            let advance = font.metrics('M', px).advance_width;
            return Ok(TextRenderer { font, px, cache: HashMap::new(), advance });
        }
        Err(format!("no usable monospace font found in {CANDIDATES:?}"))
    }

    /// Cell width. The face is monospace, so this is the same for every glyph.
    pub fn advance(&self) -> f32 {
        self.advance
    }

    pub fn line_height(&self) -> f32 {
        (self.px * 1.45).round()
    }

    pub fn width_of(&self, text: &str) -> f32 {
        text.chars().count() as f32 * self.advance
    }

    fn glyph(&mut self, c: char) -> &(Metrics, Vec<u8>) {
        let px = self.px;
        self.cache.entry(c).or_insert_with(|| self.font.rasterize(c, px))
    }

    /// Draw `text` with its baseline at `y`. Returns the x cursor after it.
    pub fn draw(&mut self, canvas: &mut Canvas, x: f32, y: f32, text: &str, color: u32) -> f32 {
        let mut pen = x;
        for c in text.chars() {
            if c != ' ' {
                let (m, bitmap) = self.glyph(c);
                let (gw, gh) = (m.width as i32, m.height as i32);
                let gx = pen as i32 + m.xmin;
                let gy = y as i32 - (gh + m.ymin);
                // Copy out of the cache borrow before touching the canvas.
                let (gw, gh, gx, gy) = (gw, gh, gx, gy);
                let bitmap: &[u8] = bitmap;
                for row in 0..gh {
                    for col in 0..gw {
                        let a = bitmap[(row * gw + col) as usize];
                        if a > 0 {
                            canvas.blend(gx + col, gy + row, color, a);
                        }
                    }
                }
            }
            pen += self.advance;
        }
        pen
    }
}

/// ARGB8888, little-endian in memory: [B, G, R, A].
pub struct Canvas<'a> {
    pub buf: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

impl Canvas<'_> {
    pub fn fill(&mut self, color: u32) {
        let px = (0xFF00_0000u32 | color).to_le_bytes();
        for chunk in self.buf.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        for row in y.max(0)..(y + h).min(self.h) {
            for col in x.max(0)..(x + w).min(self.w) {
                self.set(col, row, color);
            }
        }
    }

    /// Read a pixel back as `0xRRGGBB`, dropping alpha.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return 0;
        }
        let i = ((y * self.w + x) * 4) as usize;
        u32::from_le_bytes([self.buf[i], self.buf[i + 1], self.buf[i + 2], self.buf[i + 3]])
            & 0x00FF_FFFF
    }

    /// Write a pixel with a chosen alpha.
    ///
    /// **Wayland's `ARGB8888` is premultiplied**, so the colour components are
    /// scaled by the alpha here rather than being passed through. Writing
    /// straight (unpremultiplied) values produces a picture that is visibly too
    /// bright wherever it is translucent, which is the sort of thing that gets
    /// blamed on the compositor.
    #[inline]
    pub fn put_alpha(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let a = alpha as u32;
        let scale = |c: u32| ((c * a + 127) / 255) & 0xFF;
        let out = (a << 24)
            | (scale((color >> 16) & 0xFF) << 16)
            | (scale((color >> 8) & 0xFF) << 8)
            | scale(color & 0xFF);
        let i = ((y * self.w + x) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&out.to_le_bytes());
    }

    #[inline]
    fn set(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        self.buf[i..i + 4].copy_from_slice(&(0xFF00_0000u32 | color).to_le_bytes());
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        if alpha == 255 {
            self.set(x, y, color);
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let a = alpha as u32;
        let inv = 255 - a;
        let (sr, sg, sb) = ((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF);
        let db = self.buf[i] as u32;
        let dg = self.buf[i + 1] as u32;
        let dr = self.buf[i + 2] as u32;
        self.buf[i] = ((sb * a + db * inv) / 255) as u8;
        self.buf[i + 1] = ((sg * a + dg * inv) / 255) as u8;
        self.buf[i + 2] = ((sr * a + dr * inv) / 255) as u8;
        self.buf[i + 3] = 0xFF;
    }

    /// Dump as binary PPM so a frame can be inspected without a compositor.
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.w, self.h).into_bytes();
        for chunk in self.buf.chunks_exact(4) {
            out.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
        }
        out
    }
}
