//! woven-lite — lightweight standalone Wayland dashboard.
//!
//! Spawns a floating centered window (700×500), renders the dashboard,
//! and exits on Escape / click-to-focus-window.
//!
//! No daemon. No plugins. No config files.

mod screencopy_compat;
mod weather;
mod sysinfo;
mod theme;
mod dashboard;

use anyhow::{Context, Result};
use crossbeam_channel::unbounded;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use woven_render::{
    thumbnail::ThumbnailCapturer,
    surface::{MouseEvent, WovenSurface},
};
use woven_common::types::{EasingCurve, AnimationDef};
use crate::theme::BuiltinTheme;

use dashboard::{Dashboard, HitTarget};

// ── Window geometry ───────────────────────────────────────────────────────────

const LITE_W: u32 = 700;
const LITE_H: u32 = 500;

// ── Timing ────────────────────────────────────────────────────────────────────

const FRAME_MS:   u64 = 16;   // ~60fps while visible
const IDLE_MS:    u64 = 50;   // sleep when hidden / not animating
const SYS_TICK:   u64 = 2000; // refresh sysinfo every 2s

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("woven-lite starting");

    // Warn about screencopy backend limitation
    screencopy_compat::active_backend();

    run_event_loop()
}

fn run_event_loop() -> Result<()> {
    let (mouse_tx, mouse_rx) = unbounded::<MouseEvent>();

    // ── Surface ───────────────────────────────────────────────────────────────
    // woven-render's WovenSurface is a full-screen layer-shell overlay.
    // woven-lite reuses it — the dashboard paints a centered card over a
    // transparent background so the desktop shows through the edges.
    let mut surface = WovenSurface::new(mouse_tx)
        .context("failed to create Wayland surface")?;

    // ── Screencopy ────────────────────────────────────────────────────────────
    let mut thumbnailer = ThumbnailCapturer::new(); // None on Sway/Hyprland for now

    // ── Dashboard state ───────────────────────────────────────────────────────
    let mut dash = Dashboard::new();

    // Show surface immediately on startup
    surface.show().context("surface show failed")?;
    let mut visible = true;

    // ── Animation state ───────────────────────────────────────────────────────
    let open_anim  = AnimationDef { curve: EasingCurve::EaseOutCubic, duration_ms: 160 };
    let close_anim = AnimationDef { curve: EasingCurve::EaseOutCubic, duration_ms: 120 };

    let mut anim_t:      f32 = 0.0;
    let mut anim_target: f32 = 1.0;
    let mut anim_from:   f32 = 0.0;
    let mut anim_start        = Instant::now();

    // Kick off open animation
    anim_from = 0.0; anim_target = 1.0; anim_start = Instant::now();

    // ── Timing ────────────────────────────────────────────────────────────────
    let mut last_sys_tick = Instant::now();
    let mut pending_close = false;
    let mut pending_ws_focus: Option<u32>   = None;
    let mut pending_win_focus: Option<String> = None;

    // Kick initial window thumbnail capture
    if let Some(ref mut tc) = thumbnailer {
        tc.request_output(0);
    }

    loop {
        let loop_start = Instant::now();

        // ── Wayland dispatch ──────────────────────────────────────────────────
        surface.dispatch()?;

        // ── Input ─────────────────────────────────────────────────────────────
        while let Ok(ev) = mouse_rx.try_recv() {
            match ev {
                MouseEvent::Press { x, y } => {
                    if let Some(hit) = dash.hit_test(x, y) {
                        handle_hit(hit, &mut dash, &mut pending_close,
                                   &mut pending_ws_focus, &mut pending_win_focus);
                    }
                }
                MouseEvent::Key { keysym } => {
                    // Escape = exit
                    if keysym == 0xff1b {
                        pending_close = true;
                    }
                }
                _ => {}
            }
        }

        // ── Sys tick ──────────────────────────────────────────────────────────
        if last_sys_tick.elapsed() >= Duration::from_millis(SYS_TICK) {
            dash.tick_sys();
            last_sys_tick = Instant::now();
        }

        // ── Screencopy pump ───────────────────────────────────────────────────
        if let Some(ref mut tc) = thumbnailer {
            tc.pump();
            dash.update_thumbnails(tc.cache().clone());
        }

        // ── Close animation ───────────────────────────────────────────────────
        if pending_close && visible {
            pending_close = false;
            anim_from = anim_t; anim_target = 0.0; anim_start = Instant::now();
        }

        // ── Animation step ────────────────────────────────────────────────────
        {
            let elapsed_ms = anim_start.elapsed().as_secs_f32() * 1000.0;
            let (dur_ms, curve) = if anim_target >= anim_from {
                (open_anim.duration_ms as f32, &open_anim.curve)
            } else {
                (close_anim.duration_ms as f32, &close_anim.curve)
            };
            let raw   = if dur_ms > 0.0 { (elapsed_ms / dur_ms).clamp(0.0, 1.0) } else { 1.0 };
            let eased = apply_easing(curve, raw);
            anim_t = anim_from + (anim_target - anim_from) * eased;

            // Close animation finished → exit
            if anim_target == 0.0 && raw >= 1.0 && visible {
                info!("woven-lite: close animation done, exiting");
                // Perform any pending compositor actions before exit
                if let Some(_ws_id) = pending_ws_focus.take() {
                    // TODO: send workspace focus IPC once compositor backend wired
                }
                if let Some(_win_id) = pending_win_focus.take() {
                    // TODO: send window focus IPC once compositor backend wired
                }
                break;
            }
        }

        // ── Draw + present ────────────────────────────────────────────────────
        if visible || anim_t > 0.001 {
            let (w, h) = surface.size();
            let (pw, ph) = if w > 0 && h > 0 { (w, h) } else { (LITE_W, LITE_H) };
            let pixels = dash.paint(pw, ph, anim_t);
            if let Err(e) = surface.present(pixels, pw, ph) {
                error!("present failed: {e:#}");
            }
        }

        // ── Timing ────────────────────────────────────────────────────────────
        let animating = anim_t > 0.001 && anim_t < 0.999;
        let elapsed   = loop_start.elapsed();
        if visible || animating {
            let budget = Duration::from_millis(FRAME_MS);
            if elapsed < budget { std::thread::sleep(budget - elapsed); }
        } else {
            std::thread::sleep(Duration::from_millis(IDLE_MS));
        }
    }

    Ok(())
}

fn handle_hit(
    hit:               HitTarget,
    dash:              &mut Dashboard,
    pending_close:     &mut bool,
    pending_ws_focus:  &mut Option<u32>,
    pending_win_focus: &mut Option<String>,
) {
    match hit {
        HitTarget::GearButton => {
            dash.theme_picker.open = !dash.theme_picker.open;
        }
        HitTarget::ClosePanel => {
            dash.theme_picker.open = false;
        }
        HitTarget::ThemeOption(idx) => {
            if let Some(&t) = BuiltinTheme::ALL.get(idx) {
                dash.theme_picker.current = t;
            }
        }
        HitTarget::WorkspaceTab(ws_id) => {
            *pending_ws_focus = Some(ws_id);
            // Don't close — let the user see the workspace's windows in the grid.
            // (Actual compositor focus deferred to exit; for now just update active_ws.)
        }
        HitTarget::WindowCard { window_id } => {
            *pending_win_focus = Some(window_id);
            *pending_close = true; // click window → focus + exit
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
