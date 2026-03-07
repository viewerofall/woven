//! Render thread: owns Wayland connection, painter, and camera.
//! Mouse events from the surface are forwarded here to drive panning/zooming.

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::panic::{self, AssertUnwindSafe};
use std::thread;
use tracing::{error, info, warn};
use woven_common::types::{AnimationConfig, Theme, Workspace, WorkspaceMetrics};

use crate::draw::Painter;
use crate::surface::{LayerSurface, MouseEvent};

#[derive(Debug, Clone)]
pub enum RenderCmd {
    Show,
    Hide,
    Toggle,
    UpdateTheme(Theme),
    UpdateState {
        workspaces: Vec<Workspace>,
        metrics:    Vec<WorkspaceMetrics>,
    },
    Shutdown,
}

/// Actions the overlay UI triggers on a window — sent back to main thread
/// via the action_tx channel so the compositor can be dispatched.
#[derive(Debug, Clone)]
pub enum WindowAction {
    Focus(String),
    Close(String),
    ToggleFloat(String),
    TogglePin(String),
    ToggleFullscreen(String),
}

pub struct RenderThread {
    pub tx:        Sender<RenderCmd>,
    pub action_rx: crossbeam_channel::Receiver<WindowAction>,
}

impl RenderThread {
    pub fn spawn(theme: Theme, anims: AnimationConfig) -> Result<Self> {
        let (tx, rx)              = unbounded::<RenderCmd>();
        let (action_tx, action_rx) = unbounded::<WindowAction>();

        thread::Builder::new()
        .name("woven-render".into())
        .spawn(move || {
            if let Err(e) = render_loop(rx, action_tx, theme, anims) {
                error!("render thread crashed: {:#}", e);
            }
        })?;

        info!("render thread spawned");
        Ok(Self { tx, action_rx })
    }

    pub fn send(&self, cmd: RenderCmd) {
        let _ = self.tx.send(cmd);
    }
}

fn render_loop(rx: Receiver<RenderCmd>, action_tx: Sender<WindowAction>, theme: Theme, anims: AnimationConfig) -> Result<()> {
    info!("render loop starting");

    // channel for mouse events: surface → painter
    let (mouse_tx, mouse_rx) = unbounded::<MouseEvent>();

    let mut surface = LayerSurface::new(mouse_tx)?;
    let mut painter = Painter::new(theme, anims, action_tx);
    let mut visible = false;
    let mut pending_hide = false;

    loop {
        // ── process render commands ──────────────────────────────────────────
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                RenderCmd::Show   => { visible = true;  pending_hide = false; surface.show()?; }
                RenderCmd::Hide   => { pending_hide = true; }
                RenderCmd::Toggle => {
                    if visible { pending_hide = true; }
                    else       { visible = true; pending_hide = false; surface.show()?; }
                }
                RenderCmd::UpdateTheme(t) => painter.update_theme(t),
                RenderCmd::UpdateState { workspaces, metrics } => {
                    painter.update_state(workspaces, metrics);
                }
                RenderCmd::Shutdown => {
                    info!("render thread shutting down");
                    return Ok(());
                }
            }
        }

        // ── dispatch Wayland events first to fill the channel ────────────────
        surface.dispatch()?;

        // ── process input events ─────────────────────────────────────────────
        while let Ok(ev) = mouse_rx.try_recv() {
            match ev {
                MouseEvent::Press      { x, y } => { painter.on_press(x, y); }
                MouseEvent::RightPress { .. }   => { if visible { pending_hide = true; } }
                MouseEvent::Release    { x, y } => painter.on_release(x, y),
                MouseEvent::Motion     { x, y } => painter.on_motion(x, y),
                MouseEvent::Scroll     { dx: _, dy } => {
                    if dy > 0.5 { painter.next_page(); }
                    else if dy < -0.5 { painter.prev_page(); }
                }
                MouseEvent::Key { .. } => { if visible { pending_hide = true; } }
            }
        }

        // ── apply deferred hide (safe now — outside dispatch) ────────────────
        if pending_hide {
            visible      = false;
            pending_hide = false;
            surface.hide()?;
        }

        // ── draw frame if visible ────────────────────────────────────────────
        if visible {
            let (w, h) = surface.size();
            if w > 0 && h > 0 {
                // catch_unwind protects against tiny-skia internal panics
                // (e.g. degenerate path geometry from certain input events)
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    painter.paint(w, h)
                }));
                match result {
                    Ok(pixels) => {
                        if let Err(e) = surface.present(pixels, w, h) {
                            warn!("present failed: {:#}", e);
                        }
                    }
                    Err(cause) => {
                        let msg = cause.downcast_ref::<&str>().copied()
                        .or_else(|| cause.downcast_ref::<String>().map(|s| s.as_str()))
                        .unwrap_or("unknown panic");
                        warn!("paint panicked (skipping frame): {}", msg);
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(16)); // ~60fps
    }
}
