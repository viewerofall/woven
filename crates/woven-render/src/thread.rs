//! Render thread.
//!
//! Hide/show = keyboard interactivity toggle. Surface never unmaps.
//! The close animation fades to transparent THEN hide() releases keyboard focus.
//! On re-show, show() re-grabs keyboard and the open animation plays.

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use tracing::{error, info, warn};
use woven_common::types::{AnimationConfig, BarConfig, BarPosition, EasingCurve, Theme, Workspace, WorkspaceMetrics};

use crate::bar_surface::{BarSurface, BAR_THICK, PANEL_THICK};
use crate::draw::Painter;
use crate::surface::{WovenSurface, MouseEvent};
use crate::thumbnail::ThumbnailCapturer;

#[derive(Debug, Clone)]
pub enum RenderCmd {
    Show,
    Hide,
    Toggle,
    UpdateTheme(Theme),
    UpdateSettings { show_empty: bool },
    UpdateState { workspaces: Vec<Workspace>, metrics: Vec<WorkspaceMetrics> },
    /// The compositor just made workspace `ws_id` active — capture the output
    /// and store it as that workspace's screenshot.
    CaptureForWorkspace(u32),
    /// Apply new bar configuration (creates or destroys the bar surface).
    UpdateBarConfig(BarConfig),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum WindowAction {
    Focus(String),
    Close(String),
    ToggleFloat(String),
    TogglePin(String),
    ToggleFullscreen(String),
    CloseOverlay,
    FocusWorkspace(u32),
    /// Open the full-screen preview panel for a workspace (handled internally by Painter).
    PreviewWorkspace(u32),
    /// Close the preview panel (handled internally by Painter).
    ClosePanel,
    /// Toggle the main overlay (from bar button — handled in render thread).
    ToggleOverlay,
    /// Hide the bar (from bar button — handled in render thread).
    HideBar,
    /// Expand the bar into full control-center mode (handled in render thread).
    ExpandPanel,
    /// Collapse the control center back to the narrow bar (handled in render thread).
    CollapsePanel,
    // ── Power ─────────────────────────────────────────────────────────────────
    PowerSuspend,
    PowerReboot,
    PowerShutdown,
    PowerLock,
    PowerLogout,
    // ── Media ─────────────────────────────────────────────────────────────────
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    // ── Quick tiles ───────────────────────────────────────────────────────────
    WifiToggle,
    BtToggle,
}

pub struct RenderThread {
    pub tx:           Sender<RenderCmd>,
    pub action_rx:    Receiver<WindowAction>,
    pub visible_flag: Arc<AtomicBool>,
}

impl RenderThread {
    pub fn spawn(theme: Theme, anims: AnimationConfig) -> Result<Self> {
        let (tx, rx)               = unbounded::<RenderCmd>();
        let (action_tx, action_rx) = unbounded::<WindowAction>();
        let visible_flag           = Arc::new(AtomicBool::new(false));
        let vf                     = visible_flag.clone();

        thread::Builder::new().name("woven-render".into()).spawn(move || {
            if let Err(e) = render_loop(rx, action_tx, theme, anims, vf) {
                error!("render thread crashed: {:#}", e);
            }
        })?;

        info!("render thread spawned");
        Ok(Self { tx, action_rx, visible_flag })
    }

