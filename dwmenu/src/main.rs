// dmenu - dynamic menu (Rust port of suckless dmenu 4.3.1)

#![allow(non_upper_case_globals)]

mod draw;
mod keysyms;

use std::error::Error;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xinerama::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::CURRENT_TIME;

use draw::*;

const VERSION: &str = "4.3.1";

#[derive(Default)]
struct Item {
    text: Vec<u8>,
    left: Option<usize>,
    right: Option<usize>,
}

struct Args {
    fast: bool,
    case_insensitive: bool,
    topbar: bool,
    lines: i32,
    prompt: Option<String>,
    font: Option<String>,
    normbg: String,
    normfg: String,
    selbg: String,
    selfg: String,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            fast: false,
            case_insensitive: false,
            topbar: true,
            lines: 0,
            prompt: None,
            font: None,
            normbg: "#cccccc".into(),
            normfg: "#000000".into(),
            selbg:  "#0066ff".into(),
            selfg:  "#ffffff".into(),
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: dmenu [-b] [-f] [-i] [-l lines] [-p prompt] [-fn font]");
    eprintln!("             [-nb color] [-nf color] [-sb color] [-sf color] [-v]");
    std::process::exit(1);
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        let s = &argv[i];
        if s == "-v" {
            println!("dmenu-{}, Rust port of suckless dmenu", VERSION);
            std::process::exit(0);
        } else if s == "-b" {
            a.topbar = false;
        } else if s == "-f" {
            a.fast = true;
        } else if s == "-i" {
            a.case_insensitive = true;
        } else if i + 1 == argv.len() {
            usage();
        } else if s == "-l" {
            i += 1;
            a.lines = argv[i].parse().unwrap_or(0);
        } else if s == "-p" {
            i += 1;
            a.prompt = Some(argv[i].clone());
        } else if s == "-fn" {
            i += 1;
            a.font = Some(argv[i].clone());
        } else if s == "-nb" {
            i += 1; a.normbg = argv[i].clone();
        } else if s == "-nf" {
            i += 1; a.normfg = argv[i].clone();
        } else if s == "-sb" {
            i += 1; a.selbg = argv[i].clone();
        } else if s == "-sf" {
            i += 1; a.selfg = argv[i].clone();
        } else {
            usage();
        }
        i += 1;
    }
    a
}

struct Menu {
    dc: Dc,
    win: Window,
    utf8: Atom,
    items: Vec<Item>,
    matches: Option<usize>,
    matchend: Option<usize>,
    prev: Option<usize>,
    curr: Option<usize>,
    next: Option<usize>,
    sel: Option<usize>,
    text: Vec<u8>,
    cursor: usize,
    bh: i32,
    mw: i32,
    mh: i32,
    inputw: i32,
    promptw: i32,
    lines: i32,
    prompt: Option<Vec<u8>>,
    case_insensitive: bool,
    topbar: bool,
    normcol: [u32; COL_LAST],
    selcol: [u32; COL_LAST],
    keysym_table: Vec<u32>,
    min_keycode: u8,
    keysyms_per_keycode: u8,
}

fn main() -> ExitCode {
    match run_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("dmenu: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run_main() -> Result<ExitCode, Box<dyn Error>> {
    let args = parse_args();
    let mut dc = init_dc()?;
    init_font(&mut dc, args.font.as_deref())?;

    let mut menu = Menu {
        dc, win: 0, utf8: 0,
        items: Vec::new(),
        matches: None, matchend: None,
        prev: None, curr: None, next: None, sel: None,
        text: Vec::new(), cursor: 0,
        bh: 0, mw: 0, mh: 0, inputw: 0, promptw: 0,
        lines: args.lines.max(0),
        prompt: args.prompt.as_ref().map(|s| s.as_bytes().to_vec()),
        case_insensitive: args.case_insensitive,
        topbar: args.topbar,
        normcol: [0; COL_LAST],
        selcol: [0; COL_LAST],
        keysym_table: Vec::new(),
        min_keycode: 0,
        keysyms_per_keycode: 0,
    };

    if args.fast {
        grab_keyboard(&menu.dc)?;
        read_stdin(&mut menu)?;
    } else {
        read_stdin(&mut menu)?;
        grab_keyboard(&menu.dc)?;
    }
    setup(&mut menu, &args)?;
    let code = run(&mut menu)?;
    Ok(code)
}


fn read_stdin(menu: &mut Menu) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut max_w = 0i32;
    for line in stdin.lock().lines() {
        let line = line?;
        let bytes = line.into_bytes();
        let w = textw(&menu.dc, &bytes);
        if w > max_w {
            max_w = w;
        }
        menu.items.push(Item { text: bytes, left: None, right: None });
    }
    menu.inputw = max_w;
    Ok(())
}

