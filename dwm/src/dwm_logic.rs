// dwm semantics: tags, layouts, actions. Operates on the central `Dwm` state.

use std::os::unix::process::CommandExt;
use std::process::Command;

use smithay::desktop::Window;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::client;
use crate::config::{Arg, LAYOUTS, LT_FLOAT, MFACT, RESIZE_HINTS, SNAP, TAG_MASK};
use crate::state::Dwm;

pub type Action = fn(&mut Dwm, &Arg);

// ---------- Helpers ----------

pub fn windows_on_monitor(state: &Dwm, mon: usize) -> Vec<Window> {
    state
        .space
        .elements()
        .filter(|w| client::with(w, |c| c.monitor == mon))
        .cloned()
        .collect()
}

pub fn visible_on_monitor(state: &Dwm, mon: usize) -> Vec<Window> {
    let m = &state.monitors[mon];
    let tagset = m.tagset[m.seltags];
    state
        .space
        .elements()
        .filter(|w| client::with(w, |c| c.monitor == mon && (c.tags & tagset) != 0))
        .cloned()
        .collect()
}

pub fn tiled_on_monitor(state: &Dwm, mon: usize) -> Vec<Window> {
    visible_on_monitor(state, mon)
        .into_iter()
        .filter(|w| !client::with(w, |c| c.isfloating))
        .collect()
}

pub fn focused_window(state: &Dwm) -> Option<Window> {
    let m = state.monitors.get(state.selmon)?;
    let order = m.sel_focus_order?;
    state
        .space
        .elements()
        .find(|w| client::with(w, |c| c.focus_order == order))
        .cloned()
}

pub fn arrange_all(state: &mut Dwm) {
    for m in 0..state.monitors.len() {
        arrange_monitor(state, m);
    }
}

pub fn arrange_monitor(state: &mut Dwm, mon: usize) {
    if mon >= state.monitors.len() { return; }
    let lt = state.monitors[mon].lt[state.monitors[mon].sellt];
    if let Some(arrange) = LAYOUTS[lt].arrange {
        arrange(state, mon);
    }
    // Hide windows whose tags don't match the current tagset by unmapping.
    let m_tagset = state.monitors[mon].tagset[state.monitors[mon].seltags];
    let to_hide: Vec<Window> = state.space.elements()
        .filter(|w| client::with(w, |c| c.monitor == mon && (c.tags & m_tagset) == 0))
        .cloned()
        .collect();
    for w in to_hide {
        state.space.unmap_elem(&w);
    }
}

// ---------- Layouts ----------

pub fn tile(state: &mut Dwm, mon: usize) {
    let tiled = tiled_on_monitor(state, mon);
    let n = tiled.len();
    if n == 0 { return; }
    let wa = state.monitors[mon].work_area;
    let mfact = state.monitors[mon].mfact;
    let bw = crate::config::BORDERPX;
    let mw = if n == 1 { wa.size.w } else { (mfact * wa.size.w as f32) as i32 };

    // Master.
    let master = &tiled[0];
    let mh = wa.size.h;
    place_window(state, master, wa.loc, Size::from((mw - 2 * bw, mh - 2 * bw)));
    if n == 1 { return; }

    // Stack on the right.
    let stack_n = (n - 1) as i32;
    let h_each = wa.size.h / stack_n;
    let mut y = wa.loc.y;
    for (i, w) in tiled.iter().enumerate().skip(1) {
        let h = if i as i32 == stack_n { wa.loc.y + wa.size.h - y - 2 * bw } else { h_each - 2 * bw };
        let x = wa.loc.x + mw;
        let ww = wa.size.w - mw;
        place_window(state, w, Point::from((x, y)), Size::from((ww - 2 * bw, h)));
        y += h_each;
    }
}

pub fn monocle(state: &mut Dwm, mon: usize) {
    let tiled = tiled_on_monitor(state, mon);
    let n = tiled.len();
    if n == 0 { return; }
    state.monitors[mon].ltsymbol = format!("[{}]", n);
    let wa = state.monitors[mon].work_area;
    let bw = crate::config::BORDERPX;
    for w in &tiled {
        place_window(state, w, wa.loc, Size::from((wa.size.w - 2 * bw, wa.size.h - 2 * bw)));
    }
}

