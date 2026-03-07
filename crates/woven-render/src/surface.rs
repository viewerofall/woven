//! wlr-layer-shell surface + seat/pointer.
//! Cursor fix: set_cursor must be called inside pointer_frame (enter event),
//! not at capability time — that's when the compositor actually hands us the serial.

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
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, EventQueue, QueueHandle,
};
use crossbeam_channel::Sender;
use tracing::info;

/// Input events forwarded to the render thread
#[derive(Debug, Clone)]
pub enum MouseEvent {
    Motion      { x: f64, y: f64 },
    Scroll      { dx: f64, dy: f64 },
    Press       { x: f64, y: f64 },
    RightPress  { x: f64, y: f64 },
    Release     { x: f64, y: f64 },
    Key         { keysym: u32 },
}

pub struct LayerSurface {
    queue: EventQueue<WovenState>,
    state: WovenState,
}

impl LayerSurface {
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
        layer_surface.set_anchor(Anchor::all());
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer_surface.set_size(0, 0);
        layer_surface.commit();

        let pool = SlotPool::new(8 * 1024 * 1024, &shm).context("shm pool failed")?;

        let state = WovenState {
            registry:     RegistryState::new(&globals),
            compositor,
            output_state: OutputState::new(&globals, &qh),
            seat_state,
            shm,
            layer_surface,
            pool,
            pointer:      None,
            keyboard:     None,
            mouse_tx,
            mouse_x:      0.0,
            mouse_y:      0.0,
            width:        0,
            height:       0,
            configured:   false,
        };

        Ok(Self { queue, state })
    }

    pub fn show(&mut self) -> Result<()> {
        // Re-request configuration from compositor so surface gets a real size.
        // Don't commit here — present() will do that with a real buffer.
        self.state.configured = false;
        self.state.layer_surface.set_size(0, 0);
        self.state.layer_surface.set_keyboard_interactivity(
            smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity::OnDemand
        );
        self.state.layer_surface.wl_surface().commit();
        Ok(())
    }

    pub fn hide(&mut self) -> Result<()> {
        // Detach buffer — this makes the surface invisible without destroying it.
        self.state.configured = false;
        self.state.layer_surface.wl_surface().attach(None, 0, 0);
        self.state.layer_surface.wl_surface().commit();
        Ok(())
    }

    pub fn size(&self) -> (u32, u32) {
        (self.state.width, self.state.height)
    }

    pub fn dispatch(&mut self) -> Result<()> {
        if let Err(e) = self.queue.flush() {
            tracing::debug!("wayland flush skipped: {}", e);
        }
        if let Some(guard) = self.queue.prepare_read() {
            let _ = guard.read();
        }
        self.queue.dispatch_pending(&mut self.state).context("dispatch failed")?;
        Ok(())
    }

    pub fn present(&mut self, pixels: Vec<u8>, width: u32, height: u32) -> Result<()> {
        if !self.state.configured { return Ok(()); }
        let stride = width * 4;
        let (buffer, canvas) = self.state.pool
        .create_buffer(width as i32, height as i32, stride as i32, wl_shm::Format::Argb8888)
        .context("create_buffer failed")?;
        let n = canvas.len().min(pixels.len());
        canvas[..n].copy_from_slice(&pixels[..n]);
        let surf = self.state.layer_surface.wl_surface();
        buffer.attach_to(surf).context("attach failed")?;
        surf.damage_buffer(0, 0, width as i32, height as i32);
        surf.commit();
        Ok(())
    }
}

// ── Wayland state ─────────────────────────────────────────────────────────────

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
                      seat: wl_seat::WlSeat, cap: Capability)
    {
        if cap == Capability::Pointer && self.pointer.is_none() {
            let cursor_surf = self.compositor.create_surface(qh);
            match self.seat_state.get_pointer_with_theme(
                qh, &seat, self.shm.wl_shm(), cursor_surf, ThemeSpec::System,
            ) {
                Ok(ptr) => {
                    self.pointer = Some(ptr);
                    info!("seat: pointer ready");
                }
                Err(e) => tracing::warn!("pointer init failed: {}", e),
            }
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(kb) => {
                    self.keyboard = Some(kb);
                    info!("seat: keyboard ready");
                }
                Err(e) => tracing::warn!("keyboard init failed: {}", e),
            }
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>,
                         _: wl_seat::WlSeat, cap: Capability)
    {
        if cap == Capability::Pointer  { self.pointer  = None; }
        if cap == Capability::Keyboard { self.keyboard = None; }
    }
}

impl KeyboardHandler for WovenState {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>,
             _: &wl_keyboard::WlKeyboard,
             _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}

             fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>,
                      _: &wl_keyboard::WlKeyboard,
                      _: &wl_surface::WlSurface, _: u32) {}

                      fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                   _: &wl_keyboard::WlKeyboard,
                                   _: u32, event: KeyEvent)
                      {
                          let _ = self.mouse_tx.try_send(MouseEvent::Key { keysym: event.keysym.raw() });
                      }

                      fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                     _: &wl_keyboard::WlKeyboard,
                                     _: u32, _: KeyEvent) {}

                                     fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                         _: &wl_keyboard::WlKeyboard,
                                                         _: u32, _: Modifiers, _: u32) {}

                                                         fn update_repeat_info(&mut self, _: &Connection, _: &QueueHandle<Self>,
                                                                               _: &wl_keyboard::WlKeyboard, _: RepeatInfo) {}
}

impl PointerHandler for WovenState {
    fn pointer_frame(&mut self, conn: &Connection, _qh: &QueueHandle<Self>,
                     _: &wl_pointer::WlPointer, events: &[PointerEvent])
    {
        for ev in events {
            match ev.kind {
                // Enter event: THIS is the right time to set the cursor
                // The compositor gives us a valid serial here
                PointerEventKind::Enter { .. } => {
                    if let Some(ptr) = &self.pointer {
                        let _ = ptr.set_cursor(conn, CursorIcon::Default);
                    }
                }
                PointerEventKind::Motion { time: _ } => {
                    self.mouse_x = ev.position.0;
                    self.mouse_y = ev.position.1;
                    let _ = self.mouse_tx.try_send(MouseEvent::Motion {
                        x: self.mouse_x, y: self.mouse_y,
                    });
                }
                PointerEventKind::Press { button, .. } => {
                    if button == 273 {
                        let _ = self.mouse_tx.try_send(MouseEvent::RightPress {
                            x: self.mouse_x, y: self.mouse_y,
                        });
                    } else {
                        let _ = self.mouse_tx.try_send(MouseEvent::Press {
                            x: self.mouse_x, y: self.mouse_y,
                        });
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    let _ = self.mouse_tx.try_send(MouseEvent::Release {
                        x: self.mouse_x, y: self.mouse_y,
                    });
                    let _ = button;
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    let _ = self.mouse_tx.try_send(MouseEvent::Scroll {
                        dx: horizontal.absolute,
                        dy: vertical.absolute,
                    });
                }
                _ => {}
            }
        }
    }
}

impl LayerShellHandler for WovenState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &SctLayerSurface) {}
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>,
                 _: &SctLayerSurface, cfg: LayerSurfaceConfigure, _: u32)
    {
        self.width      = cfg.new_size.0;
        self.height     = cfg.new_size.1;
        self.configured = true;
        info!("surface configured: {}x{}", self.width, self.height);
    }
}

impl ShmHandler for WovenState {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
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
