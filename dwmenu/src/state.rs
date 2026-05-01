// Menu state and all SCTK handler delegations.

use std::error::Error;
use std::io::{self, BufRead};
use std::num::NonZeroU32;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::GlobalList,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::match_logic::Item;
use crate::render::{load_font, FontFace};
use crate::Args;

pub struct Menu {
    pub args: Args,
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub shm: Shm,
    pub pool: SlotPool,
    pub layer: LayerSurface,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub modifiers: Modifiers,

    pub font: FontFace,
    pub configured: bool,
    pub exit_code: Option<u8>,

    // Geometry (in surface-local pixels).
    pub mw: i32,
    pub mh: i32,
    pub bh: i32,
    pub inputw: i32,
    pub promptw: i32,

    // Items + match list.
    pub items: Vec<Item>,
    pub matches: Option<usize>,
    pub matchend: Option<usize>,
    pub prev: Option<usize>,
    pub curr: Option<usize>,
    pub next: Option<usize>,
    pub sel: Option<usize>,

    // Editable input text (raw bytes; multi-byte UTF-8 sequences allowed).
    pub text: Vec<u8>,
    pub cursor: usize,

    // Cached prompt bytes.
    pub prompt: Option<Vec<u8>>,

    // Initial layer-shell-suggested width in pixels (0 == not yet known).
    pub layer_w: u32,
    pub layer_h: u32,
}

impl Menu {
    pub fn new(
        _conn: &Connection,
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        args: Args,
    ) -> Result<Self, Box<dyn Error>> {
        let compositor = CompositorState::bind(globals, qh)
            .map_err(|_| "wl_compositor not available")?;
        let layer_shell = LayerShell::bind(globals, qh)
            .map_err(|_| "wlr-layer-shell not available")?;
        let shm = Shm::bind(globals, qh).map_err(|_| "wl_shm not available")?;

        let surface = compositor.create_surface(qh);
        let layer = layer_shell.create_layer_surface(qh, surface, Layer::Top, Some("dmenu"), None);
        let anchor = if args.topbar {
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
        } else {
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT
        };
        layer.set_anchor(anchor);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        // Initial size: width=0 means "use the output's full width".
        let h_guess = (args.font_size as i32 + 2) * (args.lines.max(0) + 1);
        layer.set_size(0, h_guess.max(1) as u32);
        layer.set_exclusive_zone(h_guess.max(1));
        layer.commit();

        let pool = SlotPool::new(1024 * 32 * 4, &shm)
            .map_err(|e| format!("create shm pool: {:?}", e))?;

        let font = load_font(args.font.as_deref(), args.font_size)?;
        let prompt = args.prompt.as_ref().map(|s| s.as_bytes().to_vec());
        let bh = font.height() + 2;

        Ok(Self {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            shm,
            pool,
            layer,
            keyboard: None,
            modifiers: Modifiers::default(),
            font,
            configured: false,
            exit_code: None,
            mw: 0,
            mh: bh * (args.lines.max(0) + 1),
            bh,
            inputw: 0,
            promptw: 0,
            items: Vec::new(),
            matches: None,
            matchend: None,
            prev: None,
            curr: None,
            next: None,
            sel: None,
            text: Vec::new(),
            cursor: 0,
            prompt,
            args,
            layer_w: 0,
            layer_h: 0,
        })
    }
}

impl Menu {
    pub fn read_stdin(&mut self) -> Result<(), Box<dyn Error>> {
        let stdin = io::stdin();
        let mut max_w = 0i32;
        for line in stdin.lock().lines() {
            let line = line?;
            let bytes = line.into_bytes();
            let w = self.font.measure(&bytes);
            if w > max_w { max_w = w; }
            self.items.push(Item { text: bytes, left: None, right: None });
        }
        self.inputw = max_w;
        Ok(())
    }

    pub fn text_w(&self, s: &[u8]) -> i32 {
        self.font.measure(s) + self.font.height()
    }

    pub fn recompute_geometry(&mut self) {
        // Width: if the compositor gave us 0 (full output width), default to a sensible value.
        let mw = if self.layer_w == 0 { 800 } else { self.layer_w as i32 };
        self.mw = mw;
        self.promptw = self.prompt.as_ref().map(|p| self.text_w(p)).unwrap_or(0);
        self.inputw = self.inputw.min(self.mw / 3);
        self.mh = self.bh * (self.args.lines.max(0) + 1);
    }

    pub fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        use crate::render::{paint_rect, paint_text};

        let w = self.mw.max(1);
        let h = self.mh.max(1);
        let stride = w * 4;

        let (buffer, canvas) = self.pool
            .create_buffer(w, h, stride, wl_shm::Format::Argb8888)
            .map_err(|e| format!("create_buffer: {:?}", e))?;

        // Background.
        let normbg = self.args.normbg;
        let normfg = self.args.normfg;
        let selbg = self.args.selbg;
        let selfg = self.args.selfg;
        paint_rect(canvas, stride as usize, w, h, 0, 0, w, h, normbg);

