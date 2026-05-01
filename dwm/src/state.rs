// Central compositor state for dwm.

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Instant;

use smithay::desktop::{PopupManager, Space, Window, WindowSurfaceType};
use smithay::input::{Seat, SeatState};
use smithay::reexports::calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::utils::{Logical, Point};
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::wlr_layer::WlrLayerShellState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;

use crate::bar::Bar;
use crate::config::{KeyDef, LT_TILE};
use crate::monitor::Monitor;
use crate::udev::UdevData;

pub struct Dwm {
    pub start_time: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    // Smithay protocol states.
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,
    pub seat: Seat<Self>,

    // dwm state.
    pub monitors: Vec<Monitor>,
    pub selmon: usize,
    pub keys: Vec<KeyDef>,
    pub bar: Bar,
    pub stext: String,
    pub focus_counter: u64,
    pub running: bool,

    // Backend-specific data (udev: DRM nodes, GBM/EGL/GLES, libinput).
    pub backend: UdevData,
}

impl Dwm {
    pub fn new(
        event_loop: &mut EventLoop<'static, Self>,
        display: Display<Self>,
        backend: UdevData,
    ) -> Self {
        let dh = display.handle();
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let popups = PopupManager::default();

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "seat0");
        seat.add_keyboard(Default::default(), 200, 25)
            .expect("create keyboard");
        seat.add_pointer();

        let space = Space::default();
        let socket_name = init_wayland_listener(display, event_loop);
        let loop_signal = event_loop.get_signal();

        let bar = Bar::new();
        let keys = crate::config::build_keys();

        Self {
            start_time: Instant::now(),
            socket_name,
            display_handle: dh,
            space,
            loop_signal,
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
            monitors: Vec::new(),
            selmon: 0,
            keys,
            bar,
            stext: format!("dwm-{}", crate::VERSION),
            focus_counter: 0,
            running: true,
            backend,
        }
    }

    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        })
    }

    pub fn next_focus_order(&mut self) -> u64 {
        self.focus_counter += 1;
        self.focus_counter
    }

    /// Find the index of the Monitor whose Output matches the provided surface.
    pub fn monitor_index_for_window(&self, window: &Window) -> usize {
        crate::client::with(window, |c| c.monitor.min(self.monitors.len().saturating_sub(1)))
    }

    /// Index of the monitor at a given pointer location, falling back to selmon.
    pub fn monitor_at(&self, pos: Point<f64, Logical>) -> usize {
        for (i, m) in self.monitors.iter().enumerate() {
            if m.geometry.to_f64().contains(pos) {
                return i;
            }
        }
        self.selmon
    }
}

fn init_wayland_listener(display: Display<Dwm>, event_loop: &mut EventLoop<'static, Dwm>) -> OsString {
    let listening_socket = ListeningSocketSource::new_auto().expect("create listening socket");
    let socket_name = listening_socket.socket_name().to_os_string();

    let loop_handle = event_loop.handle();
    loop_handle
        .insert_source(listening_socket, move |client_stream, _, state| {
            state
                .display_handle
                .insert_client(client_stream, Arc::new(DwmClientData::default()))
                .expect("insert client");
        })
        .expect("insert listening source");

    loop_handle
        .insert_source(
            Generic::new(display, Interest::READ, Mode::Level),
            |_, display, state| {
                unsafe { display.get_mut().dispatch_clients(state).unwrap(); }
                Ok(PostAction::Continue)
            },
        )
        .expect("insert display source");
    socket_name
}

#[derive(Default)]
pub struct DwmClientData {
    pub compositor_state: CompositorClientState,
}

impl ClientData for DwmClientData {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}
