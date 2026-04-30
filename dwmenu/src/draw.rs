use std::error::Error;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub const COL_BG: usize = 0;
pub const COL_FG: usize = 1;
pub const COL_BORDER: usize = 2;
pub const COL_LAST: usize = 3;

pub const DEFAULT_FONT: &str = "fixed";

pub struct Font {
    pub xfont: Fontable,
    pub ascent: i32,
    pub descent: i32,
    pub height: i32,
    pub min_byte2: usize,
    pub max_byte2: usize,
    pub default_char: usize,
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
    pub conn: RustConnection,
    pub screen_num: usize,
    pub root: Window,
    pub gc: Gcontext,
    pub canvas: Pixmap,
    pub canvas_w: u16,
    pub canvas_h: u16,
    pub font: Font,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub fn init_dc() -> Result<Dc, Box<dyn Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let gc = conn.generate_id()?;
    conn.create_gc(gc, root, &CreateGCAux::default()
        .line_style(LineStyle::SOLID)
        .cap_style(CapStyle::BUTT)
        .join_style(JoinStyle::MITER))?;
    let font = load_font(&conn, None)?;
    Ok(Dc {
        conn, screen_num, root, gc,
        canvas: 0, canvas_w: 0, canvas_h: 0,
        font, x: 0, y: 0, w: 0, h: 0,
    })
}

pub fn init_font(dc: &mut Dc, fontstr: Option<&str>) -> Result<(), Box<dyn Error>> {
    match load_font(&dc.conn, fontstr) {
        Ok(f) => { dc.font = f; Ok(()) }
        Err(_) => {
            if let Some(s) = fontstr {
                eprintln!("cannot load font '{}'", s);
            }
            dc.font = load_font(&dc.conn, None)?;
            Ok(())
        }
    }
}

fn load_font(conn: &RustConnection, fontstr: Option<&str>) -> Result<Font, Box<dyn Error>> {
    let name = fontstr.unwrap_or(DEFAULT_FONT);
    if name.is_empty() {
        return Err("empty font name".into());
    }
    let fid = conn.generate_id()?;
    conn.open_font(fid, name.as_bytes())?.check()?;
    let qf = conn.query_font(fid)?.reply()?;
    let min = qf.min_char_or_byte2 as usize;
    let max = qf.max_char_or_byte2 as usize;
    let widths: Vec<i32> = qf.char_infos.iter()
        .map(|ci| ci.character_width as i32).collect();
    let default_width = if qf.max_bounds.character_width != 0 {
        qf.max_bounds.character_width as i32
    } else {
        qf.min_bounds.character_width as i32
    };
    Ok(Font {
        xfont: fid,
        ascent: qf.font_ascent as i32,
        descent: qf.font_descent as i32,
        height: qf.font_ascent as i32 + qf.font_descent as i32,
        min_byte2: min,
        max_byte2: max,
        default_char: qf.default_char as usize,
        char_widths: widths,
        default_width,
    })
}

pub fn get_color(dc: &Dc, colstr: &str) -> Result<u32, Box<dyn Error>> {
    let screen = &dc.conn.setup().roots[dc.screen_num];
    let cmap = screen.default_colormap;
    if let Some(rest) = colstr.strip_prefix('#') {
        if rest.len() == 6 {
            let r = u16::from_str_radix(&rest[0..2], 16)? * 0x101;
            let g = u16::from_str_radix(&rest[2..4], 16)? * 0x101;
            let b = u16::from_str_radix(&rest[4..6], 16)? * 0x101;
            let reply = dc.conn.alloc_color(cmap, r, g, b)?.reply()
                .map_err(|e| format!("cannot allocate color '{}': {:?}", colstr, e))?;
            return Ok(reply.pixel);
        }
    }
    let r = dc.conn.alloc_named_color(cmap, colstr.as_bytes())?.reply()
        .map_err(|e| format!("cannot allocate color '{}': {:?}", colstr, e))?;
    Ok(r.pixel)
}

pub fn resize_dc(dc: &mut Dc, w: u16, h: u16) -> Result<(), Box<dyn Error>> {
    if dc.canvas != 0 {
        dc.conn.free_pixmap(dc.canvas)?;
    }
    let pid = dc.conn.generate_id()?;
    let depth = dc.conn.setup().roots[dc.screen_num].root_depth;
    dc.conn.create_pixmap(depth, pid, dc.root, w.max(1), h.max(1))?;
    dc.canvas = pid;
    dc.canvas_w = w;
    dc.canvas_h = h;
    dc.w = w as i32;
    dc.h = h as i32;
    Ok(())
}

pub fn map_dc(dc: &Dc, win: Window, w: u16, h: u16) -> Result<(), Box<dyn Error>> {
    dc.conn.copy_area(dc.canvas, win, dc.gc, 0, 0, 0, 0, w, h)?;
    Ok(())
}

pub fn draw_rect(dc: &Dc, x: i32, y: i32, w: u32, h: u32, fill: bool, color: u32)
    -> Result<(), Box<dyn Error>>
{
    dc.conn.change_gc(dc.gc, &ChangeGCAux::default().foreground(color))?;
    let rx = (dc.x + x) as i16;
    let ry = (dc.y + y) as i16;
    if fill {
        let r = Rectangle { x: rx, y: ry, width: w as u16, height: h as u16 };
        dc.conn.poly_fill_rectangle(dc.canvas, dc.gc, &[r])?;
    } else {
        let r = Rectangle { x: rx, y: ry, width: (w - 1) as u16, height: (h - 1) as u16 };
        dc.conn.poly_rectangle(dc.canvas, dc.gc, &[r])?;
    }
    Ok(())
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

pub fn draw_text(dc: &Dc, text: &[u8], col: &[u32; COL_LAST]) -> Result<(), Box<dyn Error>> {
    let mut buf: Vec<u8> = Vec::new();
    let n = text.len();
    let mut mn = n.min(255);
    while mn > 0 && textnw(dc, text, mn) + dc.font.height / 2 > dc.w {
        mn -= 1;
    }
    if mn == 0 {
        draw_rect(dc, 0, 0, dc.w as u32, dc.h as u32, true, col[COL_BG])?;
        return Ok(());
    }
    buf.extend_from_slice(&text[..mn]);
    if mn < n {
        let start = mn.saturating_sub(3);
        for b in buf.iter_mut().skip(start) { *b = b'.'; }
    }
    draw_rect(dc, 0, 0, dc.w as u32, dc.h as u32, true, col[COL_BG])?;
    draw_textn(dc, &buf, col)?;
    Ok(())
}

pub fn draw_textn(dc: &Dc, text: &[u8], col: &[u32; COL_LAST]) -> Result<(), Box<dyn Error>> {
    let x = (dc.x + dc.font.height / 2) as i16;
    let y = (dc.y + dc.font.ascent + 1) as i16;
    dc.conn.change_gc(dc.gc, &ChangeGCAux::default()
        .foreground(col[COL_FG])
        .background(col[COL_BG])
        .font(dc.font.xfont))?;
    let n = text.len().min(255);
    if n == 0 {
        return Ok(());
    }
    dc.conn.image_text8(dc.canvas, dc.gc, x, y, &text[..n])?;
    Ok(())
}

pub fn free_dc(dc: &Dc) -> Result<(), Box<dyn Error>> {
    if dc.canvas != 0 {
        dc.conn.free_pixmap(dc.canvas)?;
    }
    dc.conn.close_font(dc.font.xfont)?;
    dc.conn.free_gc(dc.gc)?;
    Ok(())
}
