// dwm - dynamic window manager (Rust port of suckless dwm 5.8.2 + pertag patch)

#![allow(non_upper_case_globals, non_snake_case, dead_code, clippy::too_many_arguments)]

mod draw;
mod keysyms;

use std::error::Error;
use std::process::ExitCode;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xinerama::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

use draw::{Dc, COL_BORDER, COL_FG, COL_BG, COL_LAST};
use keysyms::*;

const VERSION: &str = "5.8.2";
const BROKEN: &[u8] = b"broken";

// cursor indices
const CUR_NORMAL: usize = 0;
const CUR_RESIZE: usize = 1;
const CUR_MOVE: usize   = 2;
const CUR_LAST: usize   = 3;

// EWMH atoms
const NET_SUPPORTED: usize     = 0;
const NET_WM_NAME: usize       = 1;
const NET_WM_STATE: usize      = 2;
const NET_WM_FULLSCREEN: usize = 3;
const NET_LAST: usize          = 4;

// WM atoms
const WM_PROTOCOLS: usize = 0;
const WM_DELETE: usize    = 1;
const WM_STATE: usize     = 2;
const WM_LAST: usize      = 3;

// Click locations (used to dispatch button bindings)
const CLK_TAGBAR: u32      = 0;
const CLK_LT_SYMBOL: u32   = 1;
const CLK_STATUS_TEXT: u32 = 2;
const CLK_WIN_TITLE: u32   = 3;
const CLK_CLIENT_WIN: u32  = 4;
const CLK_ROOT_WIN: u32    = 5;

// XCB cursor font glyphs
const XC_LEFT_PTR: u16 = 68;
const XC_SIZING: u16   = 120;
const XC_FLEUR: u16    = 52;

// State values for ICCCM WM_STATE
const NORMAL_STATE: u32    = 1;
const ICONIC_STATE: u32    = 3;
const WITHDRAWN_STATE: u32 = 0;

// Argument variants for key/button actions
#[derive(Clone)]
enum Arg {
    None,
    I(i32),
    U(u32),
    F(f32),
    Cmd(&'static [&'static str]),
    Layout(LtId),
}

type Action = fn(&mut State, &Arg);

struct LayoutDef {
    symbol: &'static [u8],
    arrange: Option<fn(&mut State, MId)>,
}

struct KeyDef {
    mod_: u16,
    keysym: u32,
    func: Action,
    arg: Arg,
}

struct ButtonDef {
    click: u32,
    mask: u16,
    button: u8,
    func: Action,
    arg: Arg,
}

struct RuleDef {
    class: Option<&'static [u8]>,
    instance: Option<&'static [u8]>,
    title: Option<&'static [u8]>,
    tags: u32,
    isfloating: bool,
    monitor: i32,
}

type CId = usize;
type MId = usize;
type LtId = usize;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() == 2 && argv[1] == "-v" {
        eprintln!("dwm-{}, Rust port of suckless dwm", VERSION);
        return ExitCode::from(0);
    }
    if argv.len() != 1 {
        eprintln!("usage: dwm [-v]");
        return ExitCode::from(1);
    }
    match run() {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            eprintln!("dwm: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut st = State::new()?;
    check_other_wm(&st.conn, st.root)?;
    setup(&mut st)?;
    scan(&mut st)?;
    event_loop(&mut st)?;
    cleanup(&mut st)?;
    Ok(())
}


struct Client {
    name: Vec<u8>,
    mina: f32, maxa: f32,
    x: i32, y: i32, w: i32, h: i32,
    oldx: i32, oldy: i32, oldw: i32, oldh: i32,
    basew: i32, baseh: i32, incw: i32, inch: i32,
    maxw: i32, maxh: i32, minw: i32, minh: i32,
    bw: i32, oldbw: i32,
    tags: u32,
    isfixed: bool, isfloating: bool, isurgent: bool, oldstate: bool,
    next: Option<CId>,
    snext: Option<CId>,
    mon: MId,
    win: Window,
    alive: bool,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            name: Vec::new(),
            mina: 0.0, maxa: 0.0,
            x: 0, y: 0, w: 0, h: 0,
            oldx: 0, oldy: 0, oldw: 0, oldh: 0,
            basew: 0, baseh: 0, incw: 0, inch: 0,
            maxw: 0, maxh: 0, minw: 0, minh: 0,
            bw: 0, oldbw: 0,
            tags: 0,
            isfixed: false, isfloating: false, isurgent: false, oldstate: false,
            next: None, snext: None, mon: 0,
            win: 0, alive: true,
        }
    }
}

struct Monitor {
    ltsymbol: Vec<u8>,
    mfact: f32,
    num: i32,
    by: i32,
    mx: i32, my: i32, mw: i32, mh: i32,
    wx: i32, wy: i32, ww: i32, wh: i32,
    seltags: usize,
    sellt: usize,
    tagset: [u32; 2],
    showbar: bool,
    topbar: bool,
    clients: Option<CId>,
    sel: Option<CId>,
    stack: Option<CId>,
    next: Option<MId>,
    barwin: Window,
    lt: [LtId; 2],
    curtag: usize,
    prevtag: usize,
    lts: Vec<LtId>,
    mfacts: Vec<f32>,
    showbars: Vec<bool>,
    alive: bool,
}

struct State {
    conn: RustConnection,
    screen_num: usize,
    root: Window,
    sw: i32,
    sh: i32,
    bh: i32,
    blw: i32,
    numlockmask: u16,
    running: bool,
    stext: Vec<u8>,
    dc: Dc,
    cursor: [u32; CUR_LAST],
    wmatom: [Atom; WM_LAST],
    netatom: [Atom; NET_LAST],
    clients: Vec<Client>,
    mons: Vec<Monitor>,
    mons_head: Option<MId>,
    selmon: Option<MId>,
    min_keycode: u8,
    keysyms_per_keycode: u8,
    keysym_table: Vec<u32>,
}

// ===== Configuration (mirrors config.h) =====

const FONT: &str = "-*-fixed-medium-r-*-*-14-*-*-*-*-*-iso8859-*";
const NORM_BORDER_COLOR: &str = "#cccccc";
const NORM_BG_COLOR: &str     = "#cccccc";
const NORM_FG_COLOR: &str     = "#000000";
const SEL_BORDER_COLOR: &str  = "#af7817";
const SEL_BG_COLOR: &str      = "#af7817";
const SEL_FG_COLOR: &str      = "#ffffff";
const BORDERPX: u32 = 3;
const SNAP: i32 = 10;
const SHOWBAR: bool = true;
const TOPBAR: bool = true;

static TAGS: &[&[u8]] = &[b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9"];

static RULES: &[RuleDef] = &[
    RuleDef { class: Some(b"Gimp"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"Kate"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"Gedit"),                instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"Gvim"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"VirtualBox"),           instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"nm-connection-editor"), instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"skype"),                instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"Thunderbird"),          instance: None, title: Some(b"Authentication Required"),  tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some(b"Calendar"),             instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
];

const MFACT: f32 = 0.55;
const RESIZE_HINTS: bool = true;

const LT_TILE: LtId    = 0;
const LT_FLOAT: LtId   = 1;
const LT_MONOCLE: LtId = 2;

static LAYOUTS: &[LayoutDef] = &[
    LayoutDef { symbol: b"[]=", arrange: Some(tile) },
    LayoutDef { symbol: b"><>", arrange: None },
    LayoutDef { symbol: b"[M]", arrange: Some(monocle) },
];

// X11 modifier mask bits (raw values, matching Xlib's ShiftMask/etc.)
const SHIFT: u16   = 0x0001; // ShiftMask
const LOCK_M: u16  = 0x0002; // LockMask
const CONTROL: u16 = 0x0004; // ControlMask
const MOD1: u16    = 0x0008; // Mod1Mask
const MOD_KEY: u16 = MOD1;

const DMENU_CMD: &[&str] = &["dmenu_run", "-fn", FONT,
    "-nb", NORM_BG_COLOR, "-nf", NORM_FG_COLOR,
    "-sb", SEL_BG_COLOR,  "-sf", SEL_FG_COLOR];
const TERM_CMD: &[&str]    = &["/home/phill/bin/konsole"];
const LOCK_CMD: &[&str]    = &["/home/phill/bin/dl"];
const PAUSE_CMD: &[&str]   = &["/home/phill/bin/pause"];
const VOL_DOWN_CMD: &[&str] = &["/home/phill/bin/vol_down"];
const VOL_UP_CMD: &[&str]   = &["/home/phill/bin/vol_up"];
const FORWARD_TRACK: &[&str] = &["/home/phill/bin/forward"];


fn build_keys() -> Vec<KeyDef> {
    let mut k = vec![
        KeyDef { mod_: MOD_KEY,            keysym: XK_p,      func: spawn,          arg: Arg::Cmd(DMENU_CMD) },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_Return, func: spawn,          arg: Arg::Cmd(TERM_CMD) },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_l,      func: spawn,          arg: Arg::Cmd(LOCK_CMD) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_F9,     func: spawn,          arg: Arg::Cmd(PAUSE_CMD) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_F10,    func: spawn,          arg: Arg::Cmd(VOL_DOWN_CMD) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_F11,    func: spawn,          arg: Arg::Cmd(VOL_UP_CMD) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_F12,    func: spawn,          arg: Arg::Cmd(FORWARD_TRACK) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_b,      func: togglebar,      arg: Arg::None },
        KeyDef { mod_: MOD_KEY,            keysym: XK_j,      func: focusstack,     arg: Arg::I(1) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_k,      func: focusstack,     arg: Arg::I(-1) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_h,      func: setmfact,       arg: Arg::F(-0.05) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_l,      func: setmfact,       arg: Arg::F(0.05) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_Return, func: zoom,           arg: Arg::None },
        KeyDef { mod_: MOD_KEY,            keysym: XK_Tab,    func: view,           arg: Arg::None },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_c,      func: killclient,     arg: Arg::None },
        KeyDef { mod_: MOD_KEY,            keysym: XK_t,      func: setlayout,      arg: Arg::Layout(LT_TILE) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_f,      func: setlayout,      arg: Arg::Layout(LT_FLOAT) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_m,      func: setlayout,      arg: Arg::Layout(LT_MONOCLE) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_space,  func: setlayout,      arg: Arg::None },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_space,  func: togglefloating, arg: Arg::None },
        KeyDef { mod_: MOD_KEY,            keysym: XK_0,      func: view,           arg: Arg::U(!0) },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_0,      func: tag,            arg: Arg::U(!0) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_comma,  func: focusmon,       arg: Arg::I(-1) },
        KeyDef { mod_: MOD_KEY,            keysym: XK_period, func: focusmon,       arg: Arg::I(1) },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_comma,  func: tagmon,         arg: Arg::I(-1) },
        KeyDef { mod_: MOD_KEY|SHIFT,      keysym: XK_period, func: tagmon,         arg: Arg::I(1) },
    ];
    let tag_keys = [XK_1, XK_2, XK_3, XK_4, XK_5, XK_6, XK_7, XK_8, XK_9];
    for (i, &ks) in tag_keys.iter().enumerate() {
        let mask = 1u32 << i;
        k.push(KeyDef { mod_: MOD_KEY,                       keysym: ks, func: view,       arg: Arg::U(mask) });
        k.push(KeyDef { mod_: MOD_KEY|CONTROL,               keysym: ks, func: toggleview, arg: Arg::U(mask) });
        k.push(KeyDef { mod_: MOD_KEY|SHIFT,                 keysym: ks, func: tag,        arg: Arg::U(mask) });
        k.push(KeyDef { mod_: MOD_KEY|CONTROL|SHIFT,         keysym: ks, func: toggletag,  arg: Arg::U(mask) });
    }
    k.push(KeyDef { mod_: MOD_KEY|SHIFT, keysym: XK_q, func: quit, arg: Arg::None });
    k
}