fn grab_keyboard(dc: &Dc) -> Result<(), Box<dyn Error>> {
    for _ in 0..1000 {
        let r = dc.conn.grab_keyboard(true, dc.root, CURRENT_TIME,
            GrabMode::ASYNC, GrabMode::ASYNC)?.reply()?;
        if r.status == GrabStatus::SUCCESS {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_micros(1000));
    }
    Err("cannot grab keyboard".into())
}

fn load_keymap(menu: &mut Menu) -> Result<(), Box<dyn Error>> {
    let setup = menu.dc.conn.setup();
    menu.min_keycode = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max - menu.min_keycode + 1;
    let r = menu.dc.conn.get_keyboard_mapping(menu.min_keycode, count)?.reply()?;
    menu.keysyms_per_keycode = r.keysyms_per_keycode;
    menu.keysym_table = r.keysyms;
    Ok(())
}

fn keysym_for(menu: &Menu, keycode: u8, state: u16) -> u32 {
    if keycode < menu.min_keycode {
        return 0;
    }
    let off = (keycode - menu.min_keycode) as usize * menu.keysyms_per_keycode as usize;
    let kpsym = menu.keysyms_per_keycode as usize;
    if off >= menu.keysym_table.len() {
        return 0;
    }
    let group = &menu.keysym_table[off..off + kpsym.min(menu.keysym_table.len() - off)];
    let shift = state & u16::from(KeyButMask::SHIFT) != 0;
    let lock  = state & u16::from(KeyButMask::LOCK)  != 0;
    let unshifted = *group.first().unwrap_or(&0);
    let shifted   = group.get(1).copied().unwrap_or(0);
    let mut k = if shift { if shifted != 0 { shifted } else { unshifted } } else { unshifted };
    if lock {
        // Caps Lock: invert case for letters
        if (0x0061..=0x007a).contains(&k) { k -= 0x20; }
        else if (0x0041..=0x005a).contains(&k) && !shift { k += 0x20; }
    }
    k
}

fn keysym_to_bytes(ks: u32) -> Option<Vec<u8>> {
    match ks {
        0x20..=0x7e => Some(vec![ks as u8]),
        0xa0..=0xff => Some(vec![ks as u8]),
        _ => None,
    }
}