    pub fn send(&self, cmd: RenderCmd) { let _ = self.tx.send(cmd); }
}

fn render_loop(
    rx:           Receiver<RenderCmd>,
    action_tx:    Sender<WindowAction>,
    theme:        Theme,
    anims:        AnimationConfig,
    visible_flag: Arc<AtomicBool>,
) -> Result<()> {
    info!("render loop starting");

    let (mouse_tx, mouse_rx)         = unbounded::<MouseEvent>();
    let (bar_mouse_tx, bar_mouse_rx) = unbounded::<MouseEvent>();

    let mut surface     = WovenSurface::new(mouse_tx)?;
    let mut painter     = Painter::new(theme, anims.clone(), action_tx.clone());
    let mut thumbnailer = ThumbnailCapturer::new();
    let mut visible      = false;
    let mut pending_hide = false;
    let mut pending_show = false;

    // Bar surface — created on demand by UpdateBarConfig from Lua.
    // Keep a clone of bar_mouse_tx so we can recreate the bar if config changes.
    let bar_mouse_tx_store = bar_mouse_tx.clone();
    let mut bar_surface: Option<BarSurface> = None;
    let mut bar_visible  = true;
    let mut bar_position = BarPosition::Right;

    let mut anim_t:     f32             = 0.0;
    let mut anim_target: f32            = 0.0;
    let mut anim_start  = std::time::Instant::now();
    let mut anim_from:  f32             = 0.0;
    let open_ms    = anims.overlay_open.duration_ms  as f32;
    let close_ms   = anims.overlay_close.duration_ms as f32;
    let open_curve  = anims.overlay_open.curve.clone();
    let close_curve = anims.overlay_close.curve.clone();

    loop {
        let loop_start = std::time::Instant::now();

        // ── commands ──────────────────────────────────────────────────────────
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                RenderCmd::Show => {
                    if !visible {
                        // Capture before anim starts so surface is still transparent.
                        if let Some(ref mut tc) = thumbnailer {
                            tc.request_output(0);
                            // Also tag the capture as the active workspace's screenshot.
                            if let Some(ws_id) = painter.active_workspace_id() {
                                tc.request_output_for_ws(ws_id, 0);
                            }
                            tc.request_windows(&painter.all_windows());
                        }
                        visible = true; pending_hide = false;
                        visible_flag.store(true, Ordering::Relaxed);
                        surface.show()?;
                        anim_from = anim_t; anim_target = 1.0; anim_start = std::time::Instant::now();
                    }
                }
                RenderCmd::Hide   => { pending_hide = true; }
                RenderCmd::Toggle => {
                    if visible {
                        pending_hide = true;
                    } else {
                        if let Some(ref mut tc) = thumbnailer {
                            tc.request_output(0);
                            if let Some(ws_id) = painter.active_workspace_id() {
                                tc.request_output_for_ws(ws_id, 0);
                            }
                            tc.request_windows(&painter.all_windows());
                        }
                        visible = true; pending_hide = false;
                        visible_flag.store(true, Ordering::Relaxed);
                        surface.show()?;
                        anim_from = anim_t; anim_target = 1.0; anim_start = std::time::Instant::now();
                    }
                }
                RenderCmd::CaptureForWorkspace(ws_id) => {
                    if let Some(ref mut tc) = thumbnailer {
                        tc.request_output_for_ws(ws_id, 0);
                    }
                }
                RenderCmd::UpdateBarConfig(cfg) => {
                    bar_position = cfg.position.clone();
                    bar_visible  = true;
                    if cfg.enabled {
                        match BarSurface::new(&cfg.position, bar_mouse_tx_store.clone()) {
                            Ok(mut b) => { let _ = b.dispatch(); bar_surface = Some(b); }
                            Err(e)    => warn!("bar surface: {e:#}"),
                        }
                    } else {
                        bar_surface = None;
                    }
                }
                RenderCmd::UpdateTheme(t) => painter.update_theme(t),
                RenderCmd::UpdateSettings { show_empty } => painter.update_settings(show_empty),
                RenderCmd::UpdateState { workspaces, metrics } => {
                    painter.update_state(workspaces, metrics);
                    if let Some(ref mut tc) = thumbnailer {
                        let ws = painter.all_windows();
                        if !ws.is_empty() { tc.request_windows(&ws); }
                    }
                }
                RenderCmd::Shutdown => { info!("render thread shutting down"); return Ok(()); }
            }
        }

        // ── bar dispatch ──────────────────────────────────────────────────────
        if let Some(ref mut bar) = bar_surface { let _ = bar.dispatch(); }