fn build_buttons() -> Vec<ButtonDef> {
    vec![
        ButtonDef { click: CLK_LT_SYMBOL,   mask: 0,        button: 1, func: setlayout,      arg: Arg::None },
        ButtonDef { click: CLK_LT_SYMBOL,   mask: 0,        button: 3, func: setlayout,      arg: Arg::Layout(LT_MONOCLE) },
        ButtonDef { click: CLK_WIN_TITLE,   mask: 0,        button: 2, func: zoom,           arg: Arg::None },
        ButtonDef { click: CLK_STATUS_TEXT, mask: 0,        button: 2, func: spawn,          arg: Arg::Cmd(TERM_CMD) },
        ButtonDef { click: CLK_CLIENT_WIN,  mask: MOD_KEY,  button: 1, func: movemouse,      arg: Arg::None },
        ButtonDef { click: CLK_CLIENT_WIN,  mask: MOD_KEY,  button: 2, func: togglefloating, arg: Arg::None },
        ButtonDef { click: CLK_CLIENT_WIN,  mask: MOD_KEY,  button: 3, func: resizemouse,    arg: Arg::None },
        ButtonDef { click: CLK_TAGBAR,      mask: 0,        button: 1, func: view,           arg: Arg::None },
        ButtonDef { click: CLK_TAGBAR,      mask: 0,        button: 3, func: toggleview,     arg: Arg::None },
        ButtonDef { click: CLK_TAGBAR,      mask: MOD_KEY,  button: 1, func: tag,            arg: Arg::None },
        ButtonDef { click: CLK_TAGBAR,      mask: MOD_KEY,  button: 3, func: toggletag,      arg: Arg::None },
    ]
}

const TAG_MASK: u32 = (1u32 << 9) - 1;


impl State {
    fn new() -> Result<Self, Box<dyn Error>> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let sw = screen.width_in_pixels as i32;
        let sh = screen.height_in_pixels as i32;
        let font = draw::load_font(&conn, FONT)?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, root, &CreateGCAux::default()
            .line_style(LineStyle::SOLID)
            .cap_style(CapStyle::BUTT)
            .join_style(JoinStyle::MITER))?;
        let dc = Dc {
            gc, drawable: 0, drawable_w: 0, drawable_h: 0,
            font, norm: [0; COL_LAST], sel: [0; COL_LAST],
            x: 0, y: 0, w: 0, h: 0,
        };
        Ok(State {
            conn, screen_num, root, sw, sh,
            bh: 0, blw: 0, numlockmask: 0, running: true,
            stext: format!("dwm-{}", VERSION).into_bytes(),
            dc,
            cursor: [0; CUR_LAST],
            wmatom: [0; WM_LAST],
            netatom: [0; NET_LAST],
            clients: Vec::new(),
            mons: Vec::new(),
            mons_head: None,
            selmon: None,
            min_keycode: 0, keysyms_per_keycode: 0,
            keysym_table: Vec::new(),
        })
    }
}

// ===== Client/Monitor allocation helpers =====

fn alloc_client(st: &mut State, c: Client) -> CId {
    // Reuse a dead slot if present
    if let Some((idx, slot)) = st.clients.iter_mut().enumerate().find(|(_, c)| !c.alive) {
        *slot = c;
        idx
    } else {
        st.clients.push(c);
        st.clients.len() - 1
    }
}

fn alloc_monitor(st: &mut State) -> MId {
    let mut m = Monitor {
        ltsymbol: LAYOUTS[LT_TILE].symbol.to_vec(),
        mfact: MFACT,
        num: 0,
        by: 0,
        mx: 0, my: 0, mw: 0, mh: 0,
        wx: 0, wy: 0, ww: 0, wh: 0,
        seltags: 0,
        sellt: 0,
        tagset: [1, 1],
        showbar: SHOWBAR,
        topbar: TOPBAR,
        clients: None, sel: None, stack: None,
        next: None,
        barwin: 0,
        lt: [LT_TILE, LT_FLOAT],
        curtag: 1,
        prevtag: 1,
        lts: vec![LT_TILE; TAGS.len() + 1],
        mfacts: vec![MFACT; TAGS.len() + 1],
        showbars: vec![SHOWBAR; TAGS.len() + 1],
        alive: true,
    };
    let _ = &mut m;
    if let Some((idx, slot)) = st.mons.iter_mut().enumerate().find(|(_, x)| !x.alive) {
        *slot = m;
        idx
    } else {
        st.mons.push(m);
        st.mons.len() - 1
    }
}

fn isvisible(c: &Client, m: &Monitor) -> bool {
    (c.tags & m.tagset[m.seltags]) != 0
}

fn width_of(c: &Client) -> i32 { c.w + 2 * c.bw }
fn height_of(c: &Client) -> i32 { c.h + 2 * c.bw }

fn cleanmask(st: &State, mask: u16) -> u16 {
    mask & !(st.numlockmask | LOCK_M)
        & (SHIFT | CONTROL | MOD1 | 0x10 | 0x20 | 0x40 | 0x80)
}


fn check_other_wm(conn: &RustConnection, root: Window) -> Result<(), Box<dyn Error>> {
    let r = conn.change_window_attributes(root, &ChangeWindowAttributesAux::default()
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT))?.check();
    if r.is_err() {
        return Err("dwm: another window manager is already running".into());
    }
    conn.flush()?;
    Ok(())
}

fn setup(st: &mut State) -> Result<(), Box<dyn Error>> {
    // Reap zombies via SIGCHLD = SIG_IGN to avoid keeping handler logic.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN); }

    st.bh = st.dc.font.height + 2;
    st.dc.h = st.bh;

    update_geom(st)?;

    // Atoms
    st.wmatom[WM_PROTOCOLS] = st.conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    st.wmatom[WM_DELETE]    = st.conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    st.wmatom[WM_STATE]     = st.conn.intern_atom(false, b"WM_STATE")?.reply()?.atom;
    st.netatom[NET_SUPPORTED]    = st.conn.intern_atom(false, b"_NET_SUPPORTED")?.reply()?.atom;
    st.netatom[NET_WM_NAME]      = st.conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
    st.netatom[NET_WM_STATE]     = st.conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
    st.netatom[NET_WM_FULLSCREEN] = st.conn.intern_atom(false, b"_NET_WM_STATE_FULLSCREEN")?.reply()?.atom;

    // Cursors
    let cursor_font = st.conn.generate_id()?;
    st.conn.open_font(cursor_font, b"cursor")?.check()?;
    let make_cursor = |glyph: u16| -> Result<u32, Box<dyn Error>> {
        let cid = st.conn.generate_id()?;
        st.conn.create_glyph_cursor(cid, cursor_font, cursor_font, glyph, glyph + 1,
            0, 0, 0, 65535, 65535, 65535)?;
        Ok(cid)
    };
    st.cursor[CUR_NORMAL] = make_cursor(XC_LEFT_PTR)?;
    st.cursor[CUR_RESIZE] = make_cursor(XC_SIZING)?;
    st.cursor[CUR_MOVE]   = make_cursor(XC_FLEUR)?;
    st.conn.close_font(cursor_font)?;

    // Colors
    st.dc.norm[COL_BORDER] = draw::get_color(&st.conn, st.screen_num, NORM_BORDER_COLOR)?;
    st.dc.norm[COL_BG]     = draw::get_color(&st.conn, st.screen_num, NORM_BG_COLOR)?;
    st.dc.norm[COL_FG]     = draw::get_color(&st.conn, st.screen_num, NORM_FG_COLOR)?;
    st.dc.sel[COL_BORDER]  = draw::get_color(&st.conn, st.screen_num, SEL_BORDER_COLOR)?;
    st.dc.sel[COL_BG]      = draw::get_color(&st.conn, st.screen_num, SEL_BG_COLOR)?;
    st.dc.sel[COL_FG]      = draw::get_color(&st.conn, st.screen_num, SEL_FG_COLOR)?;

    // Drawable for the bar
    let depth = st.conn.setup().roots[st.screen_num].root_depth;
    let pid = st.conn.generate_id()?;
    st.conn.create_pixmap(depth, pid, st.root, st.sw.max(1) as u16, st.bh.max(1) as u16)?;
    st.dc.drawable = pid;
    st.dc.drawable_w = st.sw.max(1) as u16;
    st.dc.drawable_h = st.bh.max(1) as u16;

    update_bars(st)?;
    update_status(st)?;

    // EWMH
    let supported: Vec<Atom> = st.netatom.to_vec();
    st.conn.change_property32(PropMode::REPLACE, st.root,
        st.netatom[NET_SUPPORTED], AtomEnum::ATOM, &supported)?;

    // Root window event mask + cursor
    st.conn.change_window_attributes(st.root, &ChangeWindowAttributesAux::default()
        .cursor(st.cursor[CUR_NORMAL])
        .event_mask(EventMask::SUBSTRUCTURE_REDIRECT
            | EventMask::SUBSTRUCTURE_NOTIFY
            | EventMask::BUTTON_PRESS
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::PROPERTY_CHANGE))?;
    load_keymap(st)?;
    grabkeys(st)?;
    st.conn.flush()?;
    Ok(())
}

fn load_keymap(st: &mut State) -> Result<(), Box<dyn Error>> {
    let setup = st.conn.setup();
    st.min_keycode = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max - st.min_keycode + 1;
    let r = st.conn.get_keyboard_mapping(st.min_keycode, count)?.reply()?;
    st.keysyms_per_keycode = r.keysyms_per_keycode;
    st.keysym_table = r.keysyms;
    Ok(())
}

// Iterate the monitor linked-list, returning an owned Vec of MIds.
fn mon_list(st: &State) -> Vec<MId> {
    let mut v = Vec::new();
    let mut m = st.mons_head;
    while let Some(i) = m { v.push(i); m = st.mons[i].next; }
    v
}

// Iterate clients on a monitor, returning an owned Vec of CIds.
fn client_list(st: &State, m: MId) -> Vec<CId> {
    let mut v = Vec::new();
    let mut c = st.mons[m].clients;
    while let Some(i) = c { v.push(i); c = st.clients[i].next; }
    v
}

fn stack_list(st: &State, m: MId) -> Vec<CId> {
    let mut v = Vec::new();
    let mut c = st.mons[m].stack;
    while let Some(i) = c { v.push(i); c = st.clients[i].snext; }
    v
}

fn update_bar_pos(st: &mut State, m: MId) {
    let mon = &mut st.mons[m];
    mon.wy = mon.my;
    mon.wh = mon.mh;
    if mon.showbar {
        mon.wh -= st.bh;
        mon.by = if mon.topbar { mon.wy } else { mon.wy + mon.wh };
        mon.wy = if mon.topbar { mon.wy + st.bh } else { mon.wy };
    } else {
        mon.by = -st.bh;
    }
}

fn update_bars(st: &mut State) -> Result<(), Box<dyn Error>> {
    let depth = st.conn.setup().roots[st.screen_num].root_depth;
    let visual = st.conn.setup().roots[st.screen_num].root_visual;
    for m in mon_list(st) {
        if st.mons[m].barwin != 0 { continue; }
        let win = st.conn.generate_id()?;
        let aux = CreateWindowAux::default()
            .override_redirect(1)
            .background_pixmap(1u32) // ParentRelative
            .event_mask(EventMask::BUTTON_PRESS | EventMask::EXPOSURE);
        let mon = &st.mons[m];
        st.conn.create_window(depth, win, st.root,
            mon.wx as i16, mon.by as i16, mon.ww.max(1) as u16, st.bh as u16,
            0, WindowClass::COPY_FROM_PARENT, visual, &aux)?;
        st.conn.change_window_attributes(win, &ChangeWindowAttributesAux::default()
            .cursor(st.cursor[CUR_NORMAL]))?;
        st.conn.configure_window(win, &ConfigureWindowAux::default()
            .stack_mode(StackMode::ABOVE))?;
        st.conn.map_window(win)?;
        st.mons[m].barwin = win;
    }
    Ok(())
}

fn update_status(st: &mut State) -> Result<(), Box<dyn Error>> {
    let txt = get_text_prop(st, st.root, AtomEnum::WM_NAME.into())?;
    if txt.is_empty() {
        st.stext = format!("dwm-{}", VERSION).into_bytes();
    } else {
        st.stext = draw::utf8_to_drawable(&txt);
    }
    if let Some(m) = st.selmon {
        drawbar(st, m)?;
    }
    Ok(())
}

