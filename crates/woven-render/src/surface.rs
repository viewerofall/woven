//! wlr-layer-shell surface.
//!
//! THE LAW:
//!   1. Surface maps ONCE at startup and stays mapped forever.
//!      No null-buffer commits after first map. Ever.
//!   2. Every commit goes through layer_surface.commit() — never surf.commit().
//!   3. Hide = keyboard=None. Show = keyboard=Exclusive.
//!      Both committed with layer_surface.commit() while buffer remains attached.
//!
//! WHY NO NULL-BUFFER:
//!   Null-buffer unmap clears ALL double-buffered layer-shell state (anchors,
//!   exclusive_zone, size, keyboard_interactivity). Re-staging them requires
//!   a commit, which requires a configure ack, which requires the compositor to
//!   send a new configure — a race we cannot reliably win. Niri may reject the
//!   re-map commit with error 1 if anchors aren't re-set before size(0,0).
//!   Avoiding unmap entirely eliminates this whole class of error.
//!
//! VISUAL HIDE:
//!   The render thread's close animation fades anim_t to 0, making the final
//!   frame fully transparent. The compositor keeps rendering our buffer but the
//!   pixels are ARGB(0,0,0,0) — invisible. keyboard=None means no focus steal.

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyboardHandler, KeyEvent, Keysym, Modifiers, RepeatInfo},
        pointer::{
            CursorIcon, PointerEvent, PointerEventKind,
            PointerHandler, ThemedPointer, ThemeSpec,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler,
            LayerSurface as SctLayerSurface, LayerSurfaceConfigure,
        },
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
    Connection, EventQueue, QueueHandle,
};
use crossbeam_channel::Sender;
use tracing::info;

#[derive(Debug, Clone)]
pub enum MouseEvent {
    Motion      { x: f64, y: f64 },
    Scroll      { dx: f64, dy: f64 },
    Press       { x: f64, y: f64 },
    RightPress  { x: f64, y: f64 },
    Release     { x: f64, y: f64 },
    Key         { keysym: u32 },
}

pub struct WovenSurface {
    queue: EventQueue<WovenState>,
    state: WovenState,
}

impl WovenSurface {
    pub fn new(mouse_tx: Sender<MouseEvent>) -> Result<Self> {
        let conn = Connection::connect_to_env()
        .context("Failed to connect to Wayland display")?;
        let (globals, queue) = registry_queue_init::<WovenState>(&conn)
        .context("Failed to init Wayland registry")?;
        let qh = queue.handle();

        let compositor  = CompositorState::bind(&globals, &qh).context("wl_compositor missing")?;
        let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell missing")?;
        let shm         = Shm::bind(&globals, &qh).context("wl_shm missing")?;
        let seat_state  = SeatState::new(&globals, &qh);

        let surface = compositor.create_surface(&qh);
        let layer_surface = layer_shell.create_layer_surface(
            &qh, surface, Layer::Overlay,
            Some("woven-overlay"), None,
        );
        // Set permanent properties and do the ONE initial map commit.
        // keyboard=None — invisible and non-interactive until show() is called.
        layer_surface.set_anchor(Anchor::all());
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer_surface.set_size(0, 0);
        layer_surface.commit(); // → compositor sends configure → we ack in show()

        let pool = SlotPool::new(8 * 1024 * 1024, &shm).context("shm pool failed")?;

        let state = WovenState {
            registry:     RegistryState::new(&globals),
            compositor,
            output_state: OutputState::new(&globals, &qh),
            seat_state,
            shm,
            layer_surface,
            pool,
            pointer:    None,
            keyboard:   None,
            mouse_tx,
            mouse_x:    0.0,
            mouse_y:    0.0,
            width:      0,
            height:     0,
            configured: false,
        };

        Ok(Self { queue, state })
    }

