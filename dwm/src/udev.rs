// udev/DRM/GBM/EGL/GLES2 backend, Linux-only.
//
// This module is structurally modeled on the smithay-drm-extras `simple`
// example and the `anvil` reference compositor, simplified for dwm's needs.
//
// REQUIRES: a Linux machine with libseat, libudev, libdrm, libgbm, libinput,
// libxkbcommon, libwayland-server. Will not compile on macOS or Windows.

use std::collections::HashMap;
use std::error::Error;
use std::os::unix::io::OwnedFd;
use std::path::PathBuf;
use std::time::Duration;

use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::{
    DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, DrmSurface, GbmBufferedSurface,
};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::backend::udev::{primary_gpu, UdevBackend, UdevEvent};
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::{timer::{TimeoutAction, Timer}, EventLoop};
use smithay::reexports::wayland_server::Display;
use smithay::utils::{DeviceFd, Rectangle, Size, Transform};

use drm::control::{connector, crtc};
use input::Libinput;
use rustix::fs::OFlags;
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner};
use smithay_drm_extras::display_info;

use crate::monitor::Monitor;
use crate::state::Dwm;

/// Backend-specific state stored on `Dwm`.
pub struct UdevData {
    pub session: LibSeatSession,
    pub primary_gpu: Option<DrmNode>,
    pub devices: HashMap<DrmNode, UdevDevice>,
}

pub struct UdevDevice {
    pub drm: DrmDevice,
    pub gbm: GbmDevice<DrmDeviceFd>,
    pub egl_display: EGLDisplay,
    pub renderer: GlesRenderer,
    pub scanner: DrmScanner,
    pub surfaces: HashMap<crtc::Handle, UdevSurface>,
}

pub struct UdevSurface {
    pub output: Output,
    pub gbm_surface: GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>,
    pub mode: OutputMode,
}

pub fn run_udev() -> Result<(), Box<dyn Error>> {
    let mut event_loop: EventLoop<'static, Dwm> = EventLoop::try_new()?;
    let display = Display::<Dwm>::new()?;

    let (session, session_notifier) = LibSeatSession::new()
        .map_err(|e| format!("LibSeatSession::new: {:?}", e))?;
    event_loop.handle()
        .insert_source(session_notifier, |_, _, _| {})
        .map_err(|e| format!("insert session_notifier: {:?}", e))?;

    let primary_gpu = primary_gpu(&session.seat()).ok().flatten()
        .and_then(|p| DrmNode::from_path(p).ok());

    let backend = UdevData {
        session,
        primary_gpu,
        devices: HashMap::new(),
    };

    let mut state = Dwm::new(&mut event_loop, display, backend);

    init_libinput(&mut event_loop, &mut state)?;
    init_udev(&mut event_loop, &mut state)?;

    // Export the WAYLAND_DISPLAY env var for spawned children.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &state.socket_name); }

    // A periodic tick keeps us pumping frames even when nothing else
    // wakes the loop. Real implementations should drive this off VBlank.
    event_loop.handle().insert_source(
        Timer::from_duration(Duration::from_millis(16)),
        |_, _, _| TimeoutAction::ToDuration(Duration::from_millis(16)),
    ).map_err(|e| format!("insert tick timer: {:?}", e))?;

    event_loop.run(Some(Duration::from_millis(16)), &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display_handle.flush_clients();
        // TODO: render outputs here. See anvil/src/udev.rs::render_surface.
        let _ = state;
    })?;

    Ok(())
}

fn init_libinput(event_loop: &mut EventLoop<'static, Dwm>, state: &mut Dwm)
    -> Result<(), Box<dyn Error>>
{
    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        state.backend.session.clone().into(),
    );
    libinput.udev_assign_seat(&state.backend.session.seat())
        .map_err(|_| "udev_assign_seat failed")?;
    let backend = LibinputInputBackend::new(libinput);
    event_loop.handle()
        .insert_source(backend, move |event, _, state| {
            state.process_input_event(event);
        })
        .map_err(|e| format!("insert libinput source: {:?}", e))?;
    Ok(())
}