fn setup(menu: &mut Menu, args: &Args) -> Result<(), Box<dyn Error>> {
    load_keymap(menu)?;

    menu.normcol[COL_BG] = get_color(&menu.dc, &args.normbg)?;
    menu.normcol[COL_FG] = get_color(&menu.dc, &args.normfg)?;
    menu.selcol[COL_BG]  = get_color(&menu.dc, &args.selbg)?;
    menu.selcol[COL_FG]  = get_color(&menu.dc, &args.selfg)?;

    menu.utf8 = menu.dc.conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

    menu.bh = menu.dc.font.height + 2;
    menu.mh = (menu.lines + 1) * menu.bh;

    let setup_x = &menu.dc.conn.setup().roots[menu.dc.screen_num];
    let root = setup_x.root;
    let screen_w = setup_x.width_in_pixels as i32;
    let screen_h = setup_x.height_in_pixels as i32;
    let depth = setup_x.root_depth;
    let visual = setup_x.root_visual;

    let mut x: i32 = 0;
    let mut y: i32 = if menu.topbar { 0 } else { screen_h - menu.mh };
    let mut w: i32 = screen_w;
    let xinerama_active = menu.dc.conn
        .xinerama_is_active()
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.state)
        .unwrap_or(0) != 0;
    if xinerama_active {
        if let Ok(scr) = menu.dc.conn.xinerama_query_screens()?.reply() {
            let info = scr.screen_info;
            if !info.is_empty() {
                let q = menu.dc.conn.query_pointer(root)?.reply()?;
                let mut idx = info.len() - 1;
                for (i, s) in info.iter().enumerate() {
                    let rx = s.x_org as i32;
                    let ry = s.y_org as i32;
                    let rw = s.width as i32;
                    let rh = s.height as i32;
                    if (q.root_x as i32) >= rx && (q.root_x as i32) < rx + rw
                        && (q.root_y as i32) >= ry && (q.root_y as i32) < ry + rh {
                        idx = i;
                        break;
                    }
                }
                let s = &info[idx];
                x = s.x_org as i32;
                y = s.y_org as i32 + if menu.topbar { 0 } else { s.height as i32 - menu.mh };
                w = s.width as i32;
            }
        }
    }
    finish_geom(menu, x, y, w, depth, visual, root)?;
    Ok(())
}

fn finish_geom(menu: &mut Menu, x: i32, y: i32, w: i32, depth: u8, visual: u32, root: Window)
    -> Result<(), Box<dyn Error>>
{
    menu.mw = w;
    menu.promptw = match &menu.prompt {
        Some(p) => textw(&menu.dc, p),
        None => 0,
    };
    menu.inputw = menu.inputw.min(menu.mw / 3);
    do_match(menu, false);

    let win = menu.dc.conn.generate_id()?;
    // background_pixmap = ParentRelative (1)
    let aux = CreateWindowAux::default()
        .override_redirect(1)
        .background_pixmap(1u32)
        .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::VISIBILITY_CHANGE);
    menu.dc.conn.create_window(depth, win, root,
        x as i16, y as i16, w as u16, menu.mh as u16,
        0, WindowClass::COPY_FROM_PARENT, visual, &aux)?;
    menu.win = win;
    menu.dc.conn.map_window(win)?;
    resize_dc(&mut menu.dc, w as u16, menu.mh as u16)?;
    menu.dc.conn.flush()?;
    draw_menu(menu)?;
    Ok(())
}

fn cmp_eq(a: &[u8], b: &[u8], ci: bool) -> bool {
    if a.len() != b.len() { return false; }
    if ci {
        a.iter().zip(b).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
    } else {
        a == b
    }
}

fn starts_with(haystack: &[u8], needle: &[u8], ci: bool) -> bool {
    if haystack.len() < needle.len() { return false; }
    if ci {
        haystack.iter().zip(needle).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
    } else {
        &haystack[..needle.len()] == needle
    }
}

fn contains_sub(haystack: &[u8], needle: &[u8], ci: bool) -> bool {
    if needle.is_empty() { return true; }
    if needle.len() > haystack.len() { return false; }
    for i in 0..=haystack.len() - needle.len() {
        if starts_with(&haystack[i..], needle, ci) {
            return true;
        }
    }
    false
}

fn appenditem(items: &mut [Item], idx: usize,
    list: &mut Option<usize>, last: &mut Option<usize>)
{
    if last.is_none() {
        *list = Some(idx);
    } else if let Some(li) = *last {
        items[li].right = Some(idx);
    }
    items[idx].left = *last;
    items[idx].right = None;
    *last = Some(idx);
}