fn get_text_prop(st: &State, w: Window, atom: Atom) -> Result<Vec<u8>, Box<dyn Error>> {
    let r = st.conn.get_property(false, w, atom, AtomEnum::ANY, 0, 1024)?.reply()?;
    Ok(r.value)
}

fn update_geom(st: &mut State) -> Result<bool, Box<dyn Error>> {
    let mut dirty = false;
    let xinerama_active = st.conn.xinerama_is_active().ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.state).unwrap_or(0) != 0;
    if xinerama_active {
        let info = st.conn.xinerama_query_screens()?.reply()?.screen_info;
        let mut unique: Vec<&x11rb::protocol::xinerama::ScreenInfo> = Vec::new();
        'outer: for s in &info {
            for u in &unique {
                if u.x_org == s.x_org && u.y_org == s.y_org
                    && u.width == s.width && u.height == s.height { continue 'outer; }
            }
            unique.push(s);
        }
        let nn = unique.len();
        let n = mon_list(st).len();
        if n <= nn {
            for _ in 0..(nn - n) {
                let new_id = alloc_monitor(st);
                if let Some(head) = st.mons_head {
                    let mut last = head;
                    while let Some(nx) = st.mons[last].next { last = nx; }
                    st.mons[last].next = Some(new_id);
                } else {
                    st.mons_head = Some(new_id);
                }
            }
            for (i, m) in mon_list(st).iter().enumerate() {
                if i < nn {
                    let s = &unique[i];
                    let mon = &mut st.mons[*m];
                    if i >= n
                        || mon.mx != s.x_org as i32 || mon.my != s.y_org as i32
                        || mon.mw != s.width as i32 || mon.mh != s.height as i32
                    {
                        dirty = true;
                        mon.num = i as i32;
                        mon.mx = s.x_org as i32; mon.wx = mon.mx;
                        mon.my = s.y_org as i32; mon.wy = mon.my;
                        mon.mw = s.width as i32; mon.ww = mon.mw;
                        mon.mh = s.height as i32; mon.wh = mon.mh;
                        update_bar_pos(st, *m);
                    }
                }
            }
        } else {
            // Fewer monitors: collapse extras into first.
            let all = mon_list(st);
            for &m in all.iter().skip(nn) {
                let cs = client_list(st, m);
                for c in cs {
                    dirty = true;
                    detach(st, c); detach_stack(st, c);
                    st.clients[c].mon = st.mons_head.unwrap_or(m);
                    attach(st, c); attach_stack(st, c);
                }
                if Some(m) == st.selmon { st.selmon = st.mons_head; }
                cleanup_mon(st, m)?;
            }
        }
    } else {
        if st.mons_head.is_none() {
            let m = alloc_monitor(st);
            st.mons_head = Some(m);
        }
        if let Some(m) = st.mons_head {
            let mon = &mut st.mons[m];
            if mon.mw != st.sw || mon.mh != st.sh {
                dirty = true;
                mon.mw = st.sw; mon.ww = st.sw;
                mon.mh = st.sh; mon.wh = st.sh;
                update_bar_pos(st, m);
            }
        }
    }
    if dirty {
        st.selmon = st.mons_head;
        st.selmon = wintomon(st, st.root);
    }
    Ok(dirty)
}

fn attach(st: &mut State, c: CId) {
    let m = st.clients[c].mon;
    st.clients[c].next = st.mons[m].clients;
    st.mons[m].clients = Some(c);
}

fn attach_stack(st: &mut State, c: CId) {
    let m = st.clients[c].mon;
    st.clients[c].snext = st.mons[m].stack;
    st.mons[m].stack = Some(c);
}

fn detach(st: &mut State, c: CId) {
    let m = st.clients[c].mon;
    if st.mons[m].clients == Some(c) {
        st.mons[m].clients = st.clients[c].next;
        return;
    }
    let mut prev = st.mons[m].clients;
    while let Some(p) = prev {
        if st.clients[p].next == Some(c) {
            st.clients[p].next = st.clients[c].next;
            return;
        }
        prev = st.clients[p].next;
    }
}

fn detach_stack(st: &mut State, c: CId) {
    let m = st.clients[c].mon;
    if st.mons[m].stack == Some(c) {
        st.mons[m].stack = st.clients[c].snext;
    } else {
        let mut prev = st.mons[m].stack;
        while let Some(p) = prev {
            if st.clients[p].snext == Some(c) {
                st.clients[p].snext = st.clients[c].snext;
                break;
            }
            prev = st.clients[p].snext;
        }
    }
    if st.mons[m].sel == Some(c) {
        let mut t = st.mons[m].stack;
        while let Some(ti) = t {
            if isvisible(&st.clients[ti], &st.mons[m]) { break; }
            t = st.clients[ti].snext;
        }
        st.mons[m].sel = t;
    }
}

fn cleanup_mon(st: &mut State, m: MId) -> Result<(), Box<dyn Error>> {
    // Unlink monitor from list
    if st.mons_head == Some(m) {
        st.mons_head = st.mons[m].next;
    } else {
        let mut prev = st.mons_head;
        while let Some(p) = prev {
            if st.mons[p].next == Some(m) {
                st.mons[p].next = st.mons[m].next;
                break;
            }
            prev = st.mons[p].next;
        }
    }
    let bw = st.mons[m].barwin;
    if bw != 0 {
        st.conn.unmap_window(bw)?;
        st.conn.destroy_window(bw)?;
    }
    st.mons[m].alive = false;
    Ok(())
}

fn wintoclient(st: &State, w: Window) -> Option<CId> {
    for m in mon_list(st) {
        for c in client_list(st, m) {
            if st.clients[c].win == w { return Some(c); }
        }
    }
    None
}

fn wintomon(st: &State, w: Window) -> Option<MId> {
    if w == st.root {
        if let Ok(p) = st.conn.query_pointer(st.root) {
            if let Ok(p) = p.reply() {
                return ptrtomon(st, p.root_x as i32, p.root_y as i32);
            }
        }
    }
    for m in mon_list(st) {
        if st.mons[m].barwin == w { return Some(m); }
    }
    if let Some(c) = wintoclient(st, w) {
        return Some(st.clients[c].mon);
    }
    st.selmon
}

fn ptrtomon(st: &State, x: i32, y: i32) -> Option<MId> {
    for m in mon_list(st) {
        let mon = &st.mons[m];
        if x >= mon.wx && x < mon.wx + mon.ww
            && y >= mon.wy && y < mon.wy + mon.wh {
            return Some(m);
        }
    }
    st.selmon
}

fn dirtomon(st: &State, dir: i32) -> Option<MId> {
    let cur = st.selmon?;
    if dir > 0 {
        st.mons[cur].next.or(st.mons_head)
    } else {
        let all = mon_list(st);
        if Some(cur) == st.mons_head {
            all.last().copied()
        } else {
            // last whose next == cur
            let mut prev = st.mons_head;
            while let Some(p) = prev {
                if st.mons[p].next == Some(cur) { return Some(p); }
                prev = st.mons[p].next;
            }
            st.mons_head
        }
    }
}


fn drawbar(st: &mut State, m: MId) -> Result<(), Box<dyn Error>> {
    let mut occ: u32 = 0;
    let mut urg: u32 = 0;
    for c in client_list(st, m) {
        occ |= st.clients[c].tags;
        if st.clients[c].isurgent { urg |= st.clients[c].tags; }
    }
    let bh = st.bh;
    let mw = st.mons[m].ww;
    let tagset_seltags = st.mons[m].tagset[st.mons[m].seltags];
    let is_selmon = Some(m) == st.selmon;
    let sel_client = st.mons[m].sel;
    st.dc.x = 0;
    st.dc.h = bh;

    // Tags
    for (i, &tag) in TAGS.iter().enumerate() {
        let tw = draw::textw(&st.dc, tag);
        st.dc.w = tw;
        let invert = (urg & (1 << i)) != 0;
        let col = if (tagset_seltags & (1 << i)) != 0 { st.dc.sel } else { st.dc.norm };
        draw::drawtext(&st.conn, &st.dc, Some(tag), &col, invert)?;
        let filled = is_selmon && sel_client.map(|c| (st.clients[c].tags & (1 << i)) != 0).unwrap_or(false);
        let empty = (occ & (1 << i)) != 0;
        draw::drawsquare(&st.conn, &st.dc, filled, empty, invert, &col)?;
        st.dc.x += tw;
    }
    // Layout symbol
    let lt_sym = st.mons[m].ltsymbol.clone();
    let ltw = draw::textw(&st.dc, &lt_sym);
    st.dc.w = ltw;
    st.blw = ltw;
    let norm = st.dc.norm;
    draw::drawtext(&st.conn, &st.dc, Some(&lt_sym), &norm, false)?;
    st.dc.x += ltw;
    let after_lt = st.dc.x;

    // Status text on selmon, padding on others
    if is_selmon {
        let stext = st.stext.clone();
        let sw = draw::textw(&st.dc, &stext);
        st.dc.x = mw - sw;
        let mut w_status = sw;
        if st.dc.x < after_lt {
            st.dc.x = after_lt;
            w_status = mw - after_lt;
        }
        st.dc.w = w_status;
        let norm = st.dc.norm;
        draw::drawtext(&st.conn, &st.dc, Some(&stext), &norm, false)?;
    } else {
        st.dc.x = mw;
    }

    // Title area
    st.dc.w = st.dc.x - after_lt;
    if st.dc.w > bh {
        st.dc.x = after_lt;
        if let Some(c) = sel_client {
            let col = if is_selmon { st.dc.sel } else { st.dc.norm };
            let name = st.clients[c].name.clone();
            draw::drawtext(&st.conn, &st.dc, Some(&name), &col, false)?;
            let isfixed = st.clients[c].isfixed;
            let isfloating = st.clients[c].isfloating;
            draw::drawsquare(&st.conn, &st.dc, isfixed, isfloating, false, &col)?;
        } else {
            let norm = st.dc.norm;
            draw::drawtext(&st.conn, &st.dc, None, &norm, false)?;
        }
    }
    let bw = st.mons[m].barwin;
    if bw != 0 {
        st.conn.copy_area(st.dc.drawable, bw, st.dc.gc, 0, 0, 0, 0, mw as u16, bh as u16)?;
    }
    st.conn.flush()?;
    Ok(())
}

fn drawbars(st: &mut State) -> Result<(), Box<dyn Error>> {
    for m in mon_list(st) {
        drawbar(st, m)?;
    }
    Ok(())
}

fn nexttiled(st: &State, mut c: Option<CId>) -> Option<CId> {
    while let Some(ci) = c {
        let cl = &st.clients[ci];
        let m = &st.mons[cl.mon];
        if !cl.isfloating && isvisible(cl, m) { return Some(ci); }
        c = cl.next;
    }
    None
}

fn arrange(st: &mut State, m: Option<MId>) -> Result<(), Box<dyn Error>> {
    if let Some(mi) = m {
        let head = st.mons[mi].stack;
        showhide(st, head)?;
    } else {
        for mi in mon_list(st) {
            let head = st.mons[mi].stack;
            showhide(st, head)?;
        }
    }
    focus(st, None)?;
    if let Some(mi) = m {
        arrangemon(st, mi)?;
    } else {
        for mi in mon_list(st) { arrangemon(st, mi)?; }
    }
    Ok(())
}

fn arrangemon(st: &mut State, m: MId) -> Result<(), Box<dyn Error>> {
    let lt = st.mons[m].lt[st.mons[m].sellt];
    st.mons[m].ltsymbol = LAYOUTS[lt].symbol.to_vec();
    if let Some(f) = LAYOUTS[lt].arrange {
        f(st, m);
    }
    restack(st, m)?;
    Ok(())
}