fn init_udev(event_loop: &mut EventLoop<'static, Dwm>, state: &mut Dwm)
    -> Result<(), Box<dyn Error>>
{
    let backend = UdevBackend::new(&state.backend.session.seat())
        .map_err(|e| format!("UdevBackend::new: {:?}", e))?;
    for (device_id, path) in backend.device_list() {
        on_udev_event(state, &event_loop.handle(),
            UdevEvent::Added { device_id, path: path.to_owned() });
    }
    let handle = event_loop.handle();
    event_loop.handle()
        .insert_source(backend, move |event, _, state| {
            on_udev_event(state, &handle, event);
        })
        .map_err(|e| format!("insert udev source: {:?}", e))?;
    Ok(())
}

fn on_udev_event(
    state: &mut Dwm,
    loop_handle: &smithay::reexports::calloop::LoopHandle<'static, Dwm>,
    event: UdevEvent,
) {
    match event {
        UdevEvent::Added { device_id, path } => {
            if let Ok(node) = DrmNode::from_dev_id(device_id) {
                if let Err(e) = device_added(state, loop_handle, node, &path) {
                    tracing::warn!("device_added({:?}): {}", path, e);
                }
            }
        }
        UdevEvent::Changed { device_id } => {
            if let Ok(node) = DrmNode::from_dev_id(device_id) {
                device_changed(state, node);
            }
        }
        UdevEvent::Removed { device_id } => {
            if let Ok(node) = DrmNode::from_dev_id(device_id) {
                device_removed(state, node);
            }
        }
    }
}

fn device_added(
    state: &mut Dwm,
    loop_handle: &smithay::reexports::calloop::LoopHandle<'static, Dwm>,
    node: DrmNode,
    path: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    let fd: OwnedFd = state.backend.session
        .open(path, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)
        .map_err(|e| format!("session.open: {:?}", e))?;
    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), false)
        .map_err(|e| format!("DrmDevice::new: {:?}", e))?;

    loop_handle
        .insert_source(drm_notifier, move |event, _, _state| match event {
            DrmEvent::VBlank(_crtc) => {
                // TODO: drive a per-output render here, using DrmCompositor or
                // GbmBufferedSurface::frame_submitted.
            }
            DrmEvent::Error(err) => {
                tracing::error!("DRM error on {:?}: {}", node, err);
            }
        })
        .map_err(|e| format!("insert drm source: {:?}", e))?;

    let gbm = GbmDevice::new(drm_fd.clone())
        .map_err(|e| format!("GbmDevice::new: {:?}", e))?;
    // Safety: the EGLDisplay must outlive the renderer. We pin it inside the
    // device struct so that's true for the device's lifetime.
    let egl_display = unsafe { EGLDisplay::new(gbm.clone()) }
        .map_err(|e| format!("EGLDisplay::new: {:?}", e))?;
    let egl_context = EGLContext::new(&egl_display)
        .map_err(|e| format!("EGLContext::new: {:?}", e))?;
    let renderer = unsafe { GlesRenderer::new(egl_context) }
        .map_err(|e| format!("GlesRenderer::new: {:?}", e))?;

    let device = UdevDevice {
        drm,
        gbm,
        egl_display,
        renderer,
        scanner: DrmScanner::default(),
        surfaces: HashMap::new(),
    };
    state.backend.devices.insert(node, device);
    device_changed(state, node);
    Ok(())
}

fn device_changed(state: &mut Dwm, node: DrmNode) {
    let Some(device) = state.backend.devices.get_mut(&node) else { return };
    let scan = match device.scanner.scan_connectors(&device.drm) {
        Ok(events) => events.collect::<Vec<_>>(),
        Err(e) => { tracing::warn!("scan_connectors: {}", e); return; }
    };
    for event in scan {
        match event {
            DrmScanEvent::Connected { connector, crtc: Some(crtc) } => {
                if let Err(e) = connector_connected(state, node, connector, crtc) {
                    tracing::warn!("connector_connected: {}", e);
                }
            }
            DrmScanEvent::Disconnected { connector, crtc: Some(crtc) } => {
                connector_disconnected(state, node, connector, crtc);
            }
            _ => {}
        }
    }
}