fn do_match(menu: &mut Menu, sub: bool) {
    let len = menu.text.len();
    let mut lexact: Option<usize> = None;
    let mut lprefix: Option<usize> = None;
    let mut lsubstr: Option<usize> = None;
    let mut exactend: Option<usize> = None;
    let mut prefixend: Option<usize> = None;
    let mut substrend: Option<usize> = None;

    // Build iteration order
    let order: Vec<usize> = if sub {
        let mut v = Vec::new();
        let mut cur = menu.matches;
        while let Some(i) = cur {
            v.push(i);
            cur = menu.items[i].right;
        }
        v
    } else {
        (0..menu.items.len()).collect()
    };

    let _ = len;
    let ci = menu.case_insensitive;
    let needle: Vec<u8> = menu.text.clone();
    for idx in order {
        // clone to avoid borrowing items immutably while we pass &mut items
        let txt = menu.items[idx].text.clone();
        if cmp_eq(&txt, &needle, ci) {
            appenditem(&mut menu.items, idx, &mut lexact, &mut exactend);
        } else if starts_with(&txt, &needle, ci) {
            appenditem(&mut menu.items, idx, &mut lprefix, &mut prefixend);
        } else if contains_sub(&txt, &needle, ci) {
            appenditem(&mut menu.items, idx, &mut lsubstr, &mut substrend);
        }
    }

    menu.matches = lexact;
    menu.matchend = exactend;
    if let Some(p) = lprefix {
        if let Some(me) = menu.matchend {
            menu.items[me].right = Some(p);
            menu.items[p].left = Some(me);
        } else {
            menu.matches = lprefix;
        }
        menu.matchend = prefixend;
    }
    if let Some(s) = lsubstr {
        if let Some(me) = menu.matchend {
            menu.items[me].right = Some(s);
            menu.items[s].left = Some(me);
        } else {
            menu.matches = lsubstr;
        }
        menu.matchend = substrend;
    }
    menu.curr = menu.matches;
    menu.sel = menu.matches;
    calcoffsets(menu);
}

fn calcoffsets(menu: &mut Menu) {
    let n = if menu.lines > 0 {
        menu.lines * menu.bh
    } else {
        let lt = textw(&menu.dc, b"<");
        let gt = textw(&menu.dc, b">");
        menu.mw - (menu.promptw + menu.inputw + lt + gt)
    };
    let mut i: i32 = 0;
    let mut next = menu.curr;
    while let Some(idx) = next {
        let inc = if menu.lines > 0 { menu.bh } else {
            textw(&menu.dc, &menu.items[idx].text).min(n)
        };
        i += inc;
        if i > n { break; }
        next = menu.items[idx].right;
    }
    menu.next = next;

    let mut i: i32 = 0;
    let mut prev = menu.curr;
    while let Some(idx) = prev {
        if let Some(li) = menu.items[idx].left {
            let inc = if menu.lines > 0 { menu.bh } else {
                textw(&menu.dc, &menu.items[li].text).min(n)
            };
            i += inc;
            if i > n { break; }
            prev = Some(li);
        } else {
            break;
        }
    }
    menu.prev = prev;
}

