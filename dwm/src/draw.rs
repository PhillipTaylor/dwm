// Drawing primitives for dwm's bar (Rust port).

use std::error::Error;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

pub const COL_BORDER: usize = 0;
pub const COL_FG: usize = 1;
pub const COL_BG: usize = 2;
pub const COL_LAST: usize = 3;

pub struct Font {
    pub xfont: Fontable,
    pub ascent: i32,
    pub descent: i32,
    pub height: i32,
    pub min_byte2: usize,
    pub max_byte2: usize,
    pub char_widths: Vec<i32>,
    pub default_width: i32,
}

impl Font {
    fn measure(&self, c: u8) -> i32 {
        let cu = c as usize;
        if cu >= self.min_byte2 && cu <= self.max_byte2 {
            let w = self.char_widths[cu - self.min_byte2];
            if w != 0 { w } else { self.default_width }
        } else {
            self.default_width
        }
    }
}

pub struct Dc {
    pub gc: Gcontext,
    pub drawable: Pixmap,
    pub drawable_w: u16,
    pub drawable_h: u16,
    pub font: Font,
    pub norm: [u32; COL_LAST],
    pub sel: [u32; COL_LAST],
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub fn load_font(conn: &RustConnection, fontstr: &str) -> Result<Font, Box<dyn Error>> {
    let try_open = |name: &str| -> Result<Font, Box<dyn Error>> {
        let fid = conn.generate_id()?;
        conn.open_font(fid, name.as_bytes())?.check()?;
        let qf = conn.query_font(fid)?.reply()?;
        let widths: Vec<i32> = qf.char_infos.iter()
            .map(|ci| ci.character_width as i32).collect();
        let default_width = if qf.max_bounds.character_width != 0 {
            qf.max_bounds.character_width as i32
        } else { qf.min_bounds.character_width as i32 };
        Ok(Font {
            xfont: fid,
            ascent: qf.font_ascent as i32,
            descent: qf.font_descent as i32,
            height: qf.font_ascent as i32 + qf.font_descent as i32,
            min_byte2: qf.min_char_or_byte2 as usize,
            max_byte2: qf.max_char_or_byte2 as usize,
            char_widths: widths,
            default_width,
        })
    };
    match try_open(fontstr) {
        Ok(f) => Ok(f),
        Err(_) => {
            eprintln!("dwm: cannot load font '{}', falling back to 'fixed'", fontstr);
            try_open("fixed")
        }
    }
}

pub fn get_color(conn: &RustConnection, screen_num: usize, colstr: &str)
    -> Result<u32, Box<dyn Error>>
{
    let cmap = conn.setup().roots[screen_num].default_colormap;
    if let Some(rest) = colstr.strip_prefix('#') {
        if rest.len() == 6 {
            let r = u16::from_str_radix(&rest[0..2], 16)? * 0x101;
            let g = u16::from_str_radix(&rest[2..4], 16)? * 0x101;
            let b = u16::from_str_radix(&rest[4..6], 16)? * 0x101;
            let reply = conn.alloc_color(cmap, r, g, b)?.reply()
                .map_err(|e| format!("dwm: cannot allocate color '{}': {:?}", colstr, e))?;
            return Ok(reply.pixel);
        }
    }
    let r = conn.alloc_named_color(cmap, colstr.as_bytes())?
        .reply()
        .map_err(|e| format!("dwm: cannot allocate color '{}': {:?}", colstr, e))?;
    Ok(r.pixel)
}

pub fn textnw(dc: &Dc, text: &[u8], len: usize) -> i32 {
    let n = len.min(text.len());
    let mut w = 0i32;
    for &c in &text[..n] {
        w += dc.font.measure(c);
    }
    w
}

pub fn textw(dc: &Dc, text: &[u8]) -> i32 {
    textnw(dc, text, text.len()) + dc.font.height
}

pub fn drawtext(conn: &RustConnection, dc: &Dc, text: Option<&[u8]>,
                col: &[u32; COL_LAST], invert: bool) -> Result<(), Box<dyn Error>>
{
    // Background fill
    conn.change_gc(dc.gc, &ChangeGCAux::default()
        .foreground(col[if invert { COL_FG } else { COL_BG }]))?;
    let r = Rectangle { x: dc.x as i16, y: dc.y as i16,
        width: dc.w as u16, height: dc.h as u16 };
    conn.poly_fill_rectangle(dc.drawable, dc.gc, &[r])?;
    let text = match text { Some(t) => t, None => return Ok(()) };
    let olen = text.len();
    let h = dc.font.ascent + dc.font.descent;
    let y = dc.y + (dc.h / 2) - (h / 2) + dc.font.ascent;
    let x = dc.x + (h / 2);
    let mut len = olen.min(256);
    while len > 0 && textnw(dc, text, len) > dc.w - h {
        len -= 1;
    }
    if len == 0 { return Ok(()); }
    let mut buf: Vec<u8> = text[..len].to_vec();
    if len < olen {
        let start = if len >= 3 { len - 3 } else { 0 };
        for b in &mut buf[start..len] { *b = b'.'; }
    }
    conn.change_gc(dc.gc, &ChangeGCAux::default()
        .foreground(col[if invert { COL_BG } else { COL_FG }])
        .background(col[if invert { COL_FG } else { COL_BG }])
        .font(dc.font.xfont))?;
    conn.image_text8(dc.drawable, dc.gc, x as i16, y as i16, &buf[..len.min(255)])?;
    Ok(())
}

pub fn drawsquare(conn: &RustConnection, dc: &Dc, filled: bool, empty: bool,
                  invert: bool, col: &[u32; COL_LAST]) -> Result<(), Box<dyn Error>>
{
    conn.change_gc(dc.gc, &ChangeGCAux::default()
        .foreground(col[if invert { COL_BG } else { COL_FG }]))?;
    let s = (dc.font.ascent + dc.font.descent + 2) / 4;
    let rx = (dc.x + 1) as i16;
    let ry = (dc.y + 1) as i16;
    if filled {
        let r = Rectangle { x: rx, y: ry, width: (s + 1) as u16, height: (s + 1) as u16 };
        conn.poly_fill_rectangle(dc.drawable, dc.gc, &[r])?;
    } else if empty {
        let r = Rectangle { x: rx, y: ry, width: s as u16, height: s as u16 };
        conn.poly_rectangle(dc.drawable, dc.gc, &[r])?;
    }
    Ok(())
}