fn showhide(st: &mut State, c: Option<CId>) -> Result<(), Box<dyn Error>> {
    let ci = match c { Some(i) => i, None => return Ok(()) };
    let m = st.clients[ci].mon;
    let snext = st.clients[ci].snext;
    if isvisible(&st.clients[ci], &st.mons[m]) {
        let (x, y) = (st.clients[ci].x, st.clients[ci].y);
        st.conn.configure_window(st.clients[ci].win,
            &ConfigureWindowAux::default().x(x).y(y))?;
        let lt = st.mons[m].lt[st.mons[m].sellt];
        if LAYOUTS[lt].arrange.is_none() || st.clients[ci].isfloating {
            let (cx, cy, cw, ch) = (st.clients[ci].x, st.clients[ci].y, st.clients[ci].w, st.clients[ci].h);
            resize(st, ci, cx, cy, cw, ch, false)?;
        }
        showhide(st, snext)?;
    } else {
        showhide(st, snext)?;
        let (cx, cy) = (st.clients[ci].x, st.clients[ci].y);
        let off_x = cx + 2 * st.sw;
        st.conn.configure_window(st.clients[ci].win,
            &ConfigureWindowAux::default().x(off_x).y(cy))?;
    }
    Ok(())
}


fn tile(st: &mut State, m: MId) {
    let mut clients_tiled = Vec::new();
    let mut c = nexttiled(st, st.mons[m].clients);
    while let Some(ci) = c {
        clients_tiled.push(ci);
        c = nexttiled(st, st.clients[ci].next);
    }
    let n = clients_tiled.len();
    if n == 0 { return; }

    let (wx, wy, ww, wh) = (st.mons[m].wx, st.mons[m].wy, st.mons[m].ww, st.mons[m].wh);
    let mfact = st.mons[m].mfact;
    let mw = (mfact * ww as f32) as i32;

    // Master
    let first = clients_tiled[0];
    let bw = st.clients[first].bw;
    let mw_eff = if n == 1 { ww } else { mw };
    let _ = resize(st, first, wx, wy, mw_eff - 2 * bw, wh - 2 * bw, false);
    if n == 1 { return; }

    // Stack
    let first_x = st.clients[first].x;
    let first_w = st.clients[first].w;
    let x = if wx + mw > first_x + first_w { first_x + first_w + 2 * bw } else { wx + mw };
    let mut y = wy;
    let w = if wx + mw > first_x + first_w { wx + ww - x } else { ww - mw };
    let stack_n = n - 1;
    let h_each = wh / stack_n as i32;
    let h_each = if h_each < st.bh { wh } else { h_each };
    for (i, &ci) in clients_tiled.iter().skip(1).enumerate() {
        let cbw = st.clients[ci].bw;
        let h = if i + 1 == stack_n { wy + wh - y - 2 * cbw } else { h_each - 2 * cbw };
        let _ = resize(st, ci, x, y, w - 2 * cbw, h, false);
        if h_each != wh {
            y = st.clients[ci].y + height_of(&st.clients[ci]);
        }
    }
}

fn monocle(st: &mut State, m: MId) {
    let mut n = 0u32;
    for c in client_list(st, m) {
        if isvisible(&st.clients[c], &st.mons[m]) { n += 1; }
    }
    if n > 0 {
        let s = format!("[{}]", n);
        st.mons[m].ltsymbol = s.into_bytes();
    }
    let (wx, wy, ww, wh) = (st.mons[m].wx, st.mons[m].wy, st.mons[m].ww, st.mons[m].wh);
    let mut c = nexttiled(st, st.mons[m].clients);
    while let Some(ci) = c {
        let bw = st.clients[ci].bw;
        let _ = resize(st, ci, wx, wy, ww - 2 * bw, wh - 2 * bw, false);
        c = nexttiled(st, st.clients[ci].next);
    }
}

fn restack(st: &mut State, m: MId) -> Result<(), Box<dyn Error>> {
    drawbar(st, m)?;
    let sel = match st.mons[m].sel { Some(s) => s, None => return Ok(()) };
    let lt = st.mons[m].lt[st.mons[m].sellt];
    if st.clients[sel].isfloating || LAYOUTS[lt].arrange.is_none() {
        let win = st.clients[sel].win;
        st.conn.configure_window(win,
            &ConfigureWindowAux::default().stack_mode(StackMode::ABOVE))?;
    }
    if LAYOUTS[lt].arrange.is_some() {
        let mut sibling = st.mons[m].barwin;
        for ci in stack_list(st, m) {
            if !st.clients[ci].isfloating && isvisible(&st.clients[ci], &st.mons[m]) {
                let win = st.clients[ci].win;
                st.conn.configure_window(win, &ConfigureWindowAux::default()
                    .sibling(sibling).stack_mode(StackMode::BELOW))?;
                sibling = win;
            }
        }
    }
    st.conn.flush()?;
    // Drain enter-window events that occur from the restack.
    drain_enter_events(st)?;
    Ok(())
}

fn drain_enter_events(_st: &mut State) -> Result<(), Box<dyn Error>> {
    // x11rb wait_for_event blocks; here we rely on event loop to absorb them.
    Ok(())
}

fn applysizehints(st: &State, c: CId, x: &mut i32, y: &mut i32,
                  w: &mut i32, h: &mut i32, interact: bool) -> bool
{
    let cl = &st.clients[c];
    let m = &st.mons[cl.mon];
    *w = (*w).max(1);
    *h = (*h).max(1);
    if interact {
        if *x > st.sw { *x = st.sw - width_of(cl); }
        if *y > st.sh { *y = st.sh - height_of(cl); }
        if *x + *w + 2 * cl.bw < 0 { *x = 0; }
        if *y + *h + 2 * cl.bw < 0 { *y = 0; }
    } else {
        if *x > m.mx + m.mw { *x = m.mx + m.mw - width_of(cl); }
        if *y > m.my + m.mh { *y = m.my + m.mh - height_of(cl); }
        if *x + *w + 2 * cl.bw < m.mx { *x = m.mx; }
        if *y + *h + 2 * cl.bw < m.my { *y = m.my; }
    }
    if *h < st.bh { *h = st.bh; }
    if *w < st.bh { *w = st.bh; }
    if RESIZE_HINTS || cl.isfloating {
        let baseismin = cl.basew == cl.minw && cl.baseh == cl.minh;
        if !baseismin { *w -= cl.basew; *h -= cl.baseh; }
        if cl.mina > 0.0 && cl.maxa > 0.0 {
            if cl.maxa < (*w as f32) / (*h as f32) { *w = (*h as f32 * cl.maxa + 0.5) as i32; }
            else if cl.mina < (*h as f32) / (*w as f32) { *h = (*w as f32 * cl.mina + 0.5) as i32; }
        }
        if baseismin { *w -= cl.basew; *h -= cl.baseh; }
        if cl.incw != 0 { *w -= *w % cl.incw; }
        if cl.inch != 0 { *h -= *h % cl.inch; }
        *w += cl.basew; *h += cl.baseh;
        *w = (*w).max(cl.minw);
        *h = (*h).max(cl.minh);
        if cl.maxw != 0 { *w = (*w).min(cl.maxw); }
        if cl.maxh != 0 { *h = (*h).min(cl.maxh); }
    }
    *x != cl.x || *y != cl.y || *w != cl.w || *h != cl.h
}

fn resize(st: &mut State, c: CId, mut x: i32, mut y: i32,
          mut w: i32, mut h: i32, interact: bool) -> Result<(), Box<dyn Error>>
{
    if applysizehints(st, c, &mut x, &mut y, &mut w, &mut h, interact) {
        resize_client(st, c, x, y, w, h)?;
    }
    Ok(())
}

fn resize_client(st: &mut State, c: CId, x: i32, y: i32, w: i32, h: i32)
    -> Result<(), Box<dyn Error>>
{
    let cl = &mut st.clients[c];
    cl.oldx = cl.x; cl.x = x;
    cl.oldy = cl.y; cl.y = y;
    cl.oldw = cl.w; cl.w = w;
    cl.oldh = cl.h; cl.h = h;
    let bw = cl.bw;
    let win = cl.win;
    st.conn.configure_window(win, &ConfigureWindowAux::default()
        .x(x).y(y).width(w as u32).height(h as u32).border_width(bw as u32))?;
    configure(st, c)?;
    st.conn.flush()?;
    Ok(())
}

fn configure(st: &State, c: CId) -> Result<(), Box<dyn Error>> {
    let cl = &st.clients[c];
    let ev = ConfigureNotifyEvent {
        response_type: x11rb::protocol::xproto::CONFIGURE_NOTIFY_EVENT,
        sequence: 0, event: cl.win, window: cl.win, above_sibling: 0,
        x: cl.x as i16, y: cl.y as i16, width: cl.w as u16, height: cl.h as u16,
        border_width: cl.bw as u16, override_redirect: false,
    };
    st.conn.send_event(false, cl.win, EventMask::STRUCTURE_NOTIFY, ev)?;
    Ok(())
}

fn focus(st: &mut State, c: Option<CId>) -> Result<(), Box<dyn Error>> {
    let mon = match st.selmon { Some(m) => m, None => return Ok(()) };
    let mut c = c;
    if c.is_none() || !c.map(|ci| isvisible(&st.clients[ci], &st.mons[mon])).unwrap_or(false) {
        let mut t = st.mons[mon].stack;
        while let Some(ti) = t {
            if isvisible(&st.clients[ti], &st.mons[mon]) { break; }
            t = st.clients[ti].snext;
        }
        c = t;
    }
    let prev_sel = st.mons[mon].sel;
    if let Some(ps) = prev_sel {
        if Some(ps) != c {
            unfocus(st, ps, false)?;
        }
    }
    if let Some(ci) = c {
        let cmon = st.clients[ci].mon;
        if cmon != mon { st.selmon = Some(cmon); }
        if st.clients[ci].isurgent { clear_urgent(st, ci)?; }
        detach_stack(st, ci);
        attach_stack(st, ci);
        grabbuttons(st, ci, true)?;
        let win = st.clients[ci].win;
        let border = st.dc.sel[COL_BORDER];
        st.conn.change_window_attributes(win, &ChangeWindowAttributesAux::default()
            .border_pixel(border))?;
        st.conn.set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?;
    } else {
        st.conn.set_input_focus(InputFocus::POINTER_ROOT, st.root, CURRENT_TIME)?;
    }
    if let Some(m) = st.selmon { st.mons[m].sel = c; }
    drawbars(st)?;
    Ok(())
}

fn unfocus(st: &mut State, c: CId, setfocus: bool) -> Result<(), Box<dyn Error>> {
    grabbuttons(st, c, false)?;
    let win = st.clients[c].win;
    let border = st.dc.norm[COL_BORDER];
    st.conn.change_window_attributes(win, &ChangeWindowAttributesAux::default()
        .border_pixel(border))?;
    if setfocus {
        st.conn.set_input_focus(InputFocus::POINTER_ROOT, st.root, CURRENT_TIME)?;
    }
    Ok(())
}

fn clear_urgent(st: &mut State, c: CId) -> Result<(), Box<dyn Error>> {
    st.clients[c].isurgent = false;
    let win = st.clients[c].win;
    let r = st.conn.get_property(false, win, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 9)?.reply();
    if let Ok(r) = r {
        if r.value32().map(|i| i.count()).unwrap_or(0) >= 1 {
            let mut data: Vec<u32> = r.value32().unwrap().collect();
            if !data.is_empty() {
                data[0] &= !(1 << 8); // XUrgencyHint = (1<<8)
                st.conn.change_property32(PropMode::REPLACE, win,
                    AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &data)?;
            }
        }
    }
    Ok(())
}

fn keysym_to_keycode(st: &State, ks: u32) -> Option<u8> {
    if st.keysyms_per_keycode == 0 { return None; }
    let kpsym = st.keysyms_per_keycode as usize;
    for (i, chunk) in st.keysym_table.chunks(kpsym).enumerate() {
        if chunk.iter().any(|&k| k == ks) {
            return Some(st.min_keycode + i as u8);
        }
    }
    None
}

fn keycode_to_keysym(st: &State, kc: u8, group: usize) -> u32 {
    if kc < st.min_keycode { return 0; }
    let off = (kc - st.min_keycode) as usize * st.keysyms_per_keycode as usize;
    if off >= st.keysym_table.len() { return 0; }
    let take = (st.keysyms_per_keycode as usize).min(st.keysym_table.len() - off);
    st.keysym_table.get(off..off + take)
        .and_then(|g| g.get(group).copied())
        .unwrap_or(0)
}

