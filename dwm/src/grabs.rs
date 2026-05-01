// Pointer grabs used to drive interactive move/resize. Mirrors smallvil
// with the dwm twist that move/resize toggle a window into floating mode.

use std::sync::Mutex;

use smithay::desktop::{space::SpaceElement, Window};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
    GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
    PointerInnerHandle, RelativeMotionEvent,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::client;
use crate::state::Dwm;

pub const BTN_LEFT: u32 = 0x110;

pub struct MoveSurfaceGrab {
    pub start_data: PointerGrabStartData<Dwm>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
}

impl PointerGrab<Dwm> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut Dwm,
        handle: &mut PointerInnerHandle<'_, Dwm>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let new_location = self.initial_window_location.to_f64() + delta;
        // Mark window as floating during a move.
        client::with_mut(&self.window, |c| { c.isfloating = true; });
        data.space.map_element(self.window.clone(), new_location.to_i32_round(), true);
    }

    fn relative_motion(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>,
        focus: Option<(WlSurface, Point<f64, Logical>)>, event: &RelativeMotionEvent)
    { handle.relative_motion(data, focus, event); }

    fn button(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &ButtonEvent) {
        handle.button(data, event);
        if !handle.current_pressed().contains(&BTN_LEFT) {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, details: AxisFrame)
    { handle.axis(data, details) }

    fn frame(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>) { handle.frame(data); }

    fn gesture_swipe_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeBeginEvent)
    { handle.gesture_swipe_begin(data, event) }
    fn gesture_swipe_update(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeUpdateEvent)
    { handle.gesture_swipe_update(data, event) }
    fn gesture_swipe_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeEndEvent)
    { handle.gesture_swipe_end(data, event) }
    fn gesture_pinch_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchBeginEvent)
    { handle.gesture_pinch_begin(data, event) }
    fn gesture_pinch_update(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchUpdateEvent)
    { handle.gesture_pinch_update(data, event) }
    fn gesture_pinch_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchEndEvent)
    { handle.gesture_pinch_end(data, event) }
    fn gesture_hold_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureHoldBeginEvent)
    { handle.gesture_hold_begin(data, event) }
    fn gesture_hold_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureHoldEndEvent)
    { handle.gesture_hold_end(data, event) }

    fn start_data(&self) -> &PointerGrabStartData<Dwm> { &self.start_data }
    fn unset(&mut self, _data: &mut Dwm) {}
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ResizeEdge: u32 {
        const TOP    = 1 << 0;
        const BOTTOM = 1 << 1;
        const LEFT   = 1 << 2;
        const RIGHT  = 1 << 3;
        const TOP_LEFT     = Self::TOP.bits()    | Self::LEFT.bits();
        const BOTTOM_LEFT  = Self::BOTTOM.bits() | Self::LEFT.bits();
        const TOP_RIGHT    = Self::TOP.bits()    | Self::RIGHT.bits();
        const BOTTOM_RIGHT = Self::BOTTOM.bits() | Self::RIGHT.bits();
    }
}

impl From<xdg_toplevel::ResizeEdge> for ResizeEdge {
    fn from(x: xdg_toplevel::ResizeEdge) -> Self {
        Self::from_bits(x as u32).unwrap_or(ResizeEdge::empty())
    }
}

pub struct ResizeSurfaceState {
    edges: ResizeEdge,
    initial_rect: Rectangle<i32, Logical>,
}

impl Default for ResizeSurfaceState {
    fn default() -> Self {
        Self {
            edges: ResizeEdge::empty(),
            initial_rect: Rectangle::default(),
        }
    }
}

pub struct ResizeSurfaceGrab {
    start_data: PointerGrabStartData<Dwm>,
    window: Window,
    edges: ResizeEdge,
    initial_rect: Rectangle<i32, Logical>,
    last_window_size: Size<i32, Logical>,
}

