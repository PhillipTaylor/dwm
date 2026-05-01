// Input event dispatch. Translates raw libinput events (via smithay's
// InputBackend abstraction) into seat actions and dwm keybind dispatch.

use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
    KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::input::keyboard::{xkb, FilterResult, Keysym, ModifiersState};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent};
use smithay::utils::SERIAL_COUNTER;

use crate::config::{Arg, ModMask};
use crate::state::Dwm;

/// Convert smithay's modifier state into our ModMask bitset.
fn mods_from(m: &ModifiersState) -> ModMask {
    let mut mm = ModMask::empty();
    if m.shift   { mm |= ModMask::SHIFT; }
    if m.ctrl    { mm |= ModMask::CTRL; }
    if m.alt     { mm |= ModMask::ALT; }
    if m.logo    { mm |= ModMask::LOGO; }
    if m.caps_lock { mm |= ModMask::CAPS; }
    if m.num_lock  { mm |= ModMask::NUM; }
    mm
}

/// Mask we ignore when matching keybinds (caps + numlock).
const IGNORE_MASK: ModMask = ModMask::from_bits_truncate(
    ModMask::CAPS.bits() | ModMask::NUM.bits(),
);

impl Dwm {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => self.handle_key::<I>(event),
            InputEvent::PointerMotion { event, .. } => self.handle_pointer_motion::<I>(event),
            InputEvent::PointerMotionAbsolute { event, .. } => {
                self.handle_pointer_motion_abs::<I>(event)
            }
            InputEvent::PointerButton { event, .. } => self.handle_pointer_button::<I>(event),
            InputEvent::PointerAxis { event, .. } => self.handle_pointer_axis::<I>(event),
            _ => {}
        }
    }

    fn handle_key<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(&event);
        let keycode = event.key_code();
        let state = event.state();

        let keys = self.keys.clone();
        let action = self.seat.get_keyboard().unwrap().input::<Option<(crate::dwm_logic::Action, Arg)>, _>(
            self,
            keycode,
            state,
            serial,
            time,
            |_, mods, handle| {
                if state != KeyState::Pressed {
                    return FilterResult::Forward;
                }
                let our_mods = mods_from(mods);
                let masked = our_mods - IGNORE_MASK;
                for sym in handle.modified_syms() {
                    let s: Keysym = *sym;
                    for kd in &keys {
                        if kd.keysym == s && kd.modifiers == masked {
                            return FilterResult::Intercept(Some((kd.func, kd.arg.clone())));
                        }
                    }
                }
                FilterResult::Forward
            },
        );
        if let Some(Some((func, arg))) = action {
            func(self, &arg);
        }
    }

    fn handle_pointer_motion<I: InputBackend>(&mut self, event: I::PointerMotionEvent) {
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.seat.get_pointer().unwrap();
        let mut location = pointer.current_location();
        location += event.delta();
        let max = self.outputs_bounding_size();
        location.x = location.x.clamp(0.0, max.0 as f64);
        location.y = location.y.clamp(0.0, max.1 as f64);
        let under = self.surface_under(location);
        pointer.motion(self, under.clone(),
            &MotionEvent { location, serial, time: event.time_msec() });
        pointer.relative_motion(self, under,
            &RelativeMotionEvent { delta: event.delta(), delta_unaccel: event.delta_unaccel(), utime: event.time() });
        pointer.frame(self);
        // Follow the pointer between monitors.
        let new_mon = self.monitor_at(location);
        if new_mon != self.selmon { self.selmon = new_mon; }
    }

    fn handle_pointer_motion_abs<I: InputBackend>(&mut self, event: I::PointerMotionAbsoluteEvent) {
        let max = self.outputs_bounding_size();
        let pos = event.position_transformed(smithay::utils::Size::from((max.0, max.1)));
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.seat.get_pointer().unwrap();
        let under = self.surface_under(pos);
        pointer.motion(self, under, &MotionEvent { location: pos, serial, time: event.time_msec() });
        pointer.frame(self);
        let new_mon = self.monitor_at(pos);
        if new_mon != self.selmon { self.selmon = new_mon; }
    }

    fn handle_pointer_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) {
        let pointer = self.seat.get_pointer().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        let button = event.button_code();
        let state = event.state();

        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            // Click-to-focus.
            let loc = pointer.current_location();
            if let Some((window, _)) = self.space.element_under(loc).map(|(w, l)| (w.clone(), l)) {
                crate::dwm_logic::focus(self, Some(window));
            } else {
                crate::dwm_logic::focus(self, None);
            }
        }
        pointer.button(self, &ButtonEvent { button, state, serial, time: event.time_msec() });
        pointer.frame(self);
    }

    fn handle_pointer_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
        let source = event.source();
        let h_amount = event.amount(Axis::Horizontal)
            .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
        let v_amount = event.amount(Axis::Vertical)
            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);
        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        if h_amount != 0.0 { frame = frame.value(Axis::Horizontal, h_amount); }
        if v_amount != 0.0 { frame = frame.value(Axis::Vertical, v_amount); }
        if source == AxisSource::Finger {
            if event.amount(Axis::Horizontal) == Some(0.0) { frame = frame.stop(Axis::Horizontal); }
            if event.amount(Axis::Vertical)   == Some(0.0) { frame = frame.stop(Axis::Vertical); }
        }
        let pointer = self.seat.get_pointer().unwrap();
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// Return a bounding box covering all outputs.
    fn outputs_bounding_size(&self) -> (i32, i32) {
        let mut maxx = 0; let mut maxy = 0;
        for m in &self.monitors {
            maxx = maxx.max(m.geometry.loc.x + m.geometry.size.w);
            maxy = maxy.max(m.geometry.loc.y + m.geometry.size.h);
        }
        if maxx == 0 { maxx = 1920; }
        if maxy == 0 { maxy = 1080; }
        (maxx, maxy)
    }
}

// keep xkb and KeyState import "used" if smithay reorganises later versions
const _: fn() = || { let _ = xkb::KEYMAP_FORMAT_TEXT_V1; };
