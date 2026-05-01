// Software rendering: build an ARGB8888 buffer with text and rectangles,
// blit through wl_shm via SCTK's SlotPool.

use std::error::Error;
use std::fs;

use fontdue::{Font, FontSettings, Metrics};

pub struct FontFace {
    pub font: Font,
    pub px: f32,
    pub ascent: i32,
    pub descent: i32,
    pub line_height: i32,
}

const DEFAULT_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
];

pub fn load_font(path_opt: Option<&str>, px: f32) -> Result<FontFace, Box<dyn Error>> {
    let bytes = if let Some(p) = path_opt {
        fs::read(p).map_err(|e| format!("cannot read font {}: {}", p, e))?
    } else {
        let mut found = None;
        for p in DEFAULT_FONT_PATHS {
            if let Ok(b) = fs::read(p) { found = Some(b); break; }
        }
        found.ok_or_else(|| "no default TTF font found; pass -fn /path/to/font.ttf".to_string())?
    };
    let font = Font::from_bytes(bytes, FontSettings::default())
        .map_err(|e| format!("font parse error: {:?}", e))?;
    let line = font.horizontal_line_metrics(px)
        .ok_or_else(|| "font has no horizontal metrics".to_string())?;
    Ok(FontFace {
        ascent: line.ascent.ceil() as i32,
        descent: line.descent.floor() as i32,
        line_height: line.new_line_size.ceil() as i32,
        font,
        px,
    })
}

impl FontFace {
    pub fn measure(&self, s: &[u8]) -> i32 {
        let text = std::str::from_utf8(s).unwrap_or("");
        let mut w = 0.0f32;
        for c in text.chars() {
            let m: Metrics = self.font.metrics(c, self.px);
            w += m.advance_width;
        }
        w.ceil() as i32
    }

    pub fn height(&self) -> i32 { self.line_height.max(self.ascent - self.descent) }
}

fn argb_at(buf: &mut [u8], stride: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    if x < 0 || y < 0 || x >= w || y >= h { return; }
    let off = (y as usize) * stride + (x as usize) * 4;
    if off + 4 > buf.len() { return; }
    buf[off..off + 4].copy_from_slice(&color.to_le_bytes());
}

fn fill_rect(buf: &mut [u8], stride: usize, w: i32, h: i32,
    rx: i32, ry: i32, rw: i32, rh: i32, color: u32)
{
    let x0 = rx.max(0);
    let y0 = ry.max(0);
    let x1 = (rx + rw).min(w);
    let y1 = (ry + rh).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            argb_at(buf, stride, x, y, w, h, color);
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
            let x = px + gx as i32;
            let y = py + gy as i32;
            if x < 0 || y < 0 || x >= bw || y >= bh { continue; }
            let off = (y as usize) * stride + (x as usize) * 4;
            if off + 4 > buf.len() { continue; }
            let dst_b = buf[off]     as u32;
            let dst_g = buf[off + 1] as u32;
            let dst_r = buf[off + 2] as u32;
            let inv = 255 - a;
            let nb = (cb * a + dst_b * inv) / 255;
            let ng = (cg * a + dst_g * inv) / 255;
            let nr = (cr * a + dst_r * inv) / 255;
            buf[off]     = nb as u8;
            buf[off + 1] = ng as u8;
            buf[off + 2] = nr as u8;
            buf[off + 3] = 0xff;
        }
    }
}

fn draw_text_run(buf: &mut [u8], stride: usize, bw: i32, bh: i32,
    font: &FontFace, x: i32, y: i32, s: &[u8], color: u32) -> i32
{
    let text = std::str::from_utf8(s).unwrap_or("");
    let mut pen_x = x as f32;
    let baseline = y + font.ascent;
    for c in text.chars() {
        let (m, bitmap) = font.font.rasterize(c, font.px);
        let gx = pen_x.floor() as i32 + m.xmin;
        let gy = baseline - m.height as i32 - m.ymin;
        blend_glyph(buf, stride, bw, bh, gx, gy, m.width, m.height, &bitmap, color);
        pen_x += m.advance_width;
    }
    pen_x.ceil() as i32
}

pub fn paint_rect(buf: &mut [u8], stride: usize, w: i32, h: i32,
    x: i32, y: i32, rw: i32, rh: i32, color: u32)
{
    fill_rect(buf, stride, w, h, x, y, rw, rh, color);
}

pub fn paint_text(buf: &mut [u8], stride: usize, w: i32, h: i32,
    font: &FontFace, x: i32, y: i32, s: &[u8], color: u32) -> i32
{
    draw_text_run(buf, stride, w, h, font, x, y, s, color)
}