        // ── bar input ─────────────────────────────────────────────────────────
        while let Ok(ev) = bar_mouse_rx.try_recv() {
            match ev {
                MouseEvent::Press  { x, y } => {
                    let is_vert = matches!(bar_position, BarPosition::Left | BarPosition::Right);
                    match painter.on_bar_press(x, y) {
                        Some(WindowAction::ToggleOverlay) => {
                            if visible { pending_hide = true; } else { pending_show = true; }
                        }
                        Some(WindowAction::HideBar) => {
                            bar_visible = false;
                            if let Some(ref mut bar) = bar_surface {
                                let _ = bar.present_transparent();
                            }
                        }
                        Some(WindowAction::ExpandPanel) => {
                            painter.set_panel_expanded(true);
                            if let Some(ref mut bar) = bar_surface {
                                let _ = bar.resize(PANEL_THICK, is_vert);
                            }
                        }
                        Some(WindowAction::CollapsePanel) => {
                            painter.set_panel_expanded(false);
                            if let Some(ref mut bar) = bar_surface {
                                let _ = bar.resize(BAR_THICK, is_vert);
                            }
                        }
                        Some(WindowAction::PowerSuspend) => {
                            let _ = std::process::Command::new("systemctl").arg("suspend").spawn();
                        }
                        Some(WindowAction::PowerReboot) => {
                            let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
                        }
                        Some(WindowAction::PowerShutdown) => {
                            let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
                        }
                        Some(WindowAction::PowerLock) => {
                            let _ = std::process::Command::new("loginctl").arg("lock-session").spawn();
                        }
                        Some(WindowAction::PowerLogout) => {
                            let _ = std::process::Command::new("loginctl")
                                .args(["kill-user", ""])
                                .spawn();
                        }
                        Some(WindowAction::MediaPlayPause) => {
                            let _ = std::process::Command::new("playerctl").arg("play-pause").spawn();
                        }
                        Some(WindowAction::MediaNext) => {
                            let _ = std::process::Command::new("playerctl").arg("next").spawn();
                        }
                        Some(WindowAction::MediaPrev) => {
                            let _ = std::process::Command::new("playerctl").arg("previous").spawn();
                        }
                        Some(WindowAction::WifiToggle) => {
                            let _ = std::process::Command::new("nmcli")
                                .args(["radio", "wifi", "toggle"])
                                .spawn();
                        }
                        Some(WindowAction::BtToggle) => {
                            let _ = std::process::Command::new("sh")
                                .args(["-c",
                                    "bluetoothctl show | grep -q 'Powered: yes' \
                                     && bluetoothctl power off \
                                     || bluetoothctl power on"])
                                .spawn();
                        }
                        Some(action) => { let _ = action_tx.try_send(action); }
                        None => {}
                    }
                }
                MouseEvent::Motion { x, y } => painter.on_bar_motion(x, y),
                _ => {}
            }
        }

        // ── dispatch ──────────────────────────────────────────────────────────
        surface.dispatch()?;

        // ── input ─────────────────────────────────────────────────────────────
        while let Ok(ev) = mouse_rx.try_recv() {
            match ev {
                MouseEvent::Press { x, y } => {
                    if painter.on_press(x, y) && visible { pending_hide = true; }
                }
                MouseEvent::RightPress { .. } => { if visible { pending_hide = true; } }
                MouseEvent::Release { x, y } => painter.on_release(x, y),
                MouseEvent::Motion  { x, y } => painter.on_motion(x, y),
                MouseEvent::Scroll  { dy, .. } => {
                    if dy > 0.5 { painter.next_page(); }
                    else if dy < -0.5 { painter.prev_page(); }
                }
                MouseEvent::Key { .. } => { if visible { pending_hide = true; } }
            }
        }