fn place_window(state: &mut Dwm, w: &Window, loc: Point<i32, Logical>, size: Size<i32, Logical>) {
    if let Some(toplevel) = w.toplevel() {
        toplevel.with_pending_state(|s| {
            s.size = Some(size);
            s.bounds = Some(size);
        });
        toplevel.send_pending_configure();
    }
    state.space.map_element(w.clone(), loc, false);
    let stored = (loc.x, loc.y, size.w, size.h);
    client::with_mut(w, |c| {
        c.stored_x = stored.0;
        c.stored_y = stored.1;
        c.stored_w = stored.2;
        c.stored_h = stored.3;
    });
}


// ---------- Actions ----------

pub fn quit(state: &mut Dwm, _: &Arg) {
    state.running = false;
    state.loop_signal.stop();
}

pub fn spawn(_state: &mut Dwm, arg: &Arg) {
    let argv = match arg {
        Arg::Cmd(c) => *c,
        _ => return,
    };
    if argv.is_empty() { return; }
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..]);
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

pub fn togglebar(state: &mut Dwm, _: &Arg) {
    let m = state.selmon;
    state.monitors[m].showbar = !state.monitors[m].showbar;
    let curtag = state.monitors[m].curtag;
    state.monitors[m].showbars[curtag] = state.monitors[m].showbar;
    state.monitors[m].update_work_area(crate::bar::BAR_HEIGHT);
    arrange_monitor(state, m);
}

pub fn focusstack(state: &mut Dwm, arg: &Arg) {
    let dir = match arg { Arg::I(i) => *i, _ => 1 };
    let m = state.selmon;
    let visible = visible_on_monitor(state, m);
    if visible.is_empty() { return; }
    let cur_idx = focused_window(state)
        .and_then(|w| visible.iter().position(|v| v == &w))
        .unwrap_or(0);
    let len = visible.len() as i32;
    let next_idx = ((cur_idx as i32 + dir).rem_euclid(len)) as usize;
    focus(state, Some(visible[next_idx].clone()));
}

pub fn setmfact(state: &mut Dwm, arg: &Arg) {
    let f = match arg { Arg::F(f) => *f, _ => return };
    let m = state.selmon;
    let new_f = if f < 1.0 { state.monitors[m].mfact + f } else { f - 1.0 };
    if !(0.1..=0.9).contains(&new_f) { return; }
    state.monitors[m].mfact = new_f;
    let curtag = state.monitors[m].curtag;
    state.monitors[m].mfacts[curtag] = new_f;
    arrange_monitor(state, m);
}

pub fn zoom(state: &mut Dwm, _: &Arg) {
    let m = state.selmon;
    let lt = state.monitors[m].lt[state.monitors[m].sellt];
    if LAYOUTS[lt].arrange.is_none() { return; }
    let Some(cur) = focused_window(state) else { return; };
    if client::with(&cur, |c| c.isfloating) { return; }
    let tiled = tiled_on_monitor(state, m);
    if tiled.len() < 2 { return; }
    if &tiled[0] == &cur {
        let promoted = tiled[1].clone();
        state.space.raise_element(&promoted, true);
    } else {
        state.space.raise_element(&cur, true);
    }
    arrange_monitor(state, m);
}

pub fn killclient(state: &mut Dwm, _: &Arg) {
    let Some(w) = focused_window(state) else { return; };
    if let Some(toplevel) = w.toplevel() {
        toplevel.send_close();
    }
}

pub fn view(state: &mut Dwm, arg: &Arg) {
    let m = state.selmon;
    let mask = match arg {
        Arg::U(u) => u & TAG_MASK,
        Arg::None => state.monitors[m].tagset[state.monitors[m].seltags ^ 1],
        _ => return,
    };
    if mask == state.monitors[m].tagset[state.monitors[m].seltags] { return; }
    state.monitors[m].seltags ^= 1;
    if mask != 0 {
        state.monitors[m].tagset[state.monitors[m].seltags] = mask;
        let new_tag = if mask == TAG_MASK { 0 } else { mask.trailing_zeros() as usize + 1 };
        state.monitors[m].prevtag = state.monitors[m].curtag;
        state.monitors[m].curtag = new_tag;
        let lt = state.monitors[m].lts[new_tag];
        state.monitors[m].lt[state.monitors[m].sellt] = lt;
        state.monitors[m].mfact = state.monitors[m].mfacts[new_tag];
        state.monitors[m].showbar = state.monitors[m].showbars[new_tag];
        state.monitors[m].ltsymbol = LAYOUTS[lt].symbol.to_string();
        state.monitors[m].update_work_area(crate::bar::BAR_HEIGHT);
    }
    arrange_monitor(state, m);
    let visible = visible_on_monitor(state, m);
    for w in visible {
        let pos = client::with(&w, |c| Point::from((c.stored_x, c.stored_y)));
        state.space.map_element(w, pos, false);
    }
}

