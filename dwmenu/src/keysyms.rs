// Selected X11 keysym constants used by dmenu.

#![allow(non_upper_case_globals, dead_code)]

pub const NoSymbol: u32      = 0x000000;

pub const XK_BackSpace: u32  = 0xff08;
pub const XK_Tab: u32        = 0xff09;
pub const XK_Return: u32     = 0xff0d;
pub const XK_Escape: u32     = 0xff1b;
pub const XK_Delete: u32     = 0xffff;
pub const XK_Home: u32       = 0xff50;
pub const XK_Left: u32       = 0xff51;
pub const XK_Up: u32         = 0xff52;
pub const XK_Right: u32      = 0xff53;
pub const XK_Down: u32       = 0xff54;
pub const XK_Prior: u32      = 0xff55; // Page_Up
pub const XK_Next: u32       = 0xff56; // Page_Down
pub const XK_End: u32        = 0xff57;
pub const XK_KP_Enter: u32   = 0xff8d;

// ASCII letters used in Ctrl-bindings
pub const XK_a: u32 = 0x0061;
pub const XK_b: u32 = 0x0062;
pub const XK_c: u32 = 0x0063;
pub const XK_d: u32 = 0x0064;
pub const XK_e: u32 = 0x0065;
pub const XK_f: u32 = 0x0066;
pub const XK_h: u32 = 0x0068;
pub const XK_i: u32 = 0x0069;
pub const XK_j: u32 = 0x006a;
pub const XK_k: u32 = 0x006b;
pub const XK_n: u32 = 0x006e;
pub const XK_p: u32 = 0x0070;
pub const XK_u: u32 = 0x0075;
pub const XK_w: u32 = 0x0077;
pub const XK_y: u32 = 0x0079;

// Convert a keysym to its lower-case form for case-insensitive matching.
pub fn to_lower(ks: u32) -> u32 {
    match ks {
        0x0041..=0x005a => ks + 0x20,                // A-Z
        0x00c0..=0x00d6 => ks + 0x20,                // Latin-1 upper
        0x00d8..=0x00de => ks + 0x20,
        _ => ks,
    }
}