fn update_numlockmask(st: &mut State) -> Result<(), Box<dyn Error>> {
    st.numlockmask = 0;
    let r = st.conn.get_modifier_mapping()?.reply()?;
    let kc_numlock = keysym_to_keycode(st, XK_Num_Lock);
    let total = r.keycodes.len();
    let kpm = if total == 0 { 0 } else { total / 8 };
    for i in 0..8 {
        for j in 0..kpm {
            let kc = r.keycodes[i * kpm + j];
            if Some(kc) == kc_numlock { st.numlockmask = 1u16 << i; }
        }
    }
    Ok(())
}

fn grabkeys(st: &mut State) -> Result<(), Box<dyn Error>> {
    update_numlockmask(st)?;
    let modifiers = [0u16, LOCK_M, st.numlockmask, st.numlockmask | LOCK_M];
    st.conn.ungrab_key(0u8, st.root, ModMask::ANY)?;
    for k in build_keys() {
        if let Some(code) = keysym_to_keycode(st, k.keysym) {
            for m in modifiers {
                let mods = ModMask::from(k.mod_ | m);
                st.conn.grab_key(true, st.root, mods, code,
                    GrabMode::ASYNC, GrabMode::ASYNC)?;
            }
        }
    }
    Ok(())
}

fn grabbuttons(st: &mut State, c: CId, focused: bool) -> Result<(), Box<dyn Error>> {
    update_numlockmask(st)?;
    let modifiers = [0u16, LOCK_M, st.numlockmask, st.numlockmask | LOCK_M];
    let win = st.clients[c].win;
    st.conn.ungrab_button(ButtonIndex::ANY, win, ModMask::ANY)?;
    if !focused {
        st.conn.grab_button(false, win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC, GrabMode::SYNC, 0u32, 0u32,
            ButtonIndex::ANY, ModMask::ANY)?;
    } else {
        for b in build_buttons() {
            if b.click != CLK_CLIENT_WIN { continue; }
            for m in modifiers {
                st.conn.grab_button(false, win,
                    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                    GrabMode::ASYNC, GrabMode::SYNC, 0u32, 0u32,
                    ButtonIndex::from(b.button),
                    ModMask::from(b.mask | m))?;
            }
        }
    }
    Ok(())
}

fn applyrules(st: &mut State, c: CId) -> Result<(), Box<dyn Error>> {
    let win = st.clients[c].win;
    st.clients[c].isfloating = false;
    st.clients[c].tags = 0;
    let class_hint = st.conn.get_property(false, win, AtomEnum::WM_CLASS,
        AtomEnum::STRING, 0, 1024)?.reply().ok();
    let (class_b, instance_b) = match class_hint {
        Some(r) if !r.value.is_empty() => {
            // WM_CLASS is two consecutive null-terminated strings: instance, class.
            let mut parts = r.value.split(|&b| b == 0);
            let inst = parts.next().unwrap_or(&[]).to_vec();
            let cls = parts.next().unwrap_or(&[]).to_vec();
            (cls, inst)
        }
        _ => (BROKEN.to_vec(), BROKEN.to_vec()),
    };
    let name = st.clients[c].name.clone();
    for r in RULES {
        let class_match = r.class.map_or(true, |s| sub_contains(&class_b, s));
        let instance_match = r.instance.map_or(true, |s| sub_contains(&instance_b, s));
        let title_match = r.title.map_or(true, |s| sub_contains(&name, s));
        if class_match && instance_match && title_match {
            st.clients[c].isfloating = r.isfloating;
            st.clients[c].tags |= r.tags;
            if r.monitor >= 0 {
                for m in mon_list(st) {
                    if st.mons[m].num == r.monitor {
                        st.clients[c].mon = m;
                        break;
                    }
                }
            }
        }
    }
    let mon = st.clients[c].mon;
    let masked = st.clients[c].tags & TAG_MASK;
    st.clients[c].tags = if masked != 0 { masked } else {
        st.mons[mon].tagset[st.mons[mon].seltags]
    };
    Ok(())
}

fn sub_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() { return true; }
    if needle.len() > haystack.len() { return false; }
    for i in 0..=haystack.len() - needle.len() {
        if haystack[i..i + needle.len()] == *needle { return true; }
    }
    false
}

fn update_title(st: &mut State, c: CId) -> Result<(), Box<dyn Error>> {
    let win = st.clients[c].win;
    let net_name = st.netatom[NET_WM_NAME];
    let mut name = get_text_prop(st, win, net_name)?;
    if name.is_empty() {
        name = get_text_prop(st, win, AtomEnum::WM_NAME.into())?;
    }
    if name.is_empty() {
        name = BROKEN.to_vec();
    }
    st.clients[c].name = draw::utf8_to_drawable(&name);
    Ok(())
}

fn update_size_hints(st: &mut State, c: CId) -> Result<(), Box<dyn Error>> {
    let win = st.clients[c].win;
    // ICCCM WM_NORMAL_HINTS: 18 32-bit values
    let r = st.conn.get_property(false, win, AtomEnum::WM_NORMAL_HINTS,
        AtomEnum::WM_SIZE_HINTS, 0, 18)?.reply().ok();
    let cl = &mut st.clients[c];
    cl.basew = 0; cl.baseh = 0;
    cl.incw = 0; cl.inch = 0;
    cl.maxw = 0; cl.maxh = 0;
    cl.minw = 0; cl.minh = 0;
    cl.mina = 0.0; cl.maxa = 0.0;
    let words: Vec<u32> = r.and_then(|r| r.value32().map(|i| i.collect())).unwrap_or_default();
    if words.len() >= 18 {
        let flags = words[0];
        // PMinSize=16, PMaxSize=32, PResizeInc=64, PAspect=128, PBaseSize=256
        let p_min = (flags & 16) != 0;
        let p_max = (flags & 32) != 0;
        let p_inc = (flags & 64) != 0;
        let p_aspect = (flags & 128) != 0;
        let p_base = (flags & 256) != 0;
        if p_base {
            cl.basew = words[7] as i32; cl.baseh = words[8] as i32;
        } else if p_min {
            cl.basew = words[5] as i32; cl.baseh = words[6] as i32;
        }
        if p_inc { cl.incw = words[9] as i32; cl.inch = words[10] as i32; }
        if p_max { cl.maxw = words[11] as i32; cl.maxh = words[12] as i32; }
        if p_min { cl.minw = words[5] as i32; cl.minh = words[6] as i32; }
        else if p_base { cl.minw = words[7] as i32; cl.minh = words[8] as i32; }
        if p_aspect {
            let mina_x = words[13] as i32; let mina_y = words[14] as i32;
            let maxa_x = words[15] as i32; let maxa_y = words[16] as i32;
            if mina_x != 0 { cl.mina = mina_y as f32 / mina_x as f32; }
            if maxa_y != 0 { cl.maxa = maxa_x as f32 / maxa_y as f32; }
        }
    }
    cl.isfixed = cl.maxw != 0 && cl.minw != 0 && cl.maxh != 0 && cl.minh != 0
        && cl.maxw == cl.minw && cl.maxh == cl.minh;
    Ok(())
}

fn update_wm_hints(st: &mut State, c: CId) -> Result<(), Box<dyn Error>> {
    let win = st.clients[c].win;
    let r = st.conn.get_property(false, win, AtomEnum::WM_HINTS,
        AtomEnum::WM_HINTS, 0, 9)?.reply().ok();
    let words: Vec<u32> = r.and_then(|r| r.value32().map(|i| i.collect())).unwrap_or_default();
    if words.len() >= 1 {
        let urgency_bit = 1u32 << 8;
        let urgent = (words[0] & urgency_bit) != 0;
        if Some(c) == st.selmon.and_then(|m| st.mons[m].sel) && urgent {
            // Clear the urgent bit
            let mut data = words.clone();
            data[0] &= !urgency_bit;
            st.conn.change_property32(PropMode::REPLACE, win,
                AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, &data)?;
        } else {
            st.clients[c].isurgent = urgent;
        }
    }
    Ok(())
}

fn set_client_state(st: &State, c: CId, state: u32) -> Result<(), Box<dyn Error>> {
    let win = st.clients[c].win;
    let data: [u32; 2] = [state, 0];
    st.conn.change_property32(PropMode::REPLACE, win,
        st.wmatom[WM_STATE], st.wmatom[WM_STATE], &data)?;
    Ok(())
}

fn get_state(st: &State, w: Window) -> Option<u32> {
    let r = st.conn.get_property(false, w, st.wmatom[WM_STATE],
        st.wmatom[WM_STATE], 0, 2).ok()?.reply().ok()?;
    let mut it = r.value32()?;
    it.next()
}

fn isproto_del(st: &State, c: CId) -> Result<bool, Box<dyn Error>> {
    let win = st.clients[c].win;
    let r = st.conn.get_property(false, win, st.wmatom[WM_PROTOCOLS],
        AtomEnum::ATOM, 0, 32)?.reply().ok();
    if let Some(r) = r {
        if let Some(it) = r.value32() {
            for a in it {
                if a == st.wmatom[WM_DELETE] { return Ok(true); }
            }
        }
    }
    Ok(false)
}

fn manage(st: &mut State, w: Window, wa: &GetWindowAttributesReply,
          geom: &GetGeometryReply) -> Result<(), Box<dyn Error>>
{
    let _ = wa;
    let mut c = Client::default();
    c.win = w;
    c.x = geom.x as i32;
    c.y = geom.y as i32;
    c.w = geom.width as i32;
    c.h = geom.height as i32;
    c.oldbw = geom.border_width as i32;

    // Transient hint: the parent client's monitor & tags.
    let trans = st.conn.get_property(false, w, AtomEnum::WM_TRANSIENT_FOR,
        AtomEnum::WINDOW, 0, 1)?.reply().ok()
        .and_then(|r| r.value32().and_then(|mut it| it.next()));
    let parent = trans.and_then(|tw| wintoclient(st, tw));
    if let Some(pc) = parent {
        c.mon = st.clients[pc].mon;
        c.tags = st.clients[pc].tags;
    } else {
        c.mon = st.selmon.unwrap_or(0);
    }
    let cid = alloc_client(st, c);
    update_title(st, cid)?;
    if parent.is_none() {
        applyrules(st, cid)?;
    }

    let mon = st.clients[cid].mon;
    st.clients[cid].oldx = st.clients[cid].x;
    st.clients[cid].oldy = st.clients[cid].y;
    st.clients[cid].oldw = st.clients[cid].w;
    st.clients[cid].oldh = st.clients[cid].h;
    st.clients[cid].x += st.mons[mon].wx;
    st.clients[cid].y += st.mons[mon].wy;
    let (mw, mh) = (st.mons[mon].mw, st.mons[mon].mh);
    let (mx, my) = (st.mons[mon].mx, st.mons[mon].my);
    if st.clients[cid].w == mw && st.clients[cid].h == mh {
        st.clients[cid].isfloating = true;
        st.clients[cid].x = mx;
        st.clients[cid].y = my;
        st.clients[cid].bw = 0;
    } else {
        let bh = st.bh;
        let cl = &mut st.clients[cid];
        if cl.x + cl.w + 2 * cl.bw > mx + mw { cl.x = mx + mw - (cl.w + 2 * cl.bw); }
        if cl.y + cl.h + 2 * cl.bw > my + mh { cl.y = my + mh - (cl.h + 2 * cl.bw); }
        cl.x = cl.x.max(mx);
        cl.y = cl.y.max(my);
        // Adjust if window center might cover the bar.
        let wx = st.mons[mon].wx;
        let ww = st.mons[mon].ww;
        let by = st.mons[mon].by;
        let cl = &mut st.clients[cid];
        if by == 0 && cl.x + (cl.w / 2) >= wx && cl.x + (cl.w / 2) < wx + ww {
            cl.y = cl.y.max(bh);
        } else {
            cl.y = cl.y.max(my);
        }
        cl.bw = BORDERPX as i32;
    }
    let bw = st.clients[cid].bw;
    let border = st.dc.norm[COL_BORDER];
    st.conn.configure_window(w, &ConfigureWindowAux::default()
        .border_width(bw as u32))?;
    st.conn.change_window_attributes(w, &ChangeWindowAttributesAux::default()
        .border_pixel(border))?;
    configure(st, cid)?;
    update_size_hints(st, cid)?;
    st.conn.change_window_attributes(w, &ChangeWindowAttributesAux::default()
        .event_mask(EventMask::ENTER_WINDOW | EventMask::FOCUS_CHANGE
            | EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY))?;
    grabbuttons(st, cid, false)?;
    if !st.clients[cid].isfloating {
        let trans_or_fixed = parent.is_some() || st.clients[cid].isfixed;
        st.clients[cid].isfloating = trans_or_fixed;
        st.clients[cid].oldstate = trans_or_fixed;
    }
    if st.clients[cid].isfloating {
        st.conn.configure_window(w,
            &ConfigureWindowAux::default().stack_mode(StackMode::ABOVE))?;
    }
    attach(st, cid);
    attach_stack(st, cid);
    let off_x = st.clients[cid].x + 2 * st.sw;
    let cy = st.clients[cid].y;
    let cw = st.clients[cid].w;
    let ch = st.clients[cid].h;
    st.conn.configure_window(w, &ConfigureWindowAux::default()
        .x(off_x).y(cy).width(cw as u32).height(ch as u32))?;
    st.conn.map_window(w)?;
    set_client_state(st, cid, NORMAL_STATE)?;
    let mid = st.clients[cid].mon;
    arrange(st, Some(mid))?;
    Ok(())
}