pub fn toggleview(state: &mut Dwm, arg: &Arg) {
    let mask = match arg { Arg::U(u) => u & TAG_MASK, _ => return };
    let m = state.selmon;
    let new_tagset = state.monitors[m].tagset[state.monitors[m].seltags] ^ mask;
    if new_tagset != 0 {
        state.monitors[m].tagset[state.monitors[m].seltags] = new_tagset;
        arrange_monitor(state, m);
    }
}

pub fn tag(state: &mut Dwm, arg: &Arg) {
    let mask = match arg { Arg::U(u) => u & TAG_MASK, _ => return };
    if mask == 0 { return; }
    let Some(w) = focused_window(state) else { return; };
    client::with_mut(&w, |c| c.tags = mask);
    arrange_monitor(state, state.selmon);
}

pub fn toggletag(state: &mut Dwm, arg: &Arg) {
    let mask = match arg { Arg::U(u) => u & TAG_MASK, _ => return };
    let Some(w) = focused_window(state) else { return; };
    client::with_mut(&w, |c| {
        let new_tags = c.tags ^ mask;
        if new_tags != 0 { c.tags = new_tags; }
    });
    arrange_monitor(state, state.selmon);
}


pub fn setlayout(state: &mut Dwm, arg: &Arg) {
    let m = state.selmon;
    let new_lt = match arg {
        Arg::Layout(idx) => Some(*idx),
        Arg::None => None,
        _ => return,
    };
    state.monitors[m].set_layout(new_lt);
    arrange_monitor(state, m);
}

pub fn togglefloating(state: &mut Dwm, _: &Arg) {
    let Some(w) = focused_window(state) else { return; };
    let m = state.selmon;
    let now_float = client::with(&w, |c| !c.isfloating || c.isfixed);
    client::with_mut(&w, |c| { c.isfloating = now_float; });
    if now_float {
        // Restore stored geometry.
        let (x, y, ww, wh) = client::with(&w, |c|
            (c.stored_x, c.stored_y, c.stored_w.max(100), c.stored_h.max(100)));
        place_window(state, &w, Point::from((x, y)), Size::from((ww, wh)));
    }
    arrange_monitor(state, m);
}

pub fn focusmon(state: &mut Dwm, arg: &Arg) {
    let dir = match arg { Arg::I(i) => *i, _ => 1 };
    if state.monitors.len() < 2 { return; }
    let len = state.monitors.len() as i32;
    let new_m = ((state.selmon as i32 + dir).rem_euclid(len)) as usize;
    state.selmon = new_m;
    // Refocus the previously-selected window on the new monitor.
    let visible = visible_on_monitor(state, new_m);
    if let Some(w) = visible.first().cloned() {
        focus(state, Some(w));
    }
}

pub fn tagmon(state: &mut Dwm, arg: &Arg) {
    let dir = match arg { Arg::I(i) => *i, _ => 1 };
    if state.monitors.len() < 2 { return; }
    let Some(w) = focused_window(state) else { return; };
    let len = state.monitors.len() as i32;
    let new_m = ((state.selmon as i32 + dir).rem_euclid(len)) as usize;
    client::with_mut(&w, |c| { c.monitor = new_m; });
    arrange_monitor(state, state.selmon);
    arrange_monitor(state, new_m);
}

/// Make `w` the active window, or clear focus when `None`.
pub fn focus(state: &mut Dwm, w: Option<Window>) {
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    let kb = match state.seat.get_keyboard() { Some(k) => k, None => return };
    if let Some(w) = w {
        let order = state.next_focus_order();
        client::with_mut(&w, |c| { c.focus_order = order; });
        let m = client::with(&w, |c| c.monitor);
        if m < state.monitors.len() {
            state.monitors[m].sel_focus_order = Some(order);
            state.selmon = m;
        }
        state.space.raise_element(&w, true);
        let surf = w.toplevel().map(|t| t.wl_surface().clone());
        kb.set_focus(state, surf, serial);
        // Notify clients of activation state.
        for elem in state.space.elements() {
            let act = elem == &w;
            elem.set_activated(act);
            if let Some(t) = elem.toplevel() { t.send_pending_configure(); }
        }
    } else {
        kb.set_focus(state, Option::<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>::None, serial);
        for elem in state.space.elements() {
            elem.set_activated(false);
            if let Some(t) = elem.toplevel() { t.send_pending_configure(); }
        }
    }
}