        // Vertical centering offset within the bar.
        let pad_x = self.font.height() / 2;
        let text_y = (self.bh - self.font.height()) / 2;

        let mut x = 0i32;
        if let Some(p) = self.prompt.clone() {
            paint_rect(canvas, stride as usize, w, h, x, 0, self.promptw, self.bh, selbg);
            paint_text(canvas, stride as usize, w, h, &self.font,
                x + pad_x, text_y, &p, selfg);
            x += self.promptw;
        }
        let input_w = if self.args.lines > 0 || self.matches.is_none() {
            self.mw - x
        } else {
            self.inputw
        };
        let text = self.text.clone();
        paint_text(canvas, stride as usize, w, h, &self.font,
            x + pad_x, text_y, &text, normfg);
        // Cursor caret.
        let cur_w = self.font.measure(&self.text[..self.cursor]);
        let cx = x + pad_x + cur_w;
        if cx < x + input_w {
            paint_rect(canvas, stride as usize, w, h,
                cx, text_y, 1, self.font.height(), normfg);
        }

        if self.args.lines > 0 {
            let mut item = self.curr;
            let mut ry = self.bh;
            while item != self.next {
                let idx = match item { Some(i) => i, None => break };
                let (bg, fg) = if Some(idx) == self.sel { (selbg, selfg) } else { (normbg, normfg) };
                paint_rect(canvas, stride as usize, w, h, 0, ry, w, self.bh, bg);
                let txt = self.items[idx].text.clone();
                paint_text(canvas, stride as usize, w, h, &self.font,
                    pad_x, ry + text_y, &txt, fg);
                ry += self.bh;
                item = self.items[idx].right;
            }
        } else if self.matches.is_some() {
            let lt_w = self.text_w(b"<");
            let gt_w = self.text_w(b">");
            let mut hx = x + self.inputw;
            if self.curr.and_then(|c| self.items[c].left).is_some() {
                paint_text(canvas, stride as usize, w, h, &self.font,
                    hx + pad_x, text_y, b"<", normfg);
            }
            hx += lt_w;
            let mut item = self.curr;
            while item != self.next {
                let idx = match item { Some(i) => i, None => break };
                let item_w = self.text_w(&self.items[idx].text.clone()).min(self.mw - hx - gt_w);
                let (bg, fg) = if Some(idx) == self.sel { (selbg, selfg) } else { (normbg, normfg) };
                paint_rect(canvas, stride as usize, w, h, hx, 0, item_w, self.bh, bg);
                let txt = self.items[idx].text.clone();
                paint_text(canvas, stride as usize, w, h, &self.font,
                    hx + pad_x, text_y, &txt, fg);
                hx += item_w;
                item = self.items[idx].right;
            }
            if self.next.is_some() {
                paint_text(canvas, stride as usize, w, h, &self.font,
                    self.mw - gt_w + pad_x, text_y, b">", normfg);
            }
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w, h);
        surface.frame(qh, surface.clone());
        buffer.attach_to(surface).map_err(|e| format!("attach: {:?}", e))?;
        self.layer.commit();
        Ok(())
    }
}


impl CompositorHandler for Menu {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface, _: u32) {
        let _ = self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Menu {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Menu {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit_code = Some(1);
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>,
        _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32)
    {
        self.layer_w = NonZeroU32::new(configure.new_size.0).map_or(800, NonZeroU32::get);
        self.layer_h = NonZeroU32::new(configure.new_size.1).map_or(self.mh as u32, NonZeroU32::get);
        if !self.configured {
            self.configured = true;
            // first draw will be triggered by main once stdin is consumed
            return;
        }
        self.recompute_geometry();
        let _ = self.draw(qh);
    }
}

impl SeatHandler for Menu {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat, cap: Capability)
    {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            if let Ok(kb) = self.seat_state.get_keyboard(qh, &seat, None) {
                self.keyboard = Some(kb);
            }
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: wl_seat::WlSeat, cap: Capability)
    {
        if cap == Capability::Keyboard {
            if let Some(kb) = self.keyboard.take() { kb.release(); }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Menu {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface,
        _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent)
    {
        crate::input::handle_key(self, event);
        if self.exit_code.is_none() {
            let _ = self.draw(qh);
        }
    }
    fn repeat_key(&mut self, _: &Connection, qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent)
    {
        crate::input::handle_key(self, event);
        if self.exit_code.is_none() {
            let _ = self.draw(qh);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers,
        _: RawModifiers, _: u32)
    {
        self.modifiers = modifiers;
    }
}

impl ShmHandler for Menu {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

impl ProvidesRegistryState for Menu {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Menu);
delegate_output!(Menu);
delegate_shm!(Menu);
delegate_seat!(Menu);
delegate_keyboard!(Menu);
delegate_layer!(Menu);
delegate_registry!(Menu);