    /// Make overlay interactive. First call waits for initial configure.
    /// Subsequent calls: ack any pending configure from hide(), commit
    /// keyboard=Exclusive, then roundtrip to process Niri's configure
    /// response before present() needs to ack it.
    pub fn show(&mut self) -> Result<()> {
        if !self.state.configured {
            // First show — spin until compositor sends screen dimensions.
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while !self.state.configured && std::time::Instant::now() < deadline {
                let _ = self.queue.flush();
                if let Some(g) = self.queue.prepare_read() { let _ = g.read(); }
                let _ = self.queue.dispatch_pending(&mut self.state);
            }
            if !self.state.configured {
                tracing::warn!("compositor configure timeout — proceeding");
                self.state.configured = true;
            }
        } else {
            // hide() already closed its configure cycle via roundtrip; just
            // drain any residual buffered events before committing below.
            let _ = self.queue.dispatch_pending(&mut self.state);
        }

        // Restore full input region so pointer events land on the overlay.
        self.state.layer_surface.wl_surface().set_input_region(None);
        self.state.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        self.state.layer_surface.commit();

        // Roundtrip: Niri sends a configure in response to our keyboard=Exclusive
        // commit. We must process it here before returning — the render loop's
        // non-blocking dispatch() between show() and present() races against it
        // arriving, and present()'s commit would go out without acking it →
        // zwlr_layer_surface_v1 error 1 (invalid_surface_state).
        self.queue.roundtrip(&mut self.state).context("show roundtrip")?;
        Ok(())
    }

    /// Release focus. Set keyboard=None and commit. Buffer stays attached — surface
    /// stays mapped and transparent, just non-interactive.
    pub fn hide(&mut self) -> Result<()> {
        // Empty input region: pointer events pass through the transparent surface.
        // Without this the surface absorbs all clicks even when fully transparent.
        let qh = self.queue.handle();
        let region = self.state.compositor.wl_compositor().create_region(&qh, ());
        // No rects added → empty region → click-through.
        self.state.layer_surface.wl_surface().set_input_region(Some(&region));
        region.destroy();

        self.state.layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        self.state.layer_surface.commit();
        // Roundtrip: Niri sends a configure in response to keyboard=None. Ack it
        // here so no stale configure is left in the socket for the next show().
        self.queue.roundtrip(&mut self.state).context("hide roundtrip")?;
        Ok(())
    }

    // Legacy no-ops — hide/show handle keyboard directly.
    pub fn release_input(&mut self) {}
    pub fn grab_input(&mut self) {}

    pub fn is_visible(&self) -> bool { self.state.configured }

    pub fn size(&self) -> (u32, u32) {
        (self.state.width, self.state.height)
    }

    pub fn dispatch(&mut self) -> Result<()> {
        if let Err(e) = self.queue.flush() {
            tracing::debug!("wayland flush: {e}");
        }
        if let Some(guard) = self.queue.prepare_read() {
            use std::os::unix::io::AsRawFd;
            use rustix::fd::AsFd;
            use rustix::event::{PollFd, PollFlags, poll};
            use rustix::time::Timespec;
            let raw      = self.queue.as_fd().as_raw_fd();
            let borrowed = unsafe { rustix::fd::BorrowedFd::borrow_raw(raw) };
            let mut pfd  = PollFd::new(&borrowed, PollFlags::IN);
            let ts       = Timespec { tv_sec: 0, tv_nsec: 0 };
            let ready    = poll(std::slice::from_mut(&mut pfd), Some(&ts)).unwrap_or(0);
            if ready > 0 { let _ = guard.read(); } else { drop(guard); }
        }
        self.queue.dispatch_pending(&mut self.state).context("dispatch failed")?;
        Ok(())
    }

    pub fn present(&mut self, pixels: Vec<u8>, width: u32, height: u32) -> Result<()> {
        if self.state.width == 0 || self.state.height == 0 { return Ok(()); }
        let stride = width * 4;
        let (buffer, canvas) = self.state.pool
        .create_buffer(width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888)
        .context("create_buffer failed")?;
        let n = canvas.len().min(pixels.len());
        canvas[..n].copy_from_slice(&pixels[..n]);
        let surf = self.state.layer_surface.wl_surface();
        buffer.attach_to(surf).context("attach failed")?;
        surf.damage_buffer(0, 0, width as i32, height as i32);
        // layer_surface.commit() = ack_configure (SCT) + wl_surface.commit().
        self.state.layer_surface.commit();
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────

struct WovenState {
    registry:     RegistryState,
    compositor:   CompositorState,
    output_state: OutputState,
    seat_state:   SeatState,
    shm:          Shm,
    layer_surface: SctLayerSurface,
    pool:         SlotPool,
    pointer:      Option<ThemedPointer>,
    keyboard:     Option<wl_keyboard::WlKeyboard>,
    mouse_tx:     Sender<MouseEvent>,
    mouse_x:      f64,
    mouse_y:      f64,
    width:        u32,
    height:       u32,
    configured:   bool,
}

impl CompositorHandler for WovenState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for WovenState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for WovenState {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _conn: &Connection, qh: &QueueHandle<Self>,
                      seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            let cs = self.compositor.create_surface(qh);
            match self.seat_state.get_pointer_with_theme(qh, &seat, self.shm.wl_shm(), cs, ThemeSpec::System) {
                Ok(p) => { self.pointer = Some(p); info!("seat: pointer ready"); }
                Err(e) => tracing::warn!("pointer: {e}"),
            }
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(k) => { self.keyboard = Some(k); info!("seat: keyboard ready"); }
                Err(e) => tracing::warn!("keyboard: {e}"),
            }
        }
                      }

                      fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                           _: wl_seat::WlSeat, cap: Capability) {
                          if cap == Capability::Pointer  { self.pointer  = None; }
                          if cap == Capability::Keyboard { self.keyboard = None; }
                                           }
}