/// Reset state to initial values when the user invokes `view` with the same tag.
#[allow(dead_code)]
pub fn reset_pertag(state: &mut Dwm, mon: usize) {
    let n = TAG_MASK.count_ones() as usize + 1;
    state.monitors[mon].lts = vec![crate::config::LT_TILE; n];
    state.monitors[mon].mfacts = vec![MFACT; n];
    state.monitors[mon].showbars = vec![crate::config::SHOWBAR; n];
}

// ---------- Size hints ----------

pub fn apply_size_hints(c: &crate::client::ClientData, w: &mut i32, h: &mut i32) -> bool {
    if !RESIZE_HINTS && !c.isfloating { return false; }
    let baseismin = c.basew == c.minw && c.baseh == c.minh;
    if !baseismin { *w -= c.basew; *h -= c.baseh; }
    if c.mina > 0.0 && c.maxa > 0.0 && *h > 0 && *w > 0 {
        if c.maxa < (*w as f32) / (*h as f32) { *w = (*h as f32 * c.maxa + 0.5) as i32; }
        else if c.mina < (*h as f32) / (*w as f32) { *h = (*w as f32 * c.mina + 0.5) as i32; }
    }
    if baseismin { *w -= c.basew; *h -= c.baseh; }
    if c.incw != 0 { *w -= *w % c.incw; }
    if c.inch != 0 { *h -= *h % c.inch; }
    *w += c.basew; *h += c.baseh;
    *w = (*w).max(c.minw);
    *h = (*h).max(c.minh);
    if c.maxw != 0 { *w = (*w).min(c.maxw); }
    if c.maxh != 0 { *h = (*h).min(c.maxh); }
    true
}

// Snap a rectangle to monitor edges within SNAP pixels.
pub fn snap(state: &Dwm, mon: usize, rect: &mut Rectangle<i32, Logical>) {
    let mr = state.monitors[mon].geometry;
    if (rect.loc.x - mr.loc.x).abs() < SNAP { rect.loc.x = mr.loc.x; }
    if (rect.loc.y - mr.loc.y).abs() < SNAP { rect.loc.y = mr.loc.y; }
    let rx2 = rect.loc.x + rect.size.w;
    let mx2 = mr.loc.x + mr.size.w;
    if (rx2 - mx2).abs() < SNAP { rect.loc.x = mx2 - rect.size.w; }
    let ry2 = rect.loc.y + rect.size.h;
    let my2 = mr.loc.y + mr.size.h;
    if (ry2 - my2).abs() < SNAP { rect.loc.y = my2 - rect.size.h; }
}

// Used by xdg_shell when a new toplevel is mapped: place it on a monitor and
// give it default tags. Returns the assigned monitor index.
pub fn manage_new_window(state: &mut Dwm, window: &Window) -> usize {
    crate::client::ensure(window);
    let m = state.selmon;
    let app_id = window.toplevel()
        .and_then(|t| t.with_pending_state(|s| s.app_id.clone()))
        .unwrap_or_default();
    let title = window.toplevel()
        .and_then(|t| t.with_pending_state(|s| s.title.clone()))
        .unwrap_or_default();
    let mut isfloating = false;
    let mut tags = state.monitors[m].tagset[state.monitors[m].seltags];
    let mut mon = m;
    for r in crate::config::RULES {
        let class_match = r.class.map(|c| c == app_id).unwrap_or(true);
        let title_match = r.title.map(|t| title.contains(t)).unwrap_or(true);
        if class_match && title_match {
            isfloating = r.isfloating;
            if r.tags != 0 { tags = r.tags; }
            if r.monitor >= 0 && (r.monitor as usize) < state.monitors.len() {
                mon = r.monitor as usize;
            }
            break;
        }
    }
    crate::client::with_mut(window, |c| {
        c.tags = tags;
        c.monitor = mon;
        c.isfloating = isfloating;
    });
    mon
}

// Suppress unused-import warning for the LT_FLOAT alias from config.
const _: usize = LT_FLOAT;
