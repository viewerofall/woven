//! Wayland layer-shell surface management for woven-bar.
//! Lifted from woven/crates/woven-render/src/bar_surface.rs and adapted
//! to be a standalone crate with no woven-common dependency.

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
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
    protocol::{wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
    Connection, EventQueue, Proxy, QueueHandle,
};
use crossbeam_channel::Sender;

use crate::config::BarPosition;

#[derive(Debug, Clone)]
pub enum MouseEvent {
    Motion { x: f64, y: f64 },
    Press  { x: f64, y: f64 },
}

struct PerOutputBar {
    output_id:     u32,
    layer_surface: SctLayerSurface,
    pool:          SlotPool,
    width:         u32,
    height:        u32,
    configured:    bool,
}

pub struct BarSurface {
    queue: EventQueue<BarState>,
    state: BarState,
}

impl BarSurface {
    pub fn new(position: &BarPosition, height: u32, mouse_tx: Sender<MouseEvent>) -> Result<Self> {
        let conn = Connection::connect_to_env()
            .context("bar: failed to connect to Wayland display")?;
        let (globals, queue) = registry_queue_init::<BarState>(&conn)
            .context("bar: failed to init Wayland registry")?;
        let qh = queue.handle();

        let compositor  = CompositorState::bind(&globals, &qh).context("bar: wl_compositor missing")?;
        let layer_shell = LayerShell::bind(&globals, &qh).context("bar: wlr-layer-shell missing")?;
        let shm         = Shm::bind(&globals, &qh).context("bar: wl_shm missing")?;
        let seat_state  = SeatState::new(&globals, &qh);

        let state = BarState {
            registry:     RegistryState::new(&globals),
            compositor,
            output_state: OutputState::new(&globals, &qh),
            seat_state,
            shm,
            layer_shell,
            position: position.clone(),
            height,
            bars: Vec::new(),
            pointer: None,
            mouse_tx,
            mouse_x: 0.0,
            mouse_y: 0.0,
        };

        let mut s = Self { queue, state };
        let _ = s.queue.roundtrip(&mut s.state);
        Ok(s)
    }

    pub fn output_count(&self) -> usize {
        self.state.bars.iter().filter(|b| b.configured).count()
    }

    pub fn size(&self) -> (u32, u32) {
        self.state.bars.iter()
            .find(|b| b.configured && b.width > 0 && b.height > 0)
            .map(|b| (b.width, b.height))
            .unwrap_or((0, 0))
    }

