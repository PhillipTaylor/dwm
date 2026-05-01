// Wayland protocol handler implementations.

use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::desktop::{find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
    LayerSurface, PopupKind, PopupManager, Space, Window, WindowSurfaceType};
use smithay::input::pointer::{CursorImageStatus, Focus, GrabStartData as PointerGrabStartData};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_seat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Rectangle, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    add_pre_commit_hook, get_parent, is_sync_subsurface, with_states, CompositorClientState,
    CompositorHandler, CompositorState,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::wlr_layer::{
    Layer, LayerSurface as WlrLayerSurface, WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_shell,
};

use crate::grabs::{MoveSurfaceGrab, ResizeSurfaceGrab};
use crate::state::{Dwm, DwmClientData};

impl CompositorHandler for Dwm {
    fn compositor_state(&mut self) -> &mut CompositorState { &mut self.compositor_state }

    fn client_compositor_state<'a>(
        &self,
        client: &'a smithay::reexports::wayland_server::Client,
    ) -> &'a CompositorClientState {
        &client.get_data::<DwmClientData>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().map(|t| t.wl_surface() == &root).unwrap_or(false))
                .cloned()
            {
                window.on_commit();
                let initial_configure_sent = with_states(&root, |states| {
                    states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .map(|d| d.lock().unwrap().initial_configure_sent)
                        .unwrap_or(true)
                });
                if !initial_configure_sent {
                    if let Some(t) = window.toplevel() { t.send_configure(); }
                }
            }
        }

        self.popups.commit(surface);
        if let Some(popup) = self.popups.find_popup(surface) {
            if let PopupKind::Xdg(xdg) = &popup {
                if !xdg.is_initial_configure_sent() {
                    let _ = xdg.send_configure();
                }
            }
        }
    }
}

impl BufferHandler for Dwm {
    fn buffer_destroyed(&mut self, _: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer) {}
}

impl ShmHandler for Dwm {
    fn shm_state(&self) -> &ShmState { &self.shm_state }
}

impl OutputHandler for Dwm {}

impl SeatHandler for Dwm {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> { &mut self.seat_state }

    fn cursor_image(&mut self, _: &Seat<Self>, _: CursorImageStatus) {}

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

impl SelectionHandler for Dwm {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Dwm {
    fn data_device_state(&self) -> &DataDeviceState { &self.data_device_state }
}

impl ClientDndGrabHandler for Dwm {}
impl ServerDndGrabHandler for Dwm {}

impl WlrLayerShellHandler for Dwm {
    fn shell_state(&mut self) -> &mut WlrLayerShellState { &mut self.layer_shell_state }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let _ = namespace;
        // Pick the output: requested one, else the selected monitor's.
        let output: Output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.monitors.get(self.selmon).map(|m| m.output.clone()))
            .expect("layer surface but no output");
        let mut map = layer_map_for_output(&output);
        let _ = map.map_layer(&LayerSurface::new(surface, String::new()));
    }
}


impl XdgShellHandler for Dwm {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState { &mut self.xdg_shell_state }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        let mon = crate::dwm_logic::manage_new_window(self, &window);
        let pos = self.monitors[mon].work_area.loc;
        self.space.map_element(window.clone(), pos, false);
        crate::dwm_logic::arrange_monitor(self, mon);
        crate::dwm_logic::focus(self, Some(window));
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        unconstrain_popup(&self.space, &mut self.popups, &surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        unconstrain_popup(&self.space, &mut self.popups, &surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else { return };
        let pointer = seat.get_pointer().unwrap();
        let window = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface() == wl_surface).unwrap_or(false))
            .cloned()
            .unwrap();
        let initial_window_location = self.space.element_location(&window).unwrap();
        let grab = MoveSurfaceGrab { start_data, window, initial_window_location };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else { return };
        let pointer = seat.get_pointer().unwrap();
        let window = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface() == wl_surface).unwrap_or(false))
            .cloned()
            .unwrap();
        let initial_window_location = self.space.element_location(&window).unwrap();
        let initial_window_size = window.geometry().size;

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
        });
        surface.send_pending_configure();

        let grab = ResizeSurfaceGrab::start(
            start_data,
            window,
            edges.into(),
            Rectangle::new(initial_window_location, initial_window_size),
        );
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
}

fn check_grab(
    seat: &Seat<Dwm>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<Dwm>> {
    let pointer = seat.get_pointer()?;
    if !pointer.has_grab(serial) { return None; }
    let start_data = pointer.grab_start_data()?;
    let (focus, _) = start_data.focus.as_ref()?;
    if !focus.id().same_client_as(&surface.id()) { return None; }
    Some(start_data)
}

fn unconstrain_popup(space: &Space<Window>, _popups: &mut PopupManager, popup: &PopupSurface) {
    let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else { return; };
    let Some(window) = space.elements()
        .find(|w| w.toplevel().map(|t| t.wl_surface() == &root).unwrap_or(false))
    else { return; };

    let Some(output) = space.outputs().next() else { return; };
    let Some(output_geo) = space.output_geometry(output) else { return; };
    let Some(window_geo) = space.element_geometry(window) else { return; };

    let mut target = output_geo;
    target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
    target.loc -= window_geo.loc;
    popup.with_pending_state(|state| {
        state.geometry = state.positioner.get_unconstrained_geometry(target);
    });
}

delegate_compositor!(Dwm);
delegate_xdg_shell!(Dwm);
delegate_layer_shell!(Dwm);
delegate_output!(Dwm);
delegate_seat!(Dwm);
delegate_data_device!(Dwm);
delegate_shm!(Dwm);
