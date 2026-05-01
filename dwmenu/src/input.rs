// Keyboard event handling, ported from dmenu.c keypress().

use std::io::{self, Write};

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};

use crate::match_logic::nextrune;
use crate::state::Menu;

fn insert_text(menu: &mut Menu, s: &[u8], n: isize) {
    let cur_len = menu.text.len() as isize;
    if cur_len + n < 0 || cur_len + n > 4096 {
        return;
    }
    if n > 0 {
        let pre = menu.text[..menu.cursor].to_vec();
        let post = menu.text[menu.cursor..].to_vec();
        menu.text.clear();
        menu.text.extend_from_slice(&pre);
        menu.text.extend_from_slice(&s[..n as usize]);
        menu.text.extend_from_slice(&post);
        menu.cursor = (menu.cursor as isize + n) as usize;
    } else if n < 0 {
        let del = (-n) as usize;
        let start = menu.cursor.saturating_sub(del);
        menu.text.drain(start..menu.cursor);
        menu.cursor = start;
    }
    let sub = n > 0 && menu.cursor == menu.text.len();
    menu.do_match(sub);
}

fn handle_up(menu: &mut Menu) {
    if let Some(s) = menu.sel {
        if let Some(l) = menu.items[s].left {
            menu.sel = Some(l);
            if menu.items[l].right == menu.curr {
                menu.curr = menu.prev;
                menu.calcoffsets();
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
                menu.calcoffsets();
            }
        }
    }
}

fn finish(menu: &mut Menu, success: bool, use_sel: bool) {
    let mut out = io::stdout().lock();
    if success {
        if use_sel {
            if let Some(idx) = menu.sel {
                let _ = out.write_all(&menu.items[idx].text);
            } else {
                let _ = out.write_all(&menu.text);
            }
        } else {
            let _ = out.write_all(&menu.text);
        }
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
    menu.exit_code = Some(if success { 0 } else { 1 });
}

pub fn handle_key(menu: &mut Menu, event: KeyEvent) {
    let ctrl = menu.modifiers.ctrl;
    let shift = menu.modifiers.shift;
    let mut ksym = event.keysym;

    if ctrl {
        // Match either the lowercase or uppercase keysym for each letter binding.
        let is = |a: Keysym, b: Keysym| ksym == a || ksym == b;
        if is(Keysym::a, Keysym::A) { ksym = Keysym::Home; }
        else if is(Keysym::b, Keysym::B) { ksym = Keysym::Left; }
        else if is(Keysym::c, Keysym::C) { ksym = Keysym::Escape; }
        else if is(Keysym::d, Keysym::D) { ksym = Keysym::Delete; }
        else if is(Keysym::e, Keysym::E) { ksym = Keysym::End; }
        else if is(Keysym::f, Keysym::F) { ksym = Keysym::Right; }
        else if is(Keysym::h, Keysym::H) { ksym = Keysym::BackSpace; }
        else if is(Keysym::i, Keysym::I) { ksym = Keysym::Tab; }
        else if is(Keysym::j, Keysym::J) { ksym = Keysym::Return; }
        else if is(Keysym::k, Keysym::K) {
            menu.text.truncate(menu.cursor); menu.do_match(false); return;
        }
        else if is(Keysym::n, Keysym::N) { ksym = Keysym::Down; }
        else if is(Keysym::p, Keysym::P) { ksym = Keysym::Up; }
        else if is(Keysym::u, Keysym::U) {
            let cur = menu.cursor as isize; insert_text(menu, &[], -cur); return;
        }
        else if is(Keysym::w, Keysym::W) {
            while menu.cursor > 0 && menu.text[nextrune(&menu.text, menu.cursor, -1) as usize] == b' ' {
                let r = nextrune(&menu.text, menu.cursor, -1);
                insert_text(menu, &[], r - menu.cursor as isize);
            }
            while menu.cursor > 0 && menu.text[nextrune(&menu.text, menu.cursor, -1) as usize] != b' ' {
                let r = nextrune(&menu.text, menu.cursor, -1);
                insert_text(menu, &[], r - menu.cursor as isize);
            }
            return;
        }
        else if is(Keysym::y, Keysym::Y) {
            // Ctrl-Y: paste from primary selection. Not implemented in this port.
            return;
        }
        else { return; }
    }

    match ksym {
        Keysym::Delete => {
            if menu.cursor < menu.text.len() {
                let r = nextrune(&menu.text, menu.cursor, 1);
                menu.cursor = r as usize;
                let r2 = nextrune(&menu.text, menu.cursor, -1);
                if menu.cursor > 0 { insert_text(menu, &[], r2 - menu.cursor as isize); }
            }
        }
        Keysym::BackSpace => {
            if menu.cursor > 0 {
                let r = nextrune(&menu.text, menu.cursor, -1);
                insert_text(menu, &[], r - menu.cursor as isize);
            }
        }
        Keysym::End => {
            if menu.cursor != menu.text.len() {
                menu.cursor = menu.text.len();
            } else {
                if menu.next.is_some() {
                    menu.curr = menu.matchend;
                    menu.calcoffsets();
                    menu.curr = menu.prev;
                    menu.calcoffsets();
                    while menu.next.is_some() {
                        if let Some(c) = menu.curr {
                            if let Some(r) = menu.items[c].right {
                                menu.curr = Some(r);
                                menu.calcoffsets();
                                continue;
                            }
                        }
                        break;
                    }
                }
                menu.sel = menu.matchend;
            }
        }
        Keysym::Escape => finish(menu, false, false),
        Keysym::Home => {
            if menu.sel == menu.matches { menu.cursor = 0; }
            else { menu.sel = menu.matches; menu.curr = menu.matches; menu.calcoffsets(); }
        }
        Keysym::Left => {
            if menu.cursor > 0
                && (menu.sel.is_none()
                    || menu.sel.and_then(|s| menu.items[s].left).is_none()
                    || menu.args.lines > 0)
            {
                let r = nextrune(&menu.text, menu.cursor, -1);
                menu.cursor = r.max(0) as usize;
            } else if menu.args.lines == 0 {
                handle_up(menu);
            }
        }
        Keysym::Up => handle_up(menu),
        Keysym::Next => {
            if let Some(n) = menu.next {
                menu.sel = Some(n); menu.curr = Some(n); menu.calcoffsets();
            }
        }
        Keysym::Prior => {
            if let Some(p) = menu.prev {
                menu.sel = Some(p); menu.curr = Some(p); menu.calcoffsets();
            }
        }
        Keysym::Return | Keysym::KP_Enter => finish(menu, true, !shift),
        Keysym::Right => {
            if menu.cursor < menu.text.len() {
                let r = nextrune(&menu.text, menu.cursor, 1);
                menu.cursor = r as usize;
            } else if menu.args.lines == 0 {
                handle_down(menu);
            }
        }
        Keysym::Down => handle_down(menu),
        Keysym::Tab => {
            if let Some(s) = menu.sel {
                let s = menu.items[s].text.clone();
                menu.text.clear();
                menu.text.extend_from_slice(&s);
                menu.cursor = menu.text.len();
                menu.do_match(true);
            }
        }
        _ => {
            // Plain printable character: take the UTF-8 from the event.
            if let Some(utf8) = event.utf8.as_ref() {
                if !utf8.is_empty() && !utf8.chars().any(|c| c.is_control()) {
                    let bytes = utf8.as_bytes().to_vec();
                    insert_text(menu, &bytes, bytes.len() as isize);
                }
            }
        }
    }
}