fn unmanage(st: &mut State, c: CId, destroyed: bool) -> Result<(), Box<dyn Error>> {
    let m = st.clients[c].mon;
    let win = st.clients[c].win;
    let oldbw = st.clients[c].oldbw;
    detach(st, c);
    detach_stack(st, c);
    if !destroyed {
        st.conn.grab_server()?;
        let _ = st.conn.configure_window(win,
            &ConfigureWindowAux::default().border_width(oldbw as u32));
        let _ = st.conn.ungrab_button(ButtonIndex::ANY, win, ModMask::ANY);
        set_client_state(st, c, WITHDRAWN_STATE)?;
        st.conn.flush()?;
        st.conn.ungrab_server()?;
    }
    st.clients[c].alive = false;
    focus(st, None)?;
    arrange(st, Some(m))?;
    Ok(())
}

fn scan(st: &mut State) -> Result<(), Box<dyn Error>> {
    let r = st.conn.query_tree(st.root)?.reply()?;
    let wins = r.children;
    // First pass: non-transient
    let mut transients = Vec::new();
    for &w in &wins {
        let wa = match st.conn.get_window_attributes(w)?.reply() { Ok(r) => r, _ => continue };
        if wa.override_redirect { continue; }
        let trans_for = st.conn.get_property(false, w, AtomEnum::WM_TRANSIENT_FOR,
            AtomEnum::WINDOW, 0, 1)?.reply().ok()
            .and_then(|r| r.value32().and_then(|mut i| i.next()));
        if trans_for.is_some() { transients.push(w); continue; }
        if wa.map_state == MapState::VIEWABLE
            || get_state(st, w) == Some(ICONIC_STATE)
        {
            let geom = match st.conn.get_geometry(w)?.reply() { Ok(r) => r, _ => continue };
            manage(st, w, &wa, &geom)?;
        }
    }
    for w in transients {
        let wa = match st.conn.get_window_attributes(w)?.reply() { Ok(r) => r, _ => continue };
        if wa.map_state == MapState::VIEWABLE
            || get_state(st, w) == Some(ICONIC_STATE)
        {
            let geom = match st.conn.get_geometry(w)?.reply() { Ok(r) => r, _ => continue };
            manage(st, w, &wa, &geom)?;
        }
    }
    Ok(())
}

fn event_loop(st: &mut State) -> Result<(), Box<dyn Error>> {
    st.conn.flush()?;
    while st.running {
        let ev = match st.conn.wait_for_event() {
            Ok(e) => e,
            Err(_) => break,
        };
        if let Err(e) = dispatch_event(st, ev) {
            eprintln!("dwm: event error: {}", e);
        }
        st.conn.flush()?;
    }
    Ok(())
}

fn dispatch_event(st: &mut State, ev: Event) -> Result<(), Box<dyn Error>> {
    match ev {
        Event::ButtonPress(e)      => button_press(st, e),
        Event::ClientMessage(e)    => client_message(st, e),
        Event::ConfigureRequest(e) => configure_request(st, e),
        Event::ConfigureNotify(e)  => configure_notify(st, e),
        Event::DestroyNotify(e)    => destroy_notify(st, e),
        Event::EnterNotify(e)      => enter_notify(st, e),
        Event::Expose(e)           => expose(st, e),
        Event::FocusIn(e)          => focus_in(st, e),
        Event::KeyPress(e)         => key_press(st, e),
        Event::MappingNotify(e)    => mapping_notify(st, e),
        Event::MapRequest(e)       => map_request(st, e),
        Event::PropertyNotify(e)   => property_notify(st, e),
        Event::UnmapNotify(e)      => unmap_notify(st, e),
        _ => Ok(()),
    }
}

fn button_press(st: &mut State, ev: ButtonPressEvent) -> Result<(), Box<dyn Error>> {
    let mut click = CLK_ROOT_WIN;
    let mut arg = Arg::None;
    if let Some(m) = wintomon(st, ev.event) {
        if Some(m) != st.selmon {
            if let Some(s) = st.selmon.and_then(|sm| st.mons[sm].sel) {
                unfocus(st, s, true)?;
            }
            st.selmon = Some(m);
            focus(st, None)?;
        }
    }
    let selmon = st.selmon;
    if selmon.is_some() && Some(ev.event) == selmon.map(|m| st.mons[m].barwin) {
        let mut x = 0i32;
        let mut tag_idx = TAGS.len();
        for (i, &t) in TAGS.iter().enumerate() {
            x += draw::textw(&st.dc, t);
            if (ev.event_x as i32) < x { tag_idx = i; break; }
        }
        if tag_idx < TAGS.len() {
            click = CLK_TAGBAR;
            arg = Arg::U(1u32 << tag_idx);
        } else if (ev.event_x as i32) < x + st.blw {
            click = CLK_LT_SYMBOL;
        } else {
            let m = selmon.unwrap();
            let stext_w = draw::textw(&st.dc, &st.stext);
            let ww = st.mons[m].ww;
            let wx = st.mons[m].wx;
            if (ev.event_x as i32) > wx + ww - stext_w {
                click = CLK_STATUS_TEXT;
            } else {
                click = CLK_WIN_TITLE;
            }
        }
    } else if let Some(c) = wintoclient(st, ev.event) {
        focus(st, Some(c))?;
        click = CLK_CLIENT_WIN;
    }
    let state_clean = cleanmask(st, ev.state.into());
    for b in build_buttons() {
        if b.click == click && b.button == ev.detail
            && cleanmask(st, b.mask) == state_clean
        {
            let final_arg = if click == CLK_TAGBAR {
                if matches!(b.arg, Arg::None) { arg.clone() } else { b.arg.clone() }
            } else { b.arg.clone() };
            (b.func)(st, &final_arg);
        }
    }
    Ok(())
}

fn key_press(st: &mut State, ev: KeyPressEvent) -> Result<(), Box<dyn Error>> {
    let ksym = keycode_to_keysym(st, ev.detail, 0);
    let state_clean = cleanmask(st, ev.state.into());
    for k in build_keys() {
        if ksym == k.keysym && cleanmask(st, k.mod_) == state_clean {
            (k.func)(st, &k.arg);
        }
    }
    Ok(())
}

fn expose(st: &mut State, ev: ExposeEvent) -> Result<(), Box<dyn Error>> {
    if ev.count == 0 {
        if let Some(m) = wintomon(st, ev.window) {
            drawbar(st, m)?;
        }
    }
    Ok(())
}

fn enter_notify(st: &mut State, ev: EnterNotifyEvent) -> Result<(), Box<dyn Error>> {
    if (ev.mode != NotifyMode::NORMAL || ev.detail == NotifyDetail::INFERIOR)
        && ev.event != st.root
    {
        return Ok(());
    }
    let m = wintomon(st, ev.event);
    if m != st.selmon {
        if let Some(s) = st.selmon.and_then(|sm| st.mons[sm].sel) {
            unfocus(st, s, true)?;
        }
        st.selmon = m;
    }
    let c = wintoclient(st, ev.event);
    focus(st, c)?;
    Ok(())
}

fn focus_in(st: &mut State, ev: FocusInEvent) -> Result<(), Box<dyn Error>> {
    if let Some(m) = st.selmon {
        if let Some(s) = st.mons[m].sel {
            let win = st.clients[s].win;
            if ev.event != win {
                st.conn.set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?;
            }
        }
    }
    Ok(())
}

fn destroy_notify(st: &mut State, ev: DestroyNotifyEvent) -> Result<(), Box<dyn Error>> {
    if let Some(c) = wintoclient(st, ev.window) {
        unmanage(st, c, true)?;
    }
    Ok(())
}

fn unmap_notify(st: &mut State, ev: UnmapNotifyEvent) -> Result<(), Box<dyn Error>> {
    if let Some(c) = wintoclient(st, ev.window) {
        unmanage(st, c, false)?;
    }
    Ok(())
}

fn map_request(st: &mut State, ev: MapRequestEvent) -> Result<(), Box<dyn Error>> {
    let wa = match st.conn.get_window_attributes(ev.window)?.reply() { Ok(r) => r, _ => return Ok(()) };
    if wa.override_redirect { return Ok(()); }
    if wintoclient(st, ev.window).is_some() { return Ok(()); }
    let geom = match st.conn.get_geometry(ev.window)?.reply() { Ok(r) => r, _ => return Ok(()) };
    manage(st, ev.window, &wa, &geom)?;
    Ok(())
}

fn mapping_notify(st: &mut State, ev: MappingNotifyEvent) -> Result<(), Box<dyn Error>> {
    if ev.request == Mapping::KEYBOARD {
        load_keymap(st)?;
        grabkeys(st)?;
    }
    Ok(())
}

fn configure_request(st: &mut State, ev: ConfigureRequestEvent) -> Result<(), Box<dyn Error>> {
    let cwx       = u16::from(ConfigWindow::X);
    let cwy       = u16::from(ConfigWindow::Y);
    let cww       = u16::from(ConfigWindow::WIDTH);
    let cwh       = u16::from(ConfigWindow::HEIGHT);
    let cwbw      = u16::from(ConfigWindow::BORDER_WIDTH);
    let cwsib     = u16::from(ConfigWindow::SIBLING);
    let cwsm      = u16::from(ConfigWindow::STACK_MODE);
    let mask: u16 = ev.value_mask.into();
    if let Some(c) = wintoclient(st, ev.window) {
        if (mask & cwbw) != 0 {
            st.clients[c].bw = ev.border_width as i32;
        } else {
            let mon = st.clients[c].mon;
            let lt = st.mons[mon].lt[st.mons[mon].sellt];
            let cl_floating = st.clients[c].isfloating;
            let arrange_none = LAYOUTS[lt].arrange.is_none();
            if cl_floating || arrange_none {
                let mx = st.mons[mon].mx;
                let my = st.mons[mon].my;
                let mw = st.mons[mon].mw;
                let mh = st.mons[mon].mh;
                let cl = &mut st.clients[c];
                if (mask & cwx) != 0 { cl.x = mx + ev.x as i32; }
                if (mask & cwy) != 0 { cl.y = my + ev.y as i32; }
                if (mask & cww) != 0 { cl.w = ev.width as i32; }
                if (mask & cwh) != 0 { cl.h = ev.height as i32; }
                if cl.x + cl.w > mx + mw && cl.isfloating { cl.x = mx + mw / 2 - cl.w / 2; }
                if cl.y + cl.h > my + mh && cl.isfloating { cl.y = my + mh / 2 - cl.h / 2; }
                if (mask & (cwx | cwy)) != 0 && (mask & (cww | cwh)) == 0 {
                    configure(st, c)?;
                }
                let cl = &st.clients[c];
                let m_ref = &st.mons[cl.mon];
                if isvisible(cl, m_ref) {
                    let win = cl.win;
                    let aux = ConfigureWindowAux::default()
                        .x(cl.x).y(cl.y).width(cl.w as u32).height(cl.h as u32);
                    st.conn.configure_window(win, &aux)?;
                }
            } else {
                configure(st, c)?;
            }
        }
    } else {
        let mut aux = ConfigureWindowAux::default();
        if (mask & cwx) != 0 { aux = aux.x(ev.x as i32); }
        if (mask & cwy) != 0 { aux = aux.y(ev.y as i32); }
        if (mask & cww) != 0 { aux = aux.width(ev.width as u32); }
        if (mask & cwh) != 0 { aux = aux.height(ev.height as u32); }
        if (mask & cwbw) != 0 { aux = aux.border_width(ev.border_width as u32); }
        if (mask & cwsib) != 0 { aux = aux.sibling(ev.sibling); }
        if (mask & cwsm) != 0 { aux = aux.stack_mode(ev.stack_mode); }
        st.conn.configure_window(ev.window, &aux)?;
    }
    st.conn.flush()?;
    Ok(())
}

