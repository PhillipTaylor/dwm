// Internal status bar. Rendered via fontdue + tiny-skia into an ARGB8888
// pixel buffer that is then uploaded to the GLES2 renderer as a texture
// per output.

use std::error::Error;
use std::fs;

use fontdue::{Font, FontSettings, Metrics};

use crate::config::{
    FONT_PATHS, FONT_PX, NORM_BG_COLOR, NORM_FG_COLOR, SEL_BG_COLOR, SEL_FG_COLOR, TAGS,
};

pub const BAR_HEIGHT: i32 = (FONT_PX as i32) + 2;

/// Cached font face used by the bar.
pub struct Bar {
    pub font: Option<Font>,
    pub px: f32,
    pub ascent: i32,
    pub line_height: i32,
}

impl Default for Bar {
    fn default() -> Self {
        Self::new()
    }
}

impl Bar {
    pub fn new() -> Self {
        let mut bar = Self { font: None, px: FONT_PX, ascent: FONT_PX as i32, line_height: BAR_HEIGHT };
        for path in FONT_PATHS {
            if let Ok(bytes) = fs::read(path) {
                if let Ok(f) = Font::from_bytes(bytes, FontSettings::default()) {
                    if let Some(line) = f.horizontal_line_metrics(FONT_PX) {
                        bar.ascent = line.ascent.ceil() as i32;
                        bar.line_height = line.new_line_size.ceil() as i32;
                    }
                    bar.font = Some(f);
                    break;
                }
            }
        }
        bar
    }

    pub fn measure(&self, s: &str) -> i32 {
        let Some(font) = self.font.as_ref() else { return 0; };
        let mut w = 0.0f32;
        for c in s.chars() {
            let m: Metrics = font.metrics(c, self.px);
            w += m.advance_width;
        }
        w.ceil() as i32
    }

    /// Render the bar contents into an ARGB8888 buffer.
    pub fn render(&self, w: i32, sel_tags: u32, urg: u32, ltsymbol: &str,
        title: &str, stext: &str, selected: bool) -> Vec<u8>
    {
        let h = BAR_HEIGHT;
        let stride = (w * 4) as usize;
        let mut buf = vec![0u8; stride * h as usize];

        fill_rect(&mut buf, stride, w, h, 0, 0, w, h, NORM_BG_COLOR);

        let pad = self.px as i32;
        let text_y = (h - self.line_height) / 2;
        let mut x = 0i32;

        for (i, tag) in TAGS.iter().enumerate() {
            let mask = 1u32 << i;
            let is_sel = sel_tags & mask != 0;
            let tag_w = self.measure(tag) + pad;
            let (bg, fg) = if is_sel { (SEL_BG_COLOR, SEL_FG_COLOR) } else { (NORM_BG_COLOR, NORM_FG_COLOR) };
            fill_rect(&mut buf, stride, w, h, x, 0, tag_w, h, bg);
            self.draw_text(&mut buf, stride, w, h, x + pad / 2, text_y, tag, fg);
            if urg & mask != 0 {
                fill_rect(&mut buf, stride, w, h, x, 0, 2, h, SEL_FG_COLOR);
            }
            x += tag_w;
        }

        // Layout symbol.
        let lt_w = self.measure(ltsymbol) + pad;
        self.draw_text(&mut buf, stride, w, h, x + pad / 2, text_y, ltsymbol, NORM_FG_COLOR);
        x += lt_w;

        // Status text on the right.
        let st_w = self.measure(stext) + pad;
        let st_x = (w - st_w).max(x);
        self.draw_text(&mut buf, stride, w, h, st_x + pad / 2, text_y, stext, NORM_FG_COLOR);

        // Title in the middle, only if this monitor is selected.
        if selected {
            let title_w = (st_x - x).max(0);
            if title_w > 0 {
                fill_rect(&mut buf, stride, w, h, x, 0, title_w, h, SEL_BG_COLOR);
                self.draw_text(&mut buf, stride, w, h, x + pad / 2, text_y, title, SEL_FG_COLOR);
            }
        }

        buf
    }

    fn draw_text(&self, buf: &mut [u8], stride: usize, bw: i32, bh: i32,
        x: i32, y: i32, s: &str, color: u32)
    {
        let Some(font) = self.font.as_ref() else { return; };
        let mut pen_x = x as f32;
        let baseline = y + self.ascent;
        for c in s.chars() {
            let (m, bitmap) = font.rasterize(c, self.px);
            let gx = pen_x.floor() as i32 + m.xmin;
            let gy = baseline - m.height as i32 - m.ymin;
            blend_glyph(buf, stride, bw, bh, gx, gy, m.width, m.height, &bitmap, color);
            pen_x += m.advance_width;
        }
    }
}

fn fill_rect(buf: &mut [u8], stride: usize, w: i32, h: i32,
    rx: i32, ry: i32, rw: i32, rh: i32, color: u32)
{
    let x0 = rx.max(0); let y0 = ry.max(0);
    let x1 = (rx + rw).min(w); let y1 = (ry + rh).min(h);
    let bytes = color.to_le_bytes();
    for y in y0..y1 {
        for x in x0..x1 {
            let off = y as usize * stride + x as usize * 4;
            if off + 4 <= buf.len() { buf[off..off + 4].copy_from_slice(&bytes); }
        }
    }
}

fn blend_glyph(buf: &mut [u8], stride: usize, bw: i32, bh: i32,
    px: i32, py: i32, gw: usize, gh: usize, bitmap: &[u8], color: u32)
{
    let cr = ((color >> 16) & 0xff) as u32;
    let cg = ((color >> 8) & 0xff) as u32;
    let cb = (color & 0xff) as u32;
    for gy in 0..gh {
        for gx in 0..gw {
            let a = bitmap[gy * gw + gx] as u32;
            if a == 0 { continue; }
            let x = px + gx as i32; let y = py + gy as i32;
            if x < 0 || y < 0 || x >= bw || y >= bh { continue; }
            let off = y as usize * stride + x as usize * 4;
            if off + 4 > buf.len() { continue; }
            let dst_b = buf[off] as u32; let dst_g = buf[off + 1] as u32;
            let dst_r = buf[off + 2] as u32;
            let inv = 255 - a;
            buf[off]     = ((cb * a + dst_b * inv) / 255) as u8;
            buf[off + 1] = ((cg * a + dst_g * inv) / 255) as u8;
            buf[off + 2] = ((cr * a + dst_r * inv) / 255) as u8;
            buf[off + 3] = 0xff;
        }
    }
}