fn draw_menu(menu: &mut Menu) -> Result<(), Box<dyn Error>> {
    menu.dc.x = 0;
    menu.dc.y = 0;
    menu.dc.h = menu.bh;
    let bg = menu.normcol[COL_BG];
    draw_rect(&menu.dc, 0, 0, menu.mw as u32, menu.mh as u32, true, bg)?;

    if let Some(p) = menu.prompt.clone() {
        menu.dc.w = menu.promptw;
        let sel = menu.selcol;
        draw_text(&menu.dc, &p, &sel)?;
        menu.dc.x = menu.dc.w;
    }
    let dx_after_prompt = menu.dc.x;

    let input_w = if menu.lines > 0 || menu.matches.is_none() {
        menu.mw - menu.dc.x
    } else {
        menu.inputw
    };
    menu.dc.w = input_w;
    let normcol = menu.normcol;
    draw_text(&menu.dc, &menu.text.clone(), &normcol)?;

    // Cursor
    let curpos = textnw(&menu.dc, &menu.text, menu.cursor) + menu.dc.font.height / 2 - 2;
    if curpos < menu.dc.w {
        let fg = menu.normcol[COL_FG];
        draw_rect(&menu.dc, curpos, 2, 1, (menu.dc.h - 4) as u32, true, fg)?;
    }

    if menu.lines > 0 {
        menu.dc.w = menu.mw - menu.dc.x;
        let mut item = menu.curr;
        while item != menu.next {
            let idx = match item { Some(i) => i, None => break };
            menu.dc.y += menu.dc.h;
            let col = if Some(idx) == menu.sel { menu.selcol } else { menu.normcol };
            let txt = menu.items[idx].text.clone();
            draw_text(&menu.dc, &txt, &col)?;
            item = menu.items[idx].right;
        }
    } else if menu.matches.is_some() {
        menu.dc.x += menu.inputw;
        let lt_w = textw(&menu.dc, b"<");
        let gt_w = textw(&menu.dc, b">");
        menu.dc.w = lt_w;
        if menu.curr.and_then(|c| menu.items[c].left).is_some() {
            draw_text(&menu.dc, b"<", &menu.normcol)?;
        }
        let mut item = menu.curr;
        while item != menu.next {
            let idx = match item { Some(i) => i, None => break };
            menu.dc.x += menu.dc.w;
            let item_w = textw(&menu.dc, &menu.items[idx].text);
            menu.dc.w = item_w.min(menu.mw - menu.dc.x - gt_w);
            let col = if Some(idx) == menu.sel { menu.selcol } else { menu.normcol };
            let txt = menu.items[idx].text.clone();
            draw_text(&menu.dc, &txt, &col)?;
            item = menu.items[idx].right;
        }
        menu.dc.w = gt_w;
        menu.dc.x = menu.mw - menu.dc.w;
        if menu.next.is_some() {
            draw_text(&menu.dc, b">", &menu.normcol)?;
        }
    }
    let _ = dx_after_prompt;
    map_dc(&menu.dc, menu.win, menu.mw as u16, menu.mh as u16)?;
    menu.dc.conn.flush()?;
    Ok(())
}

fn nextrune(text: &[u8], cursor: usize, inc: i32) -> isize {
    let mut n = cursor as isize + inc as isize;
    while n + inc as isize >= 0
        && (n as usize) < text.len()
        && (text[n as usize] & 0xc0) == 0x80
    {
        n += inc as isize;
    }
    n
}

fn insert_text(menu: &mut Menu, s: &[u8], n: isize) {
    let cur = menu.cursor as isize;
    let cur_len = menu.text.len() as isize;
    if cur_len + n < 0 || cur_len + n > 4096 {
        return;
    }
    if n > 0 {
        // Insert s (n bytes) at cursor
        let pre = menu.text[..menu.cursor].to_vec();
        let post = menu.text[menu.cursor..].to_vec();
        menu.text.clear();
        menu.text.extend_from_slice(&pre);
        menu.text.extend_from_slice(&s[..n as usize]);
        menu.text.extend_from_slice(&post);
        menu.cursor = (cur + n) as usize;
    } else if n < 0 {
        // Delete -n bytes ending at cursor
        let del = (-n) as usize;
        let start = menu.cursor.saturating_sub(del);
        menu.text.drain(start..menu.cursor);
        menu.cursor = start;
    }
    let sub = n > 0 && menu.cursor == menu.text.len();
    do_match(menu, sub);
}

fn paste(menu: &mut Menu) -> Result<(), Box<dyn Error>> {
    let r = menu.dc.conn.get_property(false, menu.win, menu.utf8, menu.utf8,
        0, (4096 / 4) + 1)?.reply()?;
    let data = r.value;
    let mut end = data.len();
    if let Some(pos) = data.iter().position(|&b| b == b'\n') {
        end = pos;
    }
    insert_text(menu, &data[..end], end as isize);
    draw_menu(menu)?;
    Ok(())
}


enum KeyAction {
    None,
    Continue,   // already handled, redraw
    Exit(bool), // exit success(true) prints text or sel
}