    pub fn dispatch(&mut self) -> Result<()> {
        if let Err(e) = self.queue.flush() {
            tracing::debug!("bar flush: {e}");
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
        self.queue.dispatch_pending(&mut self.state).context("bar dispatch failed")?;
        if self.state.bars.iter().any(|b| !b.configured) {
            let _ = self.queue.roundtrip(&mut self.state);
        }
        Ok(())
    }

    /// Call `paint_fn(width, height)` for each configured output and commit the
    /// returned ARGB8888 pixel buffer to that output's layer surface.
    pub fn present_for_each<F: FnMut(u32, u32) -> Vec<u8>>(
        &mut self,
        mut paint_fn: F,
    ) -> Result<()> {
        for bar in &mut self.state.bars {
            if bar.width == 0 || bar.height == 0 { continue; }
            let pixels = paint_fn(bar.width, bar.height);
            let stride = bar.width * 4;
            let (buffer, canvas) = bar.pool
                .create_buffer(
                    bar.width as i32, bar.height as i32,
                    stride as i32, wl_shm::Format::Argb8888,
                )
                .context("bar: create_buffer failed")?;
            let n: usize = canvas.len().min(pixels.len());
            canvas[..n].copy_from_slice(&pixels[..n]);
            let surf = bar.layer_surface.wl_surface();
            buffer.attach_to(surf).ok();
            surf.damage_buffer(0, 0, bar.width as i32, bar.height as i32);
            bar.layer_surface.commit();
        }
        Ok(())
    }
}

fn position_anchor(pos: &BarPosition) -> Anchor {
    match pos {
        BarPosition::Top    => Anchor::TOP    | Anchor::LEFT | Anchor::RIGHT,
        BarPosition::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        BarPosition::Left   => Anchor::LEFT   | Anchor::TOP  | Anchor::BOTTOM,
        BarPosition::Right  => Anchor::RIGHT  | Anchor::TOP  | Anchor::BOTTOM,
    }
}

// ─────────────────────────────────────────────────────────────────────────────

struct BarState {
    registry:     RegistryState,
    compositor:   CompositorState,
    output_state: OutputState,
    seat_state:   SeatState,
    shm:          Shm,
    layer_shell:  LayerShell,
    position:     BarPosition,
    height:       u32,
    bars:         Vec<PerOutputBar>,
    pointer:      Option<ThemedPointer>,
    mouse_tx:     Sender<MouseEvent>,
    mouse_x:      f64,
    mouse_y:      f64,
}

impl BarState {
    fn add_output(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        let output_id = output.id().protocol_id();
        if self.bars.iter().any(|b| b.output_id == output_id) { return; }

        let surface       = self.compositor.create_surface(qh);
        let layer_surface = self.layer_shell.create_layer_surface(
            qh, surface, Layer::Top,
            Some("woven-bar"), Some(output),
        );

        let anchor = position_anchor(&self.position);
        layer_surface.set_anchor(anchor);
        layer_surface.set_exclusive_zone(self.height as i32);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        let (sz_w, sz_h) = if self.position.is_vertical() {
            (self.height, 0)
        } else {
            (0, self.height)
        };
        layer_surface.set_size(sz_w, sz_h);
        layer_surface.commit();

        let pool = match SlotPool::new(8 * 1024 * 1024, &self.shm) {
            Ok(p)  => p,
            Err(e) => { tracing::warn!("bar: shm pool failed for output {output_id}: {e}"); return; }
        };

        tracing::info!("bar: added surface for output {output_id}");
        self.bars.push(PerOutputBar {
            output_id,
            layer_surface,
            pool,
            width:      0,
            height:     0,
            configured: false,
        });
    }
}

// ─── Smithay handler impls ────────────────────────────────────────────────────

impl CompositorHandler for BarState {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for BarState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_output(qh, &output);
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let id = output.id().protocol_id();
        self.bars.retain(|b| b.output_id != id);
    }
}

impl SeatHandler for BarState {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat_state }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>,
                      seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            let cs = self.compositor.create_surface(qh);
            match self.seat_state.get_pointer_with_theme(qh, &seat, self.shm.wl_shm(), cs, ThemeSpec::System) {
                Ok(p)  => { self.pointer = Some(p); }
                Err(e) => tracing::warn!("bar pointer: {e}"),
            }
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>,
                         _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer { self.pointer = None; }
    }
}

impl PointerHandler for BarState {
    fn pointer_frame(&mut self, conn: &Connection, _: &QueueHandle<Self>,
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
                    if button == 272 { // left click
                        let _ = self.mouse_tx.try_send(MouseEvent::Press { x: self.mouse_x, y: self.mouse_y });
                    }
                }
                _ => {}
            }
        }
    }
}

impl LayerShellHandler for BarState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &SctLayerSurface) {}

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>,
                 layer_surface: &SctLayerSurface, cfg: LayerSurfaceConfigure, _: u32) {
        for bar in &mut self.bars {
            if bar.layer_surface.wl_surface() == layer_surface.wl_surface() {
                if cfg.new_size.0 > 0 { bar.width  = cfg.new_size.0; }
                if cfg.new_size.1 > 0 { bar.height = cfg.new_size.1; }
                bar.configured = true;
                tracing::debug!("bar configure [output {}]: {}×{}", bar.output_id, bar.width, bar.height);
                break;
            }
        }
    }
}

impl ShmHandler for BarState { fn shm_state(&mut self) -> &mut Shm { &mut self.shm } }

impl wayland_client::Dispatch<wl_region::WlRegion, ()> for BarState {
    fn event(_: &mut Self, _: &wl_region::WlRegion, _: wl_region::Event,
             _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl ProvidesRegistryState for BarState {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_compositor!(BarState);
smithay_client_toolkit::delegate_output!(BarState);
smithay_client_toolkit::delegate_seat!(BarState);
smithay_client_toolkit::delegate_pointer!(BarState);
smithay_client_toolkit::delegate_layer!(BarState);
smithay_client_toolkit::delegate_shm!(BarState);
smithay_client_toolkit::delegate_registry!(BarState);
