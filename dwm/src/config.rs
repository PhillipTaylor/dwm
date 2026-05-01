// Compile-time configuration, mirroring the original config.h of dwm.
// Edit and recompile to reconfigure.

use xkbcommon::xkb;

use crate::dwm_logic::Action;

pub type Keysym = xkb::Keysym;

#[derive(Clone)]
pub enum Arg {
    None,
    I(i32),
    U(u32),
    F(f32),
    Cmd(&'static [&'static str]),
    Layout(usize),
}

#[derive(Clone)]
pub struct KeyDef {
    pub modifiers: ModMask,
    pub keysym: Keysym,
    pub func: Action,
    pub arg: Arg,
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ModMask: u32 {
        const SHIFT = 1 << 0;
        const CAPS  = 1 << 1;
        const CTRL  = 1 << 2;
        const ALT   = 1 << 3; // Mod1 / Alt
        const NUM   = 1 << 4;
        const LOGO  = 1 << 6; // Mod4 / Super
    }
}

pub const MOD_KEY: ModMask = ModMask::ALT;

// Visual configuration.
pub const FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
];
pub const FONT_PX: f32 = 14.0;

pub const NORM_BORDER_COLOR: u32 = 0xff_cc_cc_cc;
pub const SEL_BORDER_COLOR:  u32 = 0xff_af_78_17;
pub const NORM_BG_COLOR:     u32 = 0xff_cc_cc_cc;
pub const NORM_FG_COLOR:     u32 = 0xff_00_00_00;
pub const SEL_BG_COLOR:      u32 = 0xff_af_78_17;
pub const SEL_FG_COLOR:      u32 = 0xff_ff_ff_ff;

pub const BORDERPX: i32 = 3;
pub const SNAP: i32 = 10;
pub const SHOWBAR: bool = true;
pub const TOPBAR: bool = true;
pub const MFACT: f32 = 0.55;
pub const RESIZE_HINTS: bool = true;

pub static TAGS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9"];
pub const TAG_MASK: u32 = (1u32 << 9) - 1;

// Layouts.
pub const LT_TILE: usize    = 0;
pub const LT_FLOAT: usize   = 1;
pub const LT_MONOCLE: usize = 2;

pub struct LayoutDef {
    pub symbol: &'static str,
    pub arrange: Option<fn(&mut crate::state::Dwm, usize)>,
}

pub static LAYOUTS: &[LayoutDef] = &[
    LayoutDef { symbol: "[]=", arrange: Some(crate::dwm_logic::tile) },
    LayoutDef { symbol: "><>", arrange: None },
    LayoutDef { symbol: "[M]", arrange: Some(crate::dwm_logic::monocle) },
];

// External commands. Edit to taste.
pub const DMENU_CMD: &[&str] = &["dmenu_run"];
pub const TERM_CMD:  &[&str] = &["foot"];
pub const LOCK_CMD:  &[&str] = &["swaylock"];
pub const PAUSE_CMD: &[&str] = &["playerctl", "play-pause"];
pub const VOL_DOWN_CMD: &[&str] = &["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"];
pub const VOL_UP_CMD:   &[&str] = &["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"];
pub const FORWARD_TRACK: &[&str] = &["playerctl", "next"];

pub fn build_keys() -> Vec<KeyDef> {
    use crate::dwm_logic as L;
    let mut k = vec![
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::p,      func: L::spawn,          arg: Arg::Cmd(DMENU_CMD) },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::Return, func: L::spawn,          arg: Arg::Cmd(TERM_CMD) },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::l,      func: L::spawn,          arg: Arg::Cmd(LOCK_CMD) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::F9,     func: L::spawn,          arg: Arg::Cmd(PAUSE_CMD) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::F10,    func: L::spawn,          arg: Arg::Cmd(VOL_DOWN_CMD) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::F11,    func: L::spawn,          arg: Arg::Cmd(VOL_UP_CMD) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::F12,    func: L::spawn,          arg: Arg::Cmd(FORWARD_TRACK) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::b,      func: L::togglebar,      arg: Arg::None },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::j,      func: L::focusstack,     arg: Arg::I(1) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::k,      func: L::focusstack,     arg: Arg::I(-1) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::h,      func: L::setmfact,       arg: Arg::F(-0.05) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::l,      func: L::setmfact,       arg: Arg::F(0.05) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::Return, func: L::zoom,           arg: Arg::None },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::Tab,    func: L::view,           arg: Arg::None },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::c,      func: L::killclient,     arg: Arg::None },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::t,      func: L::setlayout,      arg: Arg::Layout(LT_TILE) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::f,      func: L::setlayout,      arg: Arg::Layout(LT_FLOAT) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::m,      func: L::setlayout,      arg: Arg::Layout(LT_MONOCLE) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::space,  func: L::setlayout,      arg: Arg::None },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::space,  func: L::togglefloating, arg: Arg::None },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::_0,     func: L::view,           arg: Arg::U(!0) },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::_0,     func: L::tag,            arg: Arg::U(!0) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::comma,  func: L::focusmon,       arg: Arg::I(-1) },
        KeyDef { modifiers: MOD_KEY,                 keysym: Keysym::period, func: L::focusmon,       arg: Arg::I(1) },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::comma,  func: L::tagmon,         arg: Arg::I(-1) },
        KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,  keysym: Keysym::period, func: L::tagmon,         arg: Arg::I(1) },
    ];
    let tag_keys = [
        Keysym::_1, Keysym::_2, Keysym::_3, Keysym::_4, Keysym::_5,
        Keysym::_6, Keysym::_7, Keysym::_8, Keysym::_9,
    ];
    for (i, &ks) in tag_keys.iter().enumerate() {
        let mask = 1u32 << i;
        k.push(KeyDef { modifiers: MOD_KEY,                              keysym: ks, func: L::view,       arg: Arg::U(mask) });
        k.push(KeyDef { modifiers: MOD_KEY|ModMask::CTRL,                keysym: ks, func: L::toggleview, arg: Arg::U(mask) });
        k.push(KeyDef { modifiers: MOD_KEY|ModMask::SHIFT,               keysym: ks, func: L::tag,        arg: Arg::U(mask) });
        k.push(KeyDef { modifiers: MOD_KEY|ModMask::CTRL|ModMask::SHIFT, keysym: ks, func: L::toggletag,  arg: Arg::U(mask) });
    }
    k.push(KeyDef { modifiers: MOD_KEY|ModMask::SHIFT, keysym: Keysym::q, func: L::quit, arg: Arg::None });
    k
}

pub struct RuleDef {
    pub class: Option<&'static str>,
    pub instance: Option<&'static str>,
    pub title: Option<&'static str>,
    pub tags: u32,
    pub isfloating: bool,
    pub monitor: i32,
}

pub static RULES: &[RuleDef] = &[
    RuleDef { class: Some("Gimp"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some("Kate"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some("Gedit"),                instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some("Gvim"),                 instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some("VirtualBox"),           instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
    RuleDef { class: Some("nm-connection-editor"), instance: None, title: None,                              tags: 0, isfloating: true, monitor: -1 },
];