fn configure_notify(st: &mut State, ev: ConfigureNotifyEvent) -> Result<(), Box<dyn Error>> {
    if ev.window != st.root { return Ok(()); }
    st.sw = ev.width as i32;
    st.sh = ev.height as i32;
    if update_geom(st)? {
        // resize the bar drawable
        let depth = st.conn.setup().roots[st.screen_num].root_depth;
        if st.dc.drawable != 0 {
            st.conn.free_pixmap(st.dc.drawable)?;
        }
        let pid = st.conn.generate_id()?;
        st.conn.create_pixmap(depth, pid, st.root,
            st.sw.max(1) as u16, st.bh.max(1) as u16)?;
        st.dc.drawable = pid;
        st.dc.drawable_w = st.sw.max(1) as u16;
        st.dc.drawable_h = st.bh.max(1) as u16;
        update_bars(st)?;
        for m in mon_list(st) {
            let bar = st.mons[m].barwin;
            let wx = st.mons[m].wx;
            let by = st.mons[m].by;
            let ww = st.mons[m].ww;
            let bh = st.bh;
            if bar != 0 {
                st.conn.configure_window(bar, &ConfigureWindowAux::default()
                    .x(wx).y(by).width(ww as u32).height(bh as u32))?;
            }
        }
        arrange(st, None)?;
    }
    Ok(())
}

fn property_notify(st: &mut State, ev: PropertyNotifyEvent) -> Result<(), Box<dyn Error>> {
    if ev.window == st.root && ev.atom == AtomEnum::WM_NAME.into() {
        update_status(st)?;
        return Ok(());
    }
    if ev.state == Property::DELETE { return Ok(()); }
    let c = match wintoclient(st, ev.window) { Some(c) => c, None => return Ok(()) };
    if ev.atom == AtomEnum::WM_TRANSIENT_FOR.into() {
        let trans = st.conn.get_property(false, ev.window, AtomEnum::WM_TRANSIENT_FOR,
            AtomEnum::WINDOW, 0, 1)?.reply().ok()
            .and_then(|r| r.value32().and_then(|mut i| i.next()));
        if !st.clients[c].isfloating && trans.is_some()
            && wintoclient(st, trans.unwrap()).is_some()
        {
            st.clients[c].isfloating = true;
            let mid = st.clients[c].mon;
            arrange(st, Some(mid))?;
        }
    } else if ev.atom == AtomEnum::WM_NORMAL_HINTS.into() {
        update_size_hints(st, c)?;
    } else if ev.atom == AtomEnum::WM_HINTS.into() {
        update_wm_hints(st, c)?;
        drawbars(st)?;
    }
    if ev.atom == AtomEnum::WM_NAME.into() || ev.atom == st.netatom[NET_WM_NAME] {
        update_title(st, c)?;
        let mon = st.clients[c].mon;
        if Some(c) == st.mons[mon].sel { drawbar(st, mon)?; }
    }
    Ok(())
}

fn client_message(st: &mut State, ev: ClientMessageEvent) -> Result<(), Box<dyn Error>> {
    let c = match wintoclient(st, ev.window) { Some(c) => c, None => return Ok(()) };
    if ev.type_ != st.netatom[NET_WM_STATE] { return Ok(()); }
    let data = ev.data.as_data32();
    if data[1] != st.netatom[NET_WM_FULLSCREEN] && data[2] != st.netatom[NET_WM_FULLSCREEN] {
        return Ok(());
    }
    if data[0] != 0 {
        let win = st.clients[c].win;
        st.conn.change_property32(PropMode::REPLACE, win,
            st.netatom[NET_WM_STATE], AtomEnum::ATOM,
            &[st.netatom[NET_WM_FULLSCREEN]])?;
        let cl = &mut st.clients[c];
        cl.oldstate = cl.isfloating;
        cl.oldbw = cl.bw;
        cl.bw = 0;
        cl.isfloating = true;
        let mon = cl.mon;
        let (mx, my, mw, mh) = (st.mons[mon].mx, st.mons[mon].my,
                                st.mons[mon].mw, st.mons[mon].mh);
        resize_client(st, c, mx, my, mw, mh)?;
        st.conn.configure_window(win,
            &ConfigureWindowAux::default().stack_mode(StackMode::ABOVE))?;
    } else {
        let win = st.clients[c].win;
        st.conn.change_property32(PropMode::REPLACE, win,
            st.netatom[NET_WM_STATE], AtomEnum::ATOM, &[] as &[u32])?;
        let cl = &mut st.clients[c];
        cl.isfloating = cl.oldstate;
        cl.bw = cl.oldbw;
        cl.x = cl.oldx; cl.y = cl.oldy;
        cl.w = cl.oldw; cl.h = cl.oldh;
        let (cx, cy, cw, ch) = (cl.x, cl.y, cl.w, cl.h);
        resize_client(st, c, cx, cy, cw, ch)?;
        let mon = st.clients[c].mon;
        arrange(st, Some(mon))?;
    }
    Ok(())
}

// ===== Actions (key/button handlers) =====

fn spawn(_st: &mut State, arg: &Arg) {
    if let Arg::Cmd(cmd) = arg {
        if cmd.is_empty() { return; }
        unsafe {
            let pid = libc::fork();
            if pid == 0 {
                libc::setsid();
                let mut owned: Vec<std::ffi::CString> = cmd.iter()
                    .map(|s| std::ffi::CString::new(*s).unwrap()).collect();
                let mut argv: Vec<*const libc::c_char> = owned.iter_mut()
                    .map(|s| s.as_ptr()).collect();
                argv.push(std::ptr::null());
                libc::execvp(argv[0], argv.as_ptr());
                libc::_exit(1);
            }
        }
    }
}

fn quit(st: &mut State, _: &Arg) { st.running = false; }

fn togglebar(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    st.mons[m].showbar = !st.mons[m].showbar;
    let curtag = st.mons[m].curtag;
    let sb = st.mons[m].showbar;
    st.mons[m].showbars[curtag] = sb;
    update_bar_pos(st, m);
    let bar = st.mons[m].barwin;
    let wx = st.mons[m].wx;
    let by = st.mons[m].by;
    let ww = st.mons[m].ww;
    let bh = st.bh;
    if bar != 0 {
        let _ = st.conn.configure_window(bar, &ConfigureWindowAux::default()
            .x(wx).y(by).width(ww as u32).height(bh as u32));
    }
    let _ = arrange(st, Some(m));
}

fn focusstack(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let sel = match st.mons[m].sel { Some(s) => s, None => return };
    let lt = st.mons[m].lt[st.mons[m].sellt];
    if st.clients[sel].isfixed && LAYOUTS[lt].arrange.is_some() { return; }
    let dir = if let Arg::I(i) = arg { *i } else { 0 };
    let mut next: Option<CId> = None;
    if dir > 0 {
        let mut c = st.clients[sel].next;
        while let Some(ci) = c {
            if isvisible(&st.clients[ci], &st.mons[m]) { next = Some(ci); break; }
            c = st.clients[ci].next;
        }
        if next.is_none() {
            let mut c = st.mons[m].clients;
            while let Some(ci) = c {
                if isvisible(&st.clients[ci], &st.mons[m]) { next = Some(ci); break; }
                c = st.clients[ci].next;
            }
        }
    } else {
        let mut c = st.mons[m].clients;
        while let Some(ci) = c {
            if Some(ci) == Some(sel) { break; }
            if isvisible(&st.clients[ci], &st.mons[m]) { next = Some(ci); }
            c = st.clients[ci].next;
        }
        if next.is_none() {
            // wrap to last visible
            let mut c = st.mons[m].clients;
            while let Some(ci) = c {
                if isvisible(&st.clients[ci], &st.mons[m]) { next = Some(ci); }
                c = st.clients[ci].next;
            }
        }
    }
    if let Some(n) = next {
        let _ = focus(st, Some(n));
        let _ = restack(st, m);
    }
}

fn setmfact(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let lt = st.mons[m].lt[st.mons[m].sellt];
    if LAYOUTS[lt].arrange.is_none() { return; }
    let f = if let Arg::F(f) = arg { *f } else { return };
    let new = if f < 1.0 { f + st.mons[m].mfact } else { f - 1.0 };
    if new < 0.1 || new > 0.9 { return; }
    st.mons[m].mfact = new;
    let curtag = st.mons[m].curtag;
    st.mons[m].mfacts[curtag] = new;
    let _ = arrange(st, Some(m));
}

fn zoom(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let mut c = st.mons[m].sel;
    let lt = st.mons[m].lt[st.mons[m].sellt];
    if LAYOUTS[lt].arrange.is_none()
        || c.map(|ci| st.clients[ci].isfloating).unwrap_or(true)
    { return; }
    let first = nexttiled(st, st.mons[m].clients);
    if first == c {
        c = nexttiled(st, st.clients[c.unwrap()].next);
    }
    if let Some(ci) = c { pop(st, ci); }
}

fn pop(st: &mut State, c: CId) {
    detach(st, c);
    attach(st, c);
    let _ = focus(st, Some(c));
    let m = st.clients[c].mon;
    let _ = arrange(st, Some(m));
}

fn killclient(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    let win = st.clients[c].win;
    if isproto_del(st, c).unwrap_or(false) {
        let mut data = [0u32; 5];
        data[0] = st.wmatom[WM_DELETE];
        data[1] = CURRENT_TIME;
        let ev = ClientMessageEvent::new(32, win, st.wmatom[WM_PROTOCOLS], data);
        let _ = st.conn.send_event(false, win, EventMask::NO_EVENT, ev);
    } else {
        let _ = st.conn.grab_server();
        let _ = st.conn.kill_client(win);
        let _ = st.conn.flush();
        let _ = st.conn.ungrab_server();
    }
}

fn view(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let want = match arg {
        Arg::U(u) => *u,
        Arg::None => 0,
        _ => 0,
    } & TAG_MASK;
    if want == st.mons[m].tagset[st.mons[m].seltags] { return; }
    st.mons[m].seltags ^= 1;
    if want != 0 {
        let seltags = st.mons[m].seltags;
        st.mons[m].tagset[seltags] = want;
        let mon = &mut st.mons[m];
        mon.prevtag = mon.curtag;
        if want == TAG_MASK { mon.curtag = 0; }
        else {
            for i in 0..TAGS.len() {
                if (want & (1 << i)) != 0 { mon.curtag = i + 1; break; }
            }
        }
    } else {
        let mon = &mut st.mons[m];
        std::mem::swap(&mut mon.prevtag, &mut mon.curtag);
    }
    let mon = &mut st.mons[m];
    mon.lt[0] = mon.lts[mon.curtag];
    mon.mfact = mon.mfacts[mon.curtag];
    if mon.showbar != mon.showbars[mon.curtag] {
        mon.showbar = mon.showbars[mon.curtag];
        update_bar_pos(st, m);
        let bar = st.mons[m].barwin;
        let wx = st.mons[m].wx;
        let by = st.mons[m].by;
        let ww = st.mons[m].ww;
        let bh = st.bh;
        if bar != 0 {
            let _ = st.conn.configure_window(bar, &ConfigureWindowAux::default()
                .x(wx).y(by).width(ww as u32).height(bh as u32));
        }
    }
    let _ = focus(st, None);
    let _ = arrange(st, Some(m));
}