fn keypress(menu: &mut Menu, ev: KeyPressEvent) -> Result<KeyAction, Box<dyn Error>> {
    use keysyms::*;
    let state: u16 = ev.state.into();
    let mut ksym = keysym_for(menu, ev.detail, state);
    let ctrl = (state & u16::from(KeyButMask::CONTROL)) != 0;
    let shift = (state & u16::from(KeyButMask::SHIFT)) != 0;
    if ctrl {
        let lower = keysyms::to_lower(ksym);
        match lower {
            x if x == XK_a => ksym = XK_Home,
            x if x == XK_b => ksym = XK_Left,
            x if x == XK_c => ksym = XK_Escape,
            x if x == XK_d => ksym = XK_Delete,
            x if x == XK_e => ksym = XK_End,
            x if x == XK_f => ksym = XK_Right,
            x if x == XK_h => ksym = XK_BackSpace,
            x if x == XK_i => ksym = XK_Tab,
            x if x == XK_j => ksym = XK_Return,
            x if x == XK_k => {
                menu.text.truncate(menu.cursor);
                do_match(menu, false);
                return Ok(KeyAction::Continue);
            }
            x if x == XK_n => ksym = XK_Down,
            x if x == XK_p => ksym = XK_Up,
            x if x == XK_u => {
                let cur = menu.cursor as isize;
                insert_text(menu, &[], -cur);
                return Ok(KeyAction::Continue);
            }
            x if x == XK_w => {
                while menu.cursor > 0
                    && menu.text[(nextrune(&menu.text, menu.cursor, -1) as usize)] == b' '
                {
                    let r = nextrune(&menu.text, menu.cursor, -1);
                    let n = r - menu.cursor as isize;
                    insert_text(menu, &[], n);
                }
                while menu.cursor > 0
                    && menu.text[(nextrune(&menu.text, menu.cursor, -1) as usize)] != b' '
                {
                    let r = nextrune(&menu.text, menu.cursor, -1);
                    let n = r - menu.cursor as isize;
                    insert_text(menu, &[], n);
                }
                return Ok(KeyAction::Continue);
            }
            x if x == XK_y => {
                menu.dc.conn.convert_selection(menu.win,
                    AtomEnum::PRIMARY.into(),
                    menu.utf8, menu.utf8, CURRENT_TIME)?;
                menu.dc.conn.flush()?;
                return Ok(KeyAction::None);
            }
            _ => return Ok(KeyAction::None),
        }
    }

    let mut handled_special = true;
    match ksym {
        XK_Delete => {
            if menu.cursor < menu.text.len() {
                let r = nextrune(&menu.text, menu.cursor, 1);
                menu.cursor = r as usize;
                let r2 = nextrune(&menu.text, menu.cursor, -1);
                let n = r2 - menu.cursor as isize;
                if menu.cursor > 0 { insert_text(menu, &[], n); }
            }
        }
        XK_BackSpace => {
            if menu.cursor > 0 {
                let r = nextrune(&menu.text, menu.cursor, -1);
                let n = r - menu.cursor as isize;
                insert_text(menu, &[], n);
            }
        }
        XK_End => {
            if menu.cursor != menu.text.len() {
                menu.cursor = menu.text.len();
            } else {
                if menu.next.is_some() {
                    menu.curr = menu.matchend;
                    calcoffsets(menu);
                    menu.curr = menu.prev;
                    calcoffsets(menu);
                    while menu.next.is_some() {
                        if let Some(c) = menu.curr {
                            if let Some(r) = menu.items[c].right {
                                menu.curr = Some(r);
                                calcoffsets(menu);
                                continue;
                            }
                        }
                        break;
                    }
                }
                menu.sel = menu.matchend;
            }
        }
        XK_Escape => return Ok(KeyAction::Exit(false)),
        XK_Home => {
            if menu.sel == menu.matches {
                menu.cursor = 0;
            } else {
                menu.sel = menu.matches;
                menu.curr = menu.matches;
                calcoffsets(menu);
            }
        }
        XK_Left => {
            if menu.cursor > 0
                && (menu.sel.is_none()
                    || menu.sel.and_then(|s| menu.items[s].left).is_none()
                    || menu.lines > 0)
            {
                let r = nextrune(&menu.text, menu.cursor, -1);
                menu.cursor = r.max(0) as usize;
            } else if menu.lines > 0 {
                return Ok(KeyAction::None);
            } else {
                handle_up(menu);
            }
        }
        XK_Up => handle_up(menu),
        XK_Next => {
            if menu.next.is_none() { return Ok(KeyAction::None); }
            menu.sel = menu.next;
            menu.curr = menu.next;
            calcoffsets(menu);
        }
        XK_Prior => {
            if menu.prev.is_none() { return Ok(KeyAction::None); }
            menu.sel = menu.prev;
            menu.curr = menu.prev;
            calcoffsets(menu);
        }
        XK_Return | XK_KP_Enter => {
            if menu.sel.is_some() && !shift {
                let txt = menu.items[menu.sel.unwrap()].text.clone();
                io::stdout().write_all(&txt)?;
            } else {
                io::stdout().write_all(&menu.text)?;
            }
            io::stdout().write_all(b"\n")?;
            return Ok(KeyAction::Exit(true));
        }
        XK_Right => {
            if menu.cursor < menu.text.len() {
                let r = nextrune(&menu.text, menu.cursor, 1);
                menu.cursor = r as usize;
            } else if menu.lines > 0 {
                return Ok(KeyAction::None);
            } else {
                handle_down(menu);
            }
        }
        XK_Down => handle_down(menu),
        XK_Tab => {
            if menu.sel.is_none() { return Ok(KeyAction::None); }
            let s = menu.items[menu.sel.unwrap()].text.clone();
            menu.text.clear();
            menu.text.extend_from_slice(&s);
            menu.cursor = menu.text.len();
            do_match(menu, true);
        }
        _ => handled_special = false,
    }

    if !handled_special {
        if let Some(bytes) = keysym_to_bytes(ksym) {
            insert_text(menu, &bytes, bytes.len() as isize);
        }
    }
    Ok(KeyAction::Continue)
}

