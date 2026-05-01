// Per-window dwm metadata. Stored in smithay's `Window::user_data()`.
// Uses a Mutex because UserDataMap entries must be Send+Sync.

use std::sync::Mutex;

use smithay::desktop::Window;

#[derive(Debug)]
pub struct ClientData {
    pub tags: u32,
    pub monitor: usize,
    pub isfloating: bool,
    pub isfixed: bool,
    pub isurgent: bool,
    pub never_focus: bool,
    pub bw: i32,

    // Pre-floating geometry, used to restore a window after
    // toggling out of floating mode.
    pub stored_x: i32,
    pub stored_y: i32,
    pub stored_w: i32,
    pub stored_h: i32,

    // Size hints, populated from xdg_toplevel state.
    pub minw: i32,
    pub minh: i32,
    pub maxw: i32,
    pub maxh: i32,
    pub basew: i32,
    pub baseh: i32,
    pub incw: i32,
    pub inch: i32,
    pub mina: f32,
    pub maxa: f32,

    // Linkage in the per-monitor focus stack.
    pub focus_order: u64,
}

impl Default for ClientData {
    fn default() -> Self {
        Self {
            tags: 1,
            monitor: 0,
            isfloating: false,
            isfixed: false,
            isurgent: false,
            never_focus: false,
            bw: crate::config::BORDERPX,
            stored_x: 0,
            stored_y: 0,
            stored_w: 0,
            stored_h: 0,
            minw: 0,
            minh: 0,
            maxw: 0,
            maxh: 0,
            basew: 0,
            baseh: 0,
            incw: 0,
            inch: 0,
            mina: 0.0,
            maxa: 0.0,
            focus_order: 0,
        }
    }
}

pub type CellData = Mutex<ClientData>;

pub fn ensure(window: &Window) {
    let ud = window.user_data();
    ud.insert_if_missing_threadsafe(|| Mutex::new(ClientData::default()));
}

pub fn with<R>(window: &Window, f: impl FnOnce(&ClientData) -> R) -> R {
    ensure(window);
    let cell = window.user_data().get::<CellData>().expect("ClientData missing");
    let b = cell.lock().unwrap();
    f(&b)
}

pub fn with_mut<R>(window: &Window, f: impl FnOnce(&mut ClientData) -> R) -> R {
    ensure(window);
    let cell = window.user_data().get::<CellData>().expect("ClientData missing");
    let mut b = cell.lock().unwrap();
    f(&mut b)
}