impl KeyboardHandler for WovenState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard,
             _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
             fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard,
                      _: &wl_surface::WlSurface, _: u32) {}
                      fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                   _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
                          let _ = self.mouse_tx.try_send(MouseEvent::Key { keysym: event.keysym.raw() });
                                   }
                                   fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                  _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
                                                  fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                                      _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers,
                                                                      _: smithay_client_toolkit::seat::keyboard::RawModifiers, _: u32) {}
                                                                      fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                                                    _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
                                                                          let _ = self.mouse_tx.try_send(MouseEvent::Key { keysym: event.keysym.raw() });
                                                                                    }
                                                                                    fn update_repeat_info(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                                                                          _: &wl_keyboard::WlKeyboard, _: RepeatInfo) {}
}

impl PointerHandler for WovenState {
    fn pointer_frame(&mut self, conn: &Connection, _qh: &QueueHandle<Self>,
                     _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for ev in events {
            match ev.kind {
                PointerEventKind::Enter { .. } => {
                    if let Some(p) = &self.pointer { let _ = p.set_cursor(conn, CursorIcon::Default); }
                    self.mouse_x = ev.position.0; self.mouse_y = ev.position.1;
                    let _ = self.mouse_tx.try_send(MouseEvent::Motion { x: self.mouse_x, y: self.mouse_y });
                }
                PointerEventKind::Motion { .. } => {
                    self.mouse_x = ev.position.0; self.mouse_y = ev.position.1;
                    let _ = self.mouse_tx.try_send(MouseEvent::Motion { x: self.mouse_x, y: self.mouse_y });
                }
                PointerEventKind::Press { button, .. } => {
                    if button == 273 {
                        let _ = self.mouse_tx.try_send(MouseEvent::RightPress { x: self.mouse_x, y: self.mouse_y });
                    } else {
                        let _ = self.mouse_tx.try_send(MouseEvent::Press { x: self.mouse_x, y: self.mouse_y });
                    }
                }
                PointerEventKind::Release { .. } => {
                    let _ = self.mouse_tx.try_send(MouseEvent::Release { x: self.mouse_x, y: self.mouse_y });
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    let _ = self.mouse_tx.try_send(MouseEvent::Scroll { dx: horizontal.absolute, dy: vertical.absolute });
                }
                _ => {}
            }
        }
                     }
}

impl LayerShellHandler for WovenState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &SctLayerSurface) {}
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>,
                 _: &SctLayerSurface, cfg: LayerSurfaceConfigure, _serial: u32) {
        if cfg.new_size.0 > 0 { self.width  = cfg.new_size.0; }
        if cfg.new_size.1 > 0 { self.height = cfg.new_size.1; }
        self.configured = true;
        tracing::debug!("configure: {}x{}", self.width, self.height);
                 }
}

impl ShmHandler for WovenState { fn shm_state(&mut self) -> &mut Shm { &mut self.shm } }

// wl_region has no events; Dispatch impl required to satisfy wayland-client.
impl wayland_client::Dispatch<wayland_client::protocol::wl_region::WlRegion, ()> for WovenState {
    fn event(_: &mut Self, _: &wayland_client::protocol::wl_region::WlRegion,
             _: wl_region::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl ProvidesRegistryState for WovenState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_compositor!(WovenState);
smithay_client_toolkit::delegate_output!(WovenState);
smithay_client_toolkit::delegate_seat!(WovenState);
smithay_client_toolkit::delegate_keyboard!(WovenState);
smithay_client_toolkit::delegate_pointer!(WovenState);
smithay_client_toolkit::delegate_layer!(WovenState);
smithay_client_toolkit::delegate_shm!(WovenState);
smithay_client_toolkit::delegate_registry!(WovenState);