fn handle_up(menu: &mut Menu) {
    if let Some(s) = menu.sel {
        if let Some(l) = menu.items[s].left {
            menu.sel = Some(l);
            // if old sel's left's right was curr, then we move curr backwards
            if menu.items[l].right == menu.curr {
                menu.curr = menu.prev;
                calcoffsets(menu);
            }
        }
    }
}

fn handle_down(menu: &mut Menu) {
    if let Some(s) = menu.sel {
        if let Some(r) = menu.items[s].right {
            menu.sel = Some(r);
            if Some(r) == menu.next {
                menu.curr = menu.next;
                calcoffsets(menu);
            }
        }
    }
}


fn run(menu: &mut Menu) -> Result<ExitCode, Box<dyn Error>> {
    loop {
        let ev = menu.dc.conn.wait_for_event()?;
        match ev {
            Event::Expose(e) => {
                if e.count == 0 {
                    draw_menu(menu)?;
                }
            }
            Event::KeyPress(e) => {
                match keypress(menu, e)? {
                    KeyAction::None => {}
                    KeyAction::Continue => { draw_menu(menu)?; }
                    KeyAction::Exit(success) => {
                        let _ = io::stdout().flush();
                        return Ok(if success { ExitCode::from(0) } else { ExitCode::from(1) });
                    }
                }
            }
            Event::SelectionNotify(e) => {
                if e.property == menu.utf8 {
                    paste(menu)?;
                }
            }
            Event::VisibilityNotify(e) => {
                if e.state != Visibility::UNOBSCURED {
                    menu.dc.conn.configure_window(menu.win,
                        &ConfigureWindowAux::default()
                            .stack_mode(StackMode::ABOVE))?;
                    menu.dc.conn.flush()?;
                }
            }
            _ => {}
        }
    }
}