fn connector_connected(
    state: &mut Dwm,
    node: DrmNode,
    connector: connector::Info,
    crtc: crtc::Handle,
) -> Result<(), Box<dyn Error>> {
    let device = state.backend.devices.get_mut(&node)
        .ok_or("device for connector not found")?;

    let drm_mode = connector.modes().get(0).ok_or("connector has no modes")?.clone();
    let mode: OutputMode = drm_mode.into();

    let name = format!("{}-{}",
        connector.interface().as_str(),
        connector.interface_id());

    let info = display_info::for_connector(&device.drm, connector.handle());
    let make = info.as_ref().and_then(|i| i.make()).unwrap_or_else(|| "Unknown".into());
    let model = info.as_ref().and_then(|i| i.model()).unwrap_or_else(|| "Unknown".into());

    let physical = PhysicalProperties {
        size: Size::from((
            connector.size().map(|s| s.0 as i32).unwrap_or(0),
            connector.size().map(|s| s.1 as i32).unwrap_or(0),
        )),
        subpixel: Subpixel::Unknown,
        make,
        model,
    };
    let output = Output::new(name, physical);
    let _ = output.create_global::<Dwm>(&state.display_handle);
    output.change_current_state(Some(mode), Some(Transform::Normal), None,
        Some(smithay::utils::Point::from((0, 0))));
    output.set_preferred(mode);

    // Create the GBM-backed scanout surface for this CRTC.
    let drm_surface: DrmSurface = device.drm
        .create_surface(crtc, drm_mode, &[connector.handle()])
        .map_err(|e| format!("create_surface: {:?}", e))?;
    let allocator = GbmAllocator::new(
        device.gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let gbm_surface = GbmBufferedSurface::new(
        drm_surface,
        allocator,
        device.egl_display.dmabuf_render_formats().clone(),
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
    ).map_err(|e| format!("GbmBufferedSurface::new: {:?}", e))?;

    device.surfaces.insert(crtc, UdevSurface { output: output.clone(), gbm_surface, mode });

    // Add the output as a smithay Monitor + Space view.
    let mode_size = mode.size;
    let geom = Rectangle::new(
        smithay::utils::Point::from((next_output_x(state), 0)),
        Size::from((mode_size.w as i32, mode_size.h as i32)),
    );
    state.space.map_output(&output, geom.loc);
    let monitor = Monitor::new(output, geom, state.monitors.len() as i32);
    state.monitors.push(monitor);
    if state.monitors.len() == 1 {
        state.selmon = 0;
    }
    Ok(())
}

fn connector_disconnected(state: &mut Dwm, node: DrmNode,
    _connector: connector::Info, crtc: crtc::Handle)
{
    if let Some(device) = state.backend.devices.get_mut(&node) {
        if let Some(surface) = device.surfaces.remove(&crtc) {
            let output = surface.output.clone();
            state.space.unmap_output(&output);
            state.monitors.retain(|m| m.output != output);
        }
    }
}

fn device_removed(state: &mut Dwm, node: DrmNode) {
    if let Some(mut device) = state.backend.devices.remove(&node) {
        let crtcs: Vec<_> = device.surfaces.keys().copied().collect();
        for crtc in crtcs {
            if let Some(surface) = device.surfaces.remove(&crtc) {
                let output = surface.output;
                state.space.unmap_output(&output);
                state.monitors.retain(|m| m.output != output);
            }
        }
    }
}

fn next_output_x(state: &Dwm) -> i32 {
    state.monitors.iter().map(|m| m.geometry.loc.x + m.geometry.size.w).max().unwrap_or(0)
}

// Suppress warnings if some imports are unused in a stripped build.
const _: fn() = || { let _: Option<Dmabuf> = None; };