impl ResizeSurfaceGrab {
    pub fn start(start_data: PointerGrabStartData<Dwm>, window: Window,
        edges: ResizeEdge, initial_rect: Rectangle<i32, Logical>) -> Self
    {
        let initial_size = initial_rect.size;
        let state = window.user_data();
        state.insert_if_missing_threadsafe(|| Mutex::new(ResizeSurfaceState::default()));
        if let Some(s) = state.get::<Mutex<ResizeSurfaceState>>() {
            let mut s = s.lock().unwrap();
            s.edges = edges;
            s.initial_rect = initial_rect;
        }
        Self {
            start_data, window, edges, initial_rect,
            last_window_size: initial_size,
        }
    }
}

impl PointerGrab<Dwm> for ResizeSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut Dwm,
        handle: &mut PointerInnerHandle<'_, Dwm>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let mut new_size = self.initial_rect.size;
        if self.edges.contains(ResizeEdge::LEFT)   { new_size.w = (self.initial_rect.size.w as f64 - delta.x) as i32; }
        if self.edges.contains(ResizeEdge::RIGHT)  { new_size.w = (self.initial_rect.size.w as f64 + delta.x) as i32; }
        if self.edges.contains(ResizeEdge::TOP)    { new_size.h = (self.initial_rect.size.h as f64 - delta.y) as i32; }
        if self.edges.contains(ResizeEdge::BOTTOM) { new_size.h = (self.initial_rect.size.h as f64 + delta.y) as i32; }
        new_size.w = new_size.w.max(1);
        new_size.h = new_size.h.max(1);
        self.last_window_size = new_size;
        if let Some(toplevel) = self.window.toplevel() {
            toplevel.with_pending_state(|s| s.size = Some(new_size));
            toplevel.send_pending_configure();
        }
    }

    fn relative_motion(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>,
        focus: Option<(WlSurface, Point<f64, Logical>)>, event: &RelativeMotionEvent)
    { handle.relative_motion(data, focus, event); }

    fn button(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &ButtonEvent) {
        handle.button(data, event);
        if !handle.current_pressed().contains(&BTN_LEFT) {
            handle.unset_grab(self, data, event.serial, event.time, true);
            // Force into floating mode after a resize.
            client::with_mut(&self.window, |c| { c.isfloating = true; });
            if let Some(t) = self.window.toplevel() {
                t.with_pending_state(|s| { s.states.unset(xdg_toplevel::State::Resizing); });
                t.send_pending_configure();
            }
            // Apply the new geometry to the space.
            data.space.map_element(self.window.clone(),
                self.initial_rect.loc, true);
            client::with_mut(&self.window, |c| {
                c.stored_x = self.initial_rect.loc.x;
                c.stored_y = self.initial_rect.loc.y;
                c.stored_w = self.last_window_size.w;
                c.stored_h = self.last_window_size.h;
            });
        }
    }

    fn axis(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, details: AxisFrame)
    { handle.axis(data, details) }
    fn frame(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>) { handle.frame(data); }
    fn gesture_swipe_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeBeginEvent)
    { handle.gesture_swipe_begin(data, event) }
    fn gesture_swipe_update(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeUpdateEvent)
    { handle.gesture_swipe_update(data, event) }
    fn gesture_swipe_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureSwipeEndEvent)
    { handle.gesture_swipe_end(data, event) }
    fn gesture_pinch_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchBeginEvent)
    { handle.gesture_pinch_begin(data, event) }
    fn gesture_pinch_update(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchUpdateEvent)
    { handle.gesture_pinch_update(data, event) }
    fn gesture_pinch_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GesturePinchEndEvent)
    { handle.gesture_pinch_end(data, event) }
    fn gesture_hold_begin(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureHoldBeginEvent)
    { handle.gesture_hold_begin(data, event) }
    fn gesture_hold_end(&mut self, data: &mut Dwm, handle: &mut PointerInnerHandle<'_, Dwm>, event: &GestureHoldEndEvent)
    { handle.gesture_hold_end(data, event) }

    fn start_data(&self) -> &PointerGrabStartData<Dwm> { &self.start_data }
    fn unset(&mut self, _data: &mut Dwm) {}
}

// Suppress unused warnings for SpaceElement import.
const _: fn(&Window) = |w| { let _ = w.bbox(); };