fn toggleview(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let bit = match arg { Arg::U(u) => *u, _ => return };
    let seltags = st.mons[m].seltags;
    let new = st.mons[m].tagset[seltags] ^ (bit & TAG_MASK);
    if new == 0 { return; }
    st.mons[m].tagset[seltags] = new;
    // Update curtag to first set bit (or 0 for all)
    let mon = &mut st.mons[m];
    if new == TAG_MASK { mon.curtag = 0; }
    else { for i in 0..TAGS.len() { if (new & (1 << i)) != 0 { mon.curtag = i + 1; break; } } }
    mon.lt[0] = mon.lts[mon.curtag];
    mon.mfact = mon.mfacts[mon.curtag];
    let _ = focus(st, None);
    let _ = arrange(st, Some(m));
}

fn tag(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    let bits = match arg { Arg::U(u) => *u, _ => return } & TAG_MASK;
    if bits == 0 { return; }
    st.clients[c].tags = bits;
    let _ = focus(st, None);
    let _ = arrange(st, Some(m));
}

fn toggletag(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    let bits = match arg { Arg::U(u) => *u, _ => return };
    let new = st.clients[c].tags ^ (bits & TAG_MASK);
    if new == 0 { return; }
    st.clients[c].tags = new;
    let _ = focus(st, None);
    let _ = arrange(st, Some(m));
}

fn setlayout(st: &mut State, arg: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let mon = &mut st.mons[m];
    let cur_lt = mon.lt[mon.sellt];
    let want_same = match arg { Arg::Layout(lt) => Some(*lt) == Some(cur_lt), _ => true };
    if matches!(arg, Arg::None) || want_same {
        mon.sellt ^= 1;
    }
    if let Arg::Layout(lt) = arg {
        mon.lt[mon.sellt] = *lt;
    }
    mon.lts[mon.curtag] = mon.lt[mon.sellt];
    let new_sym = LAYOUTS[mon.lt[mon.sellt]].symbol.to_vec();
    mon.ltsymbol = new_sym;
    if mon.sel.is_some() {
        let _ = arrange(st, Some(m));
    } else {
        let _ = drawbar(st, m);
    }
}

fn togglefloating(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    if st.clients[c].isfixed { return; }
    st.clients[c].isfloating = !st.clients[c].isfloating || st.clients[c].isfixed;
    if st.clients[c].isfloating {
        let (cx, cy, cw, ch) = (st.clients[c].x, st.clients[c].y, st.clients[c].w, st.clients[c].h);
        let _ = resize(st, c, cx, cy, cw, ch, false);
    }
    let _ = arrange(st, Some(m));
}

fn focusmon(st: &mut State, arg: &Arg) {
    let dir = if let Arg::I(i) = arg { *i } else { return };
    if mon_list(st).len() < 2 { return; }
    let target = match dirtomon(st, dir) { Some(m) => m, None => return };
    if Some(target) == st.selmon { return; }
    if let Some(s) = st.selmon.and_then(|m| st.mons[m].sel) {
        let _ = unfocus(st, s, true);
    }
    st.selmon = Some(target);
    let _ = focus(st, None);
}

fn sendmon(st: &mut State, c: CId, m: MId) {
    if st.clients[c].mon == m { return; }
    let _ = unfocus(st, c, true);
    detach(st, c);
    detach_stack(st, c);
    st.clients[c].mon = m;
    st.clients[c].tags = st.mons[m].tagset[st.mons[m].seltags];
    attach(st, c);
    attach_stack(st, c);
    let _ = focus(st, None);
    let _ = arrange(st, None);
}

fn tagmon(st: &mut State, arg: &Arg) {
    let dir = if let Arg::I(i) = arg { *i } else { return };
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    if mon_list(st).len() < 2 { return; }
    if let Some(target) = dirtomon(st, dir) {
        sendmon(st, c, target);
    }
}

fn movemouse(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    let win = st.clients[c].win;
    if st.clients[c].isfloating == false {
        // toggle floating handled by drag start
    }
    let r = match st.conn.grab_pointer(false, st.root,
        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
        GrabMode::ASYNC, GrabMode::ASYNC, 0u32, st.cursor[CUR_MOVE], CURRENT_TIME)
    {
        Ok(c) => c,
        Err(_) => return,
    };
    if r.reply().map(|r| r.status).unwrap_or(GrabStatus::FROZEN) != GrabStatus::SUCCESS {
        return;
    }
    let pq = match st.conn.query_pointer(st.root) { Ok(c) => c, Err(_) => return };
    let pq = match pq.reply() { Ok(r) => r, Err(_) => return };
    let ocx = st.clients[c].x;
    let ocy = st.clients[c].y;
    let start_rx = pq.root_x as i32;
    let start_ry = pq.root_y as i32;
    let mut last_time: u32 = 0;
    loop {
        let ev = match st.conn.wait_for_event() { Ok(e) => e, Err(_) => break };
        match ev {
            Event::Expose(_) | Event::ConfigureRequest(_) | Event::MapRequest(_) => {
                let _ = dispatch_event(st, ev);
            }
            Event::MotionNotify(e) => {
                if e.time.wrapping_sub(last_time) <= 1000/60 { continue; }
                last_time = e.time;
                let mut nx = ocx + (e.root_x as i32 - start_rx);
                let mut ny = ocy + (e.root_y as i32 - start_ry);
                let mw = st.mons[m].wx;
                let mww = st.mons[m].ww;
                let my = st.mons[m].wy;
                let mwh = st.mons[m].wh;
                let cw = width_of(&st.clients[c]);
                let ch = height_of(&st.clients[c]);
                if (mw - nx).abs() < SNAP { nx = mw; }
                else if ((mw + mww) - (nx + cw)).abs() < SNAP { nx = mw + mww - cw; }
                if (my - ny).abs() < SNAP { ny = my; }
                else if ((my + mwh) - (ny + ch)).abs() < SNAP { ny = my + mwh - ch; }
                let lt = st.mons[m].lt[st.mons[m].sellt];
                let arrange_some = LAYOUTS[lt].arrange.is_some();
                if !st.clients[c].isfloating && arrange_some
                    && ((nx - st.clients[c].x).abs() > SNAP || (ny - st.clients[c].y).abs() > SNAP)
                {
                    togglefloating(st, &Arg::None);
                }
                if !arrange_some || st.clients[c].isfloating {
                    let cw_eff = st.clients[c].w;
                    let ch_eff = st.clients[c].h;
                    let _ = resize(st, c, nx, ny, cw_eff, ch_eff, true);
                }
            }
            Event::ButtonRelease(_) => break,
            _ => { let _ = dispatch_event(st, ev); }
        }
    }
    let _ = st.conn.ungrab_pointer(CURRENT_TIME);
    if let Some(target) = ptrtomon(st, st.clients[c].x + width_of(&st.clients[c]) / 2,
                                       st.clients[c].y + height_of(&st.clients[c]) / 2)
    {
        if target != m {
            sendmon(st, c, target);
            st.selmon = Some(target);
            let _ = focus(st, None);
        }
    }
    let _ = win;
}

fn resizemouse(st: &mut State, _: &Arg) {
    let m = match st.selmon { Some(m) => m, None => return };
    let c = match st.mons[m].sel { Some(s) => s, None => return };
    let win = st.clients[c].win;
    let ocx = st.clients[c].x;
    let ocy = st.clients[c].y;
    let bw = st.clients[c].bw;
    let r = match st.conn.grab_pointer(false, st.root,
        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
        GrabMode::ASYNC, GrabMode::ASYNC, 0u32, st.cursor[CUR_RESIZE], CURRENT_TIME)
    {
        Ok(c) => c,
        Err(_) => return,
    };
    if r.reply().map(|r| r.status).unwrap_or(GrabStatus::FROZEN) != GrabStatus::SUCCESS {
        return;
    }
    let _ = st.conn.warp_pointer(0u32, win, 0, 0, 0, 0,
        (st.clients[c].w + bw - 1) as i16, (st.clients[c].h + bw - 1) as i16);
    let mut last_time: u32 = 0;
    loop {
        let ev = match st.conn.wait_for_event() { Ok(e) => e, Err(_) => break };
        match ev {
            Event::Expose(_) | Event::ConfigureRequest(_) | Event::MapRequest(_) => {
                let _ = dispatch_event(st, ev);
            }
            Event::MotionNotify(e) => {
                if e.time.wrapping_sub(last_time) <= 1000/60 { continue; }
                last_time = e.time;
                let nw = (e.root_x as i32 - ocx - 2 * bw + 1).max(1);
                let nh = (e.root_y as i32 - ocy - 2 * bw + 1).max(1);
                let mwx = st.mons[m].wx;
                let mww = st.mons[m].ww;
                let mwy = st.mons[m].wy;
                let mwh = st.mons[m].wh;
                let cl = &st.clients[c];
                let lt = st.mons[m].lt[st.mons[m].sellt];
                let arrange_some = LAYOUTS[lt].arrange.is_some();
                if cl.mon == m
                    && cl.x + nw >= mwx && cl.x + nw <= mwx + mww
                    && cl.y + nh >= mwy && cl.y + nh <= mwy + mwh
                {
                    if !cl.isfloating && arrange_some
                        && ((nw - cl.w).abs() > SNAP || (nh - cl.h).abs() > SNAP)
                    {
                        togglefloating(st, &Arg::None);
                    }
                    let cl = &st.clients[c];
                    if !arrange_some || cl.isfloating {
                        let (cx, cy) = (cl.x, cl.y);
                        let _ = resize(st, c, cx, cy, nw, nh, true);
                    }
                }
            }
            Event::ButtonRelease(_) => {
                let _ = st.conn.warp_pointer(0u32, win, 0, 0, 0, 0,
                    (st.clients[c].w + bw - 1) as i16, (st.clients[c].h + bw - 1) as i16);
                break;
            }
            _ => { let _ = dispatch_event(st, ev); }
        }
    }
    let _ = st.conn.ungrab_pointer(CURRENT_TIME);
    while st.conn.poll_for_event().ok().flatten().is_some() {}
    if let Some(target) = ptrtomon(st, st.clients[c].x + width_of(&st.clients[c]) / 2,
                                       st.clients[c].y + height_of(&st.clients[c]) / 2)
    {
        if target != m {
            sendmon(st, c, target);
            st.selmon = Some(target);
            let _ = focus(st, None);
        }
    }
}


fn cleanup(st: &mut State) -> Result<(), Box<dyn Error>> {
    let view_all = Arg::U(TAG_MASK);
    view(st, &view_all);
    if let Some(m) = st.selmon {
        let sellt = st.mons[m].sellt;
        st.mons[m].lt[sellt] = LT_FLOAT;
    }
    for m in mon_list(st) {
        while let Some(c) = st.mons[m].stack {
            unmanage(st, c, false)?;
        }
    }
    let _ = st.conn.ungrab_key(0u8, st.root, ModMask::ANY);
    for i in 0..CUR_LAST {
        if st.cursor[i] != 0 {
            let _ = st.conn.free_cursor(st.cursor[i]);
        }
    }
    if st.dc.drawable != 0 {
        let _ = st.conn.free_pixmap(st.dc.drawable);
    }
    let _ = st.conn.free_gc(st.dc.gc);
    let _ = st.conn.close_font(st.dc.font.xfont);
    for m in mon_list(st) {
        let _ = cleanup_mon(st, m);
    }
    st.conn.set_input_focus(InputFocus::POINTER_ROOT, x11rb::NONE, CURRENT_TIME)?;
    st.conn.flush()?;
    Ok(())
}