        // ── pending show (from bar toggle button) ─────────────────────────────
        if pending_show && !visible {
            pending_show = false;
            if let Some(ref mut tc) = thumbnailer {
                tc.request_output(0);
                if let Some(ws_id) = painter.active_workspace_id() {
                    tc.request_output_for_ws(ws_id, 0);
                }
                tc.request_windows(&painter.all_windows());
            }
            visible = true;
            visible_flag.store(true, Ordering::Relaxed);
            surface.show()?;
            anim_from = anim_t; anim_target = 1.0; anim_start = std::time::Instant::now();
        } else {
            pending_show = false;
        }

        // ── start close animation ─────────────────────────────────────────────
        if pending_hide {
            pending_hide = false;
            anim_from = anim_t; anim_target = 0.0; anim_start = std::time::Instant::now();
            // Don't call hide() yet — wait until animation completes so the
            // close animation renders before we release keyboard focus.
        }

        // ── animation ─────────────────────────────────────────────────────────
        {
            let elapsed_ms = anim_start.elapsed().as_secs_f32() * 1000.0;
            let (dur_ms, curve) = if anim_target > anim_from {
                (open_ms, &open_curve)
            } else {
                (close_ms, &close_curve)
            };
            let raw   = if dur_ms > 0.0 { (elapsed_ms / dur_ms).clamp(0.0, 1.0) } else { 1.0 };
            let eased = apply_easing(curve, raw);
            anim_t = anim_from + (anim_target - anim_from) * eased;

            // Close animation finished → release keyboard focus.
            // Frame is already transparent (anim_t=0). Safe to release now.
            if anim_target == 0.0 && raw >= 1.0 && visible {
                anim_t = 0.0;
                visible = false;
                visible_flag.store(false, Ordering::Relaxed);
                surface.hide()?; // keyboard=None, buffer stays attached (transparent)
            }
        }

        // ── screencopy ────────────────────────────────────────────────────────
        // Pump even when not visible so workspace-transition captures complete.
        if let Some(ref mut tc) = thumbnailer {
            tc.pump();
            if visible {
                painter.update_thumbnails(tc.cache().clone());
                painter.update_output_thumbnail(tc.output_cache().get(&0).cloned());
                painter.update_workspace_cache(tc.workspace_cache().clone());
            }
        }

        // ── bar render ────────────────────────────────────────────────────────
        if let Some(ref mut bar) = bar_surface {
            if bar_visible {
                if let Err(e) = bar.present_for_each(|bw, bh| {
                    painter.paint_bar(bw, bh, &bar_position)
                }) {
                    warn!("bar present: {e:#}");
                }
            }
        }

        // ── draw + present ────────────────────────────────────────────────────
        // Always render while visible OR animating (close anim needs to play out).
        if visible || anim_t > 0.001 {
            let (w, h) = surface.size();
            if w > 0 && h > 0 {
                let result = panic::catch_unwind(AssertUnwindSafe(|| painter.paint(w, h, anim_t)));
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
                        warn!("paint panicked: {}", msg);
                    }
                }
            }
        }

        // ── timing ────────────────────────────────────────────────────────────
        let animating = anim_t > 0.001 && anim_t < 0.999;
        let elapsed   = loop_start.elapsed();
        let budget    = std::time::Duration::from_millis(16);
        if visible || animating {
            if elapsed < budget { std::thread::sleep(budget - elapsed); }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

fn apply_easing(curve: &EasingCurve, t: f32) -> f32 {
    match curve {
        EasingCurve::Linear         => t,
        EasingCurve::EaseOutCubic   => 1.0 - (1.0 - t).powi(3),
        EasingCurve::EaseInCubic    => t * t * t,
        EasingCurve::EaseInOutCubic => {
            if t < 0.5 { 4.0 * t * t * t }
            else       { 1.0 - (-2.0 * t + 2.0_f32).powi(3) / 2.0 }
        }
        EasingCurve::Spring { tension } => {
            let w = tension.sqrt().max(1.0);
            1.0 - (-6.0 * t).exp() * (w * t * std::f32::consts::TAU).cos()
        }
    }
}
