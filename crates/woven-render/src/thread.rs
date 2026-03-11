//! Render thread: owns Wayland connection, painter, and camera.
//! Mouse events from the surface are forwarded here to drive panning/zooming.

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::panic::{self, AssertUnwindSafe};
use std::thread;
use tracing::{error, info, warn};
use woven_common::types::{AnimationConfig, EasingCurve, Theme, Workspace, WorkspaceMetrics};

use crate::draw::Painter;
use crate::surface::{LayerSurface, MouseEvent};
use crate::thumbnail::ThumbnailCapturer;

#[derive(Debug, Clone)]
pub enum RenderCmd {
    Show,
    Hide,
    Toggle,
    UpdateTheme(Theme),
    UpdateSettings { show_empty: bool },
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
    CloseOverlay,
}

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

pub struct RenderThread {
    pub tx:           Sender<RenderCmd>,
    pub action_rx:    crossbeam_channel::Receiver<WindowAction>,
    pub visible_flag: Arc<AtomicBool>,
}

impl RenderThread {
    pub fn spawn(theme: Theme, anims: AnimationConfig) -> Result<Self> {
        let (tx, rx)               = unbounded::<RenderCmd>();
        let (action_tx, action_rx) = unbounded::<WindowAction>();
        let visible_flag           = Arc::new(AtomicBool::new(false));
        let vf_loop                = visible_flag.clone();

        thread::Builder::new()
        .name("woven-render".into())
        .spawn(move || {
            if let Err(e) = render_loop(rx, action_tx, theme, anims, vf_loop) {
                error!("render thread crashed: {:#}", e);
            }
        })?;

        info!("render thread spawned");
        Ok(Self { tx, action_rx, visible_flag })
    }

    pub fn send(&self, cmd: RenderCmd) {
        let _ = self.tx.send(cmd);
    }
}

fn render_loop(rx: Receiver<RenderCmd>, action_tx: Sender<WindowAction>, theme: Theme, anims: AnimationConfig, visible_flag: Arc<AtomicBool>) -> Result<()> {
    info!("render loop starting");

    let (mouse_tx, mouse_rx) = unbounded::<MouseEvent>();

    let mut surface = LayerSurface::new(mouse_tx)?;
    let mut painter = Painter::new(theme, anims.clone(), action_tx);
    let mut thumbnailer = ThumbnailCapturer::new(); // None on non-Hyprland
    let mut visible      = false;
    let mut pending_hide = false;

    // ── animation state ───────────────────────────────────────────────────────
    // anim_t:   0.0 = fully hidden, 1.0 = fully visible
    // animating: true while a transition is in progress
    let mut anim_t:      f32              = 0.0;
    let mut anim_target: f32              = 0.0;
    let mut anim_start:  std::time::Instant = std::time::Instant::now();
    let mut anim_from:   f32              = 0.0;
    let open_ms  = anims.overlay_open.duration_ms  as f32;
    let close_ms = anims.overlay_close.duration_ms as f32;
    let open_curve  = anims.overlay_open.curve.clone();
    let close_curve = anims.overlay_close.curve.clone();

    loop {
        let loop_start = std::time::Instant::now();
        // ── process render commands ──────────────────────────────────────────
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                RenderCmd::Show   => {
                    if !visible {
                        visible = true; pending_hide = false;
                        visible_flag.store(true, Ordering::Relaxed);
                        surface.show()?;
                        surface.grab_input();
                        anim_from = anim_t; anim_target = 1.0; anim_start = std::time::Instant::now();
                        // kick off thumbnail captures for current windows
                        if let Some(ref mut tc) = thumbnailer {
                            let handles = painter.window_handles();
                            tc.request_all(&handles.iter().map(|(a,h)| (a.as_str(),*h)).collect::<Vec<_>>());
                            let cache = tc.pump_and_collect().clone();
                            painter.update_thumbnails(cache);
                        }
                    }
                }
                RenderCmd::Hide   => { pending_hide = true; }
                RenderCmd::Toggle => {
                    if visible {
                        pending_hide = true;
                    } else {
                        visible = true; pending_hide = false;
                        visible_flag.store(true, Ordering::Relaxed);
                        surface.show()?;
                        surface.grab_input();
                        anim_from = anim_t; anim_target = 1.0; anim_start = std::time::Instant::now();
                    }
                }
                RenderCmd::UpdateTheme(t)    => painter.update_theme(t),
                RenderCmd::UpdateSettings { show_empty } => painter.update_settings(show_empty),
                RenderCmd::UpdateState { workspaces, metrics } => {
                    painter.update_state(workspaces, metrics);
                    if visible {
                        if let Some(ref mut tc) = thumbnailer {
                            let handles = painter.window_handles();
                            tc.request_all(&handles.iter().map(|(a,h)| (a.as_str(),*h)).collect::<Vec<_>>());
                            let cache = tc.pump_and_collect().clone();
                            painter.update_thumbnails(cache);
                        }
                    }
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
                MouseEvent::Press { x, y } => {
                    if painter.on_press(x, y) && visible {
                        pending_hide = true;
                    }
                }
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

        // ── pending_hide kicks off close animation ────────────────────────────
        if pending_hide {
            pending_hide = false;
            // Release keyboard/pointer immediately so the focused window gets
            // input right away — don't wait for the animation to finish.
            surface.release_input();
            anim_from = anim_t; anim_target = 0.0; anim_start = std::time::Instant::now();
        }

        // ── advance animation ─────────────────────────────────────────────────
        {
            let elapsed_ms = anim_start.elapsed().as_secs_f32() * 1000.0;
            let (dur_ms, curve) = if anim_target > anim_from {
                (open_ms,  &open_curve)
            } else {
                (close_ms, &close_curve)
            };
            let raw = if dur_ms > 0.0 { (elapsed_ms / dur_ms).clamp(0.0, 1.0) } else { 1.0 };
            let eased = apply_easing(curve, raw);
            anim_t = anim_from + (anim_target - anim_from) * eased;

            // close animation finished → actually hide
            if anim_target == 0.0 && raw >= 1.0 {
                anim_t = 0.0;
                visible = false;
                visible_flag.store(false, Ordering::Relaxed);
                surface.hide()?;
            }
        }

        // ── draw frame if visible or animating ───────────────────────────────
        if visible || anim_t > 0.001 {
            let (w, h) = surface.size();
            if w > 0 && h > 0 {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    painter.paint(w, h, anim_t)
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

        let animating = anim_t > 0.001 && anim_t < 0.999;
        let elapsed = loop_start.elapsed();
        let budget  = std::time::Duration::from_millis(16);
        if visible || animating {
            if elapsed < budget { std::thread::sleep(budget - elapsed); }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

fn apply_easing(curve: &EasingCurve, t: f32) -> f32 {
    match curve {
        EasingCurve::Linear           => t,
        EasingCurve::EaseOutCubic     => 1.0 - (1.0 - t).powi(3),
        EasingCurve::EaseInCubic      => t * t * t,
        EasingCurve::EaseInOutCubic   => {
            if t < 0.5 { 4.0 * t * t * t }
            else       { 1.0 - (-2.0 * t + 2.0_f32).powi(3) / 2.0 }
        }
        EasingCurve::Spring { tension } => {
            // Simple damped spring approximation
            let w = tension.sqrt().max(1.0);
            1.0 - (-6.0 * t).exp() * (w * t * std::f32::consts::TAU).cos()
        }
    }
}
