//! Main bar render loop.
//! Layout: [left widgets] | [center widget(s) — self-position] | [right widgets]
//! Polls Sway state and redraws on change or every 1s for time/sys updates.

use anyhow::Result;
use crossbeam_channel::unbounded;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tiny_skia::{Color, Pixmap};
use tokio::runtime::Runtime;

use crate::config::{BarConfig, BarPosition, BarStyle, ModuleKind, ThemeSource};
use crate::draw::{clear, fill_rect, fill_rounded_rect, hex_color};
use crate::icons::IconCache;
use crate::sway::{BarState as SwayState, SwayClient};
use crate::text::TextRenderer;
use crate::wayland::{BarSurface, MouseEvent};
use crate::widgets::{
    RenderCtx, Widget,
    activities::ActivitiesWidget,
    audio::AudioWidget,
    battery::BatteryWidget,
    clock::ClockWidget,
    date::DateWidget,
    cava::CavaWidget,
    control_center::ControlCenterWidget,
    cpu::CpuWidget,
    disk::DiskWidget,
    media::MediaWidget,
    memory::MemoryWidget,
    network::NetworkWidget,
    notifications::NotificationsWidget,
    systray::SystrayWidget,
    temp::TempWidget,
    window_title::WindowTitleWidget,
    weather::WeatherWidget,
    workspaces::WorkspacesWidget,
};

// ── Animation ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct BubbleAnim {
    opacity:    f32,
    slide:      f32,
    target_opacity: f32,
    target_slide:   f32,
    delay_s:    f32,   // seconds to wait before animating
}

impl BubbleAnim {
    fn new_intro(slide_offset: f32, delay_s: f32) -> Self {
        Self {
            opacity: 0.0,
            slide: slide_offset,
            target_opacity: 1.0,
            target_slide: 0.0,
            delay_s,
        }
    }

    fn done(&self) -> bool {
        self.delay_s <= 0.0
            && (self.opacity - self.target_opacity).abs() < 0.005
            && (self.slide - self.target_slide).abs() < 0.5
    }

    fn tick(&mut self, dt: f32) {
        if self.delay_s > 0.0 {
            self.delay_s -= dt;
            return;
        }
        let speed = 6.0 * dt;
        self.opacity += (self.target_opacity - self.opacity) * speed.min(1.0);
        self.slide   += (self.target_slide - self.slide) * speed.min(1.0);

        if (self.opacity - self.target_opacity).abs() < 0.005 { self.opacity = self.target_opacity; }
        if (self.slide - self.target_slide).abs() < 0.5 { self.slide = self.target_slide; }
    }
}

struct AnimState {
    left:   Vec<BubbleAnim>,
    center: Vec<BubbleAnim>,
    right:  Vec<BubbleAnim>,
    last_frame: std::time::Instant,
}

impl AnimState {
    fn new_intro(left_n: usize, center_n: usize, right_n: usize) -> Self {
        let stagger = 0.06; // seconds between each bubble
        let slide_dist = 30.0;

        let left: Vec<BubbleAnim> = (0..left_n)
            .map(|i| BubbleAnim::new_intro(-slide_dist, i as f32 * stagger))
            .collect();
        let right: Vec<BubbleAnim> = (0..right_n)
            .map(|i| BubbleAnim::new_intro(slide_dist, i as f32 * stagger))
            .collect();
        let center: Vec<BubbleAnim> = (0..center_n)
            .map(|i| BubbleAnim::new_intro(0.0, (left_n as f32 + i as f32) * stagger))
            .collect();

        Self { left, center, right, last_frame: std::time::Instant::now() }
    }

    fn animating(&self) -> bool {
        self.left.iter().any(|a| !a.done())
            || self.center.iter().any(|a| !a.done())
            || self.right.iter().any(|a| !a.done())
    }

    fn tick(&mut self) {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        for a in self.left.iter_mut()
            .chain(self.center.iter_mut())
            .chain(self.right.iter_mut())
        {
            a.tick(dt);
        }
    }

    fn sync_counts(&mut self, left_n: usize, center_n: usize, right_n: usize) {
        let full = BubbleAnim { opacity: 1.0, slide: 0.0, target_opacity: 1.0, target_slide: 0.0, delay_s: 0.0 };
        self.left.resize(left_n, full.clone());
        self.center.resize(center_n, full.clone());
        self.right.resize(right_n, full);
    }

    fn flash_reload(&mut self) {
        for a in self.left.iter_mut().chain(self.center.iter_mut()).chain(self.right.iter_mut()) {
            a.opacity = 0.5;
            a.target_opacity = 1.0;
            a.delay_s = 0.0;
        }
    }
}

// ── Click zone tracking ───────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone { Left, Center, Right }

#[derive(Clone)]
struct ClickZone {
    x0:   f32,
    x1:   f32,
    zone: Zone,
    idx:  usize,
}

// ─────────────────────────────────────────────────────────────────────────────

pub fn run(mut cfg: BarConfig, shutdown: Arc<AtomicBool>) -> Result<()> {
    let rt = Runtime::new()?;

    let (mouse_tx, mouse_rx) = unbounded::<MouseEvent>();
    let mut surface = BarSurface::new(&cfg.position, cfg.height, mouse_tx)?;

    // Wait for at least one output to be configured
    for _ in 0..50 {
        surface.dispatch()?;
        if surface.output_count() > 0 { break; }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if surface.output_count() == 0 {
        anyhow::bail!("no Wayland outputs became available");
    }

    // Build widget lists from config (filter out separators — they're layout markers only)
    let mut left   = build_widgets(&cfg.modules.left, &cfg);
    let mut center = build_widgets(&cfg.modules.center, &cfg);
    let mut right  = build_widgets(&cfg.modules.right, &cfg);

    // Pre-compute bubble groupings from separator positions
    let mut left_groups   = compute_groups(&cfg.modules.left);
    let mut center_groups = compute_center_groups(&cfg.modules.center);
    let mut right_groups  = compute_groups(&cfg.modules.right);

    let mut text       = TextRenderer::new();
    let mut icons      = IconCache::new();
    let mut state      = SwayState::default();
    let mut click_zones: Vec<ClickZone> = Vec::new();
    let mut anim = AnimState::new_intro(left_groups.len(), center_groups.len(), right_groups.len());

    // Wallpaper theme watching
    let theme_path = wallpaper_theme_path();
    let mut last_theme_check = std::time::Instant::now();
    let mut last_theme_mtime: Option<std::time::SystemTime> = None;

    // Load wallpaper theme on startup if configured
    if cfg.theme_source == ThemeSource::Wallpaper {
        if let Some((t, bb)) = load_wallpaper_theme(&theme_path) {
            cfg.theme = t;
            if let Some(bg) = bb { cfg.bubbles.background = bg; }
            tracing::info!("bar: loaded wallpaper theme from {theme_path}");
        }
    }

    // Sway event subscription (optional — gracefully absent if not on sway)
    let mut sway_rx = if SwayClient::detect() {
        let client = SwayClient::new()?;
        let rx = {
            let _guard = rt.enter();
            client.subscribe()
        };

        // Initial fetch
        state = rt.block_on(async {
            let mut s = SwayState::default();
            if let Ok(ws) = client.workspaces().await { s.workspaces = ws; }
            if let Ok((title, class)) = client.focused_window().await {
                s.active_title = title;
                s.active_class = class;
            }
            s
        });

        Some((client, rx))
    } else {
        tracing::warn!("SWAYSOCK not set — workspace/title widgets inactive");
        None
    };

    let cava_in_modules = |m: &crate::config::Modules| {
        m.left.iter().chain(&m.center).chain(&m.right).any(|k| *k == ModuleKind::Cava)
    };
    let mut tick = std::time::Duration::from_millis(
        if cava_in_modules(&cfg.modules) { 33 } else { 1000 }
    );
    let mut last_tick = std::time::Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) { return Ok(()); }

        surface.dispatch()?;

        // Handle mouse clicks
        while let Ok(ev) = mouse_rx.try_recv() {
            if let MouseEvent::Press { x, y } = ev {
                handle_click(x, y, &click_zones, &mut left, &mut center, &mut right);
            }
        }

        // Poll sway events
        if let Some((ref client, ref mut rx)) = sway_rx {
            let mut dirty = false;
            while rx.try_recv().is_ok() { dirty = true; }
            if dirty {
                state = rt.block_on(async {
                    let mut s = SwayState::default();
                    if let Ok(ws) = client.workspaces().await { s.workspaces = ws; }
                    if let Ok((title, class)) = client.focused_window().await {
                        s.active_title = title;
                        s.active_class = class;
                    }
                    s
                });
                text.clear_dynamic_cache();
            }
        }

        // Poll wallpaper theme file for changes
        if cfg.theme_source == ThemeSource::Wallpaper && last_theme_check.elapsed().as_secs() >= 2 {
            last_theme_check = std::time::Instant::now();
            let mtime = std::fs::metadata(&theme_path).and_then(|m| m.modified()).ok();
            if mtime != last_theme_mtime {
                last_theme_mtime = mtime;
                if let Some((t, bb)) = load_wallpaper_theme(&theme_path) {
                    cfg.theme = t;
                    if let Some(bg) = bb { cfg.bubbles.background = bg; }
                    anim.flash_reload();
                    tracing::info!("bar: wallpaper theme updated");
                }
            }
        }

        let needs_anim = anim.animating();
        let redraw_interval = if needs_anim { std::time::Duration::from_millis(16) } else { tick };

        if last_tick.elapsed() >= redraw_interval {
            last_tick = std::time::Instant::now();
            if needs_anim { anim.tick(); }
            let zones = &mut click_zones;
            surface.present_for_each(|w, h| {
                zones.clear();
                render_frame(
                    w, h, &cfg,
                    &mut left, &mut center, &mut right,
                    &left_groups, &center_groups, &right_groups,
                    &mut text, &mut icons, &state,
                    zones, &anim,
                )
            })?;
        }

        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

/// Split a module list at Separator markers into groups of contiguous widget indices.
/// Each group is a Vec of indices into the widget list (which has separators removed).
/// For center modules, each module gets its own group (individual bubbles).
fn compute_groups(modules: &[ModuleKind]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut widget_idx = 0usize;
    let is_center = false; // we'll determine this from context differently

    for kind in modules {
        if *kind == ModuleKind::Separator {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(widget_idx);
            widget_idx += 1;
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    // if no separators were used, put everything in one group
    if groups.is_empty() && widget_idx > 0 {
        groups.push((0..widget_idx).collect());
    }
    groups
}

/// Compute groups for center modules — each module gets its own bubble.
fn compute_center_groups(modules: &[ModuleKind]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut widget_idx = 0usize;

    for kind in modules {
        if *kind != ModuleKind::Separator {
            groups.push(vec![widget_idx]);
            widget_idx += 1;
        }
    }
    groups
}

fn wallpaper_theme_path() -> String {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    format!("{runtime}/woven-theme.toml")
}

/// Extended theme struct for the wallpaper theme file (includes bubble_background).
#[derive(serde::Deserialize)]
struct WallpaperThemeFile {
    #[serde(flatten)]
    theme: crate::config::Theme,
    #[serde(default)]
    bubble_background: Option<String>,
}

fn load_wallpaper_theme(path: &str) -> Option<(crate::config::Theme, Option<String>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let wt: WallpaperThemeFile = toml::from_str(&content).ok()?;
    Some((wt.theme, wt.bubble_background))
}

// ─────────────────────────────────────────────────────────────────────────────

fn render_frame(
    width: u32,
    height: u32,
    cfg: &BarConfig,
    left:   &mut Vec<Box<dyn Widget>>,
    center: &mut Vec<Box<dyn Widget>>,
    right:  &mut Vec<Box<dyn Widget>>,
    left_groups:   &[Vec<usize>],
    center_groups: &[Vec<usize>],
    right_groups:  &[Vec<usize>],
    text:   &mut TextRenderer,
    icons:  &mut IconCache,
    state:  &SwayState,
    zones:  &mut Vec<ClickZone>,
    anim:   &AnimState,
) -> Vec<u8> {
    // For vertical bars, render into a virtual horizontal buffer (screen_h × thickness)
    // then rotate 90° into the compositor's thickness × screen_h buffer.
    // Left/right use opposite rotation so glyph tops point toward the screen center.
    let (render_w, render_h) = if cfg.position.is_vertical() {
        (height, width) // height = screen height (long axis), width = thickness
    } else {
        (width, height)
    };

    let mut pixmap = Pixmap::new(render_w, render_h).unwrap();
    let is_bubbles = cfg.style == BarStyle::Bubbles;

    if is_bubbles {
        clear(&mut pixmap, tiny_skia::Color::TRANSPARENT);
    } else {
        clear(&mut pixmap, hex_color(&cfg.theme.background));
    }

    let mut ctx = RenderCtx {
        pixmap: &mut pixmap,
        text,
        icons,
        theme: &cfg.theme,
        state,
        height: render_h,
    };

    if is_bubbles {
        render_bubbles(
            render_w, render_h, cfg,
            left, center, right,
            left_groups, center_groups, right_groups,
            &mut ctx, zones, anim,
        );
    } else {
        render_solid(render_w, cfg, left, center, right, &mut ctx, zones);
    }

    let data = ctx.pixmap.data();
    if cfg.position.is_vertical() {
        // dst dims: render_h × render_w (thickness wide, screen_h tall)
        let dst_w = render_h as usize;
        let dst_h = render_w as usize;
        let mut out = vec![0u8; dst_w * dst_h * 4];
        let is_right = matches!(cfg.position, BarPosition::Right);
        for sy in 0..render_h as usize {
            for sx in 0..render_w as usize {
                let src_idx = (sy * render_w as usize + sx) * 4;
                // Right bar: 90° CCW frame → glyph tops point left (toward center) ✓
                // Left bar:  90° CW  frame → glyph tops point right (toward center) ✓
                let (dx, dy) = if is_right {
                    (sy, render_w as usize - 1 - sx)
                } else {
                    (render_h as usize - 1 - sy, sx)
                };
                let dst_idx = (dy * dst_w + dx) * 4;
                let c = &data[src_idx..src_idx + 4];
                out[dst_idx]     = c[2];
                out[dst_idx + 1] = c[1];
                out[dst_idx + 2] = c[0];
                out[dst_idx + 3] = c[3];
            }
        }
        out
    } else {
        // Convert tiny-skia premultiplied RGBA → wl_shm ARGB8888
        let mut out = Vec::with_capacity(data.len());
        for chunk in data.chunks_exact(4) {
            out.push(chunk[2]); // B
            out.push(chunk[1]); // G
            out.push(chunk[0]); // R
            out.push(chunk[3]); // A
        }
        out
    }
}

/// Original solid-bar rendering
fn render_solid(
    width: u32,
    cfg: &BarConfig,
    left:   &mut Vec<Box<dyn Widget>>,
    center: &mut Vec<Box<dyn Widget>>,
    right:  &mut Vec<Box<dyn Widget>>,
    ctx:    &mut RenderCtx<'_>,
    zones:  &mut Vec<ClickZone>,
) {
    // ── Left zone ─────────────────────────────────────────────────────────────
    let mut x = 6.0f32;
    for (i, w) in left.iter_mut().enumerate() {
        let ww = w.width(ctx.theme, ctx.text) as f32;
        let x0 = x;
        w.render(ctx, x);
        zones.push(ClickZone { x0, x1: x + ww, zone: Zone::Left, idx: i });
        x += ww + 4.0;
    }

    // ── Right zone ───────────────────────────────────────────────────────────
    let right_gap    = 10.0f32;
    let outer_pad    = 10.0f32;
    let right_widths: Vec<f32> = right.iter()
        .map(|w| w.width(ctx.theme, ctx.text) as f32)
        .collect();
    let visible: Vec<(usize, f32)> = right_widths.iter()
        .copied()
        .enumerate()
        .filter(|(_, w)| *w > 0.0)
        .collect();
    let total_right: f32 = visible.iter().map(|(_, w)| w).sum::<f32>()
        + right_gap * visible.len().saturating_sub(1) as f32;
    let mut right_x = width as f32 - total_right - outer_pad;
    for (i, ww) in &visible {
        let x0 = right_x;
        right.get_mut(*i).unwrap().render(ctx, right_x);
        zones.push(ClickZone { x0, x1: right_x + ww, zone: Zone::Right, idx: *i });
        right_x += ww + right_gap;
    }

    // ── Center zone ──────────────────────────────────────────────────────────
    for (i, w) in center.iter_mut().enumerate() {
        w.render(ctx, x);
        let cw = w.width(ctx.theme, ctx.text) as f32;
        let cx = (width as f32 / 2.0 - cw / 2.0).max(x);
        zones.push(ClickZone { x0: cx, x1: cx + cw, zone: Zone::Center, idx: i });
    }
}

/// Apply opacity to a color.
fn with_opacity(c: Color, opacity: f32) -> Color {
    Color::from_rgba(c.red(), c.green(), c.blue(), c.alpha() * opacity).unwrap_or(c)
}

/// Bubbles rendering — each group gets its own rounded pill background
fn render_bubbles(
    width: u32,
    height: u32,
    cfg: &BarConfig,
    left:   &mut Vec<Box<dyn Widget>>,
    center: &mut Vec<Box<dyn Widget>>,
    right:  &mut Vec<Box<dyn Widget>>,
    left_groups:   &[Vec<usize>],
    center_groups: &[Vec<usize>],
    right_groups:  &[Vec<usize>],
    ctx:    &mut RenderCtx<'_>,
    zones:  &mut Vec<ClickZone>,
    anim:   &AnimState,
) {
    let bs = &cfg.bubbles;
    let pad    = bs.padding as f32;
    let gap    = bs.gap as f32;
    let margin = bs.margin as f32;
    let radius = bs.radius as f32;
    let inner_gap = 4.0f32;
    let bubble_bg = hex_color(&bs.background);
    let bubble_h = height as f32 - margin * 2.0;
    let bubble_y = margin;

    // ── Pre-measure all zones to calculate layout ─────────────────────────────

    // Measure left: starts at gap, each group adds (group_w + pad*2 + gap)
    let mut left_boundary = gap;
    for group in left_groups {
        let widths: Vec<f32> = group.iter()
            .map(|&i| left[i].width(ctx.theme, ctx.text) as f32)
            .collect();
        let visible: Vec<f32> = widths.iter().copied().filter(|w| *w > 0.0).collect();
        if visible.is_empty() { continue; }
        let group_w: f32 = visible.iter().sum::<f32>()
            + inner_gap * visible.len().saturating_sub(1) as f32;
        left_boundary += group_w + pad * 2.0 + gap;
    }

    // Measure right: original formula — gap*N includes trailing gap on rightmost
    let mut right_group_info: Vec<(&Vec<usize>, f32, Vec<(usize, f32)>)> = Vec::new();
    for group in right_groups {
        let widths: Vec<f32> = group.iter()
            .map(|&i| right[i].width(ctx.theme, ctx.text) as f32)
            .collect();
        let visible: Vec<(usize, f32)> = widths.iter().copied().enumerate()
            .filter(|(_, w)| *w > 0.0).collect();
        if visible.is_empty() { continue; }
        let group_w: f32 = visible.iter().map(|(_, w)| w).sum::<f32>()
            + inner_gap * visible.len().saturating_sub(1) as f32;
        right_group_info.push((group, group_w, visible));
    }
    let total_right_w: f32 = right_group_info.iter()
        .map(|(_, gw, _)| gw + pad * 2.0)
        .sum::<f32>()
        + gap * right_group_info.len() as f32;
    let right_boundary = width as f32 - total_right_w;

    // Measure center: each visible widget is its own bubble, gap between bubbles only
    let mut center_widths: Vec<f32> = Vec::new();
    let mut visible_center: Vec<usize> = Vec::new();
    let mut center_total = 0.0f32;
    for (i, w) in center.iter().enumerate() {
        let ww = w.width(ctx.theme, ctx.text) as f32;
        if ww > 0.0 {
            if !visible_center.is_empty() { center_total += gap; }
            center_total += ww + pad * 2.0;
            center_widths.push(ww);
            visible_center.push(i);
        }
    }

    // Center relative to full screen width, clamped to not overlap left
    let center_x = ((width as f32 / 2.0) - (center_total / 2.0)).max(left_boundary);

    // ── Render left bubbles ──────────────────────────────────────────────────
    let mut x = gap;
    let mut left_ai = 0usize;
    for group in left_groups {
        let widths: Vec<f32> = group.iter()
            .map(|&i| left[i].width(ctx.theme, ctx.text) as f32)
            .collect();
        let visible: Vec<(usize, f32)> = widths.iter().copied().enumerate()
            .filter(|(_, w)| *w > 0.0).collect();
        if visible.is_empty() { continue; }
        let group_w: f32 = visible.iter().map(|(_, w)| w).sum::<f32>()
            + inner_gap * visible.len().saturating_sub(1) as f32;

        let a = anim.left.get(left_ai).cloned().unwrap_or(BubbleAnim { opacity: 1.0, slide: 0.0, target_opacity: 1.0, target_slide: 0.0, delay_s: 0.0 });
        left_ai += 1;
        let bx = x + a.slide;

        fill_rounded_rect(ctx.pixmap, bx, bubble_y, group_w + pad * 2.0, bubble_h, radius, with_opacity(bubble_bg, a.opacity));

        let mut wx = bx + pad;
        for (vi, ww) in &visible {
            let widget_idx = group[*vi];
            let x0 = wx;
            if a.opacity > 0.1 {
                left[widget_idx].render(ctx, wx);
            }
            zones.push(ClickZone { x0, x1: wx + ww, zone: Zone::Left, idx: widget_idx });
            wx += ww + inner_gap;
        }

        x += group_w + pad * 2.0 + gap;
    }

    // ── Render center bubbles — each module gets its own bubble ──────────────
    let mut cx = center_x.max(left_boundary);
    let mut center_ai = 0usize;
    for (vi, &widget_idx) in visible_center.iter().enumerate() {
        let ww = center_widths[vi];
        let a = anim.center.get(center_ai).cloned().unwrap_or(BubbleAnim { opacity: 1.0, slide: 0.0, target_opacity: 1.0, target_slide: 0.0, delay_s: 0.0 });
        center_ai += 1;
        let bx = cx + a.slide;

        // Individual bubble for each module
        fill_rounded_rect(ctx.pixmap, bx, bubble_y, ww + pad * 2.0, bubble_h, radius, with_opacity(bubble_bg, a.opacity));

        let wx = bx + pad;
        let x0 = wx;
        if a.opacity > 0.1 {
            center[widget_idx].render(ctx, wx);
        }
        zones.push(ClickZone { x0, x1: wx + ww, zone: Zone::Center, idx: widget_idx });

        cx += ww + pad * 2.0 + gap;
    }

    // ── Render right bubbles ─────────────────────────────────────────────────
    let mut rx = right_boundary;
    let mut right_ai = 0usize;
    for (group, group_w, visible) in &right_group_info {
        let a = anim.right.get(right_ai).cloned().unwrap_or(BubbleAnim { opacity: 1.0, slide: 0.0, target_opacity: 1.0, target_slide: 0.0, delay_s: 0.0 });
        right_ai += 1;
        let bx = rx + a.slide;

        fill_rounded_rect(ctx.pixmap, bx, bubble_y, group_w + pad * 2.0, bubble_h, radius, with_opacity(bubble_bg, a.opacity));

        let mut wx = bx + pad;
        for (vi, ww) in visible {
            let widget_idx = group[*vi];
            let x0 = wx;
            if a.opacity > 0.1 {
                right[widget_idx].render(ctx, wx);
            }
            zones.push(ClickZone { x0, x1: wx + ww, zone: Zone::Right, idx: widget_idx });
            wx += ww + inner_gap;
        }

        rx += group_w + pad * 2.0 + gap;
    }
}

fn handle_click(
    x: f64,
    y: f64,
    zones: &[ClickZone],
    left:   &mut Vec<Box<dyn Widget>>,
    center: &mut Vec<Box<dyn Widget>>,
    right:  &mut Vec<Box<dyn Widget>>,
) {
    let fx = x as f32;
    // Find the innermost matching zone (last pushed = highest layer)
    for zone in zones.iter().rev() {
        if fx >= zone.x0 && fx <= zone.x1 {
            match zone.zone {
                Zone::Left   => { if let Some(w) = left.get_mut(zone.idx)   { w.on_click(x, y); } }
                Zone::Center => { if let Some(w) = center.get_mut(zone.idx) { w.on_click(x, y); } }
                Zone::Right  => { if let Some(w) = right.get_mut(zone.idx)  { w.on_click(x, y); } }
            }
            return;
        }
    }
}

fn build_widgets(kinds: &[ModuleKind], cfg: &BarConfig) -> Vec<Box<dyn Widget>> {
    kinds.iter().filter_map(|k| -> Option<Box<dyn Widget>> {
        match k {
            ModuleKind::Activities    => Some(Box::new(ActivitiesWidget::new())),
            ModuleKind::Workspaces    => Some(Box::new(WorkspacesWidget::new())),
            ModuleKind::WindowTitle   => Some(Box::new(WindowTitleWidget::new())),
            ModuleKind::Clock         => Some(Box::new(ClockWidget::new())),
            ModuleKind::Battery       => Some(Box::new(BatteryWidget::new())),
            ModuleKind::Audio         => Some(Box::new(AudioWidget::new())),
            ModuleKind::Network       => Some(Box::new(NetworkWidget::new())),
            ModuleKind::Systray       => Some(Box::new(SystrayWidget::new())),
            ModuleKind::Cpu           => Some(Box::new(CpuWidget::new())),
            ModuleKind::Memory        => Some(Box::new(MemoryWidget::new())),
            ModuleKind::Disk          => Some(Box::new(DiskWidget::new("/"))),
            ModuleKind::Temp          => Some(Box::new(TempWidget::new())),
            ModuleKind::Media         => Some(Box::new(MediaWidget::new())),
            ModuleKind::Notifications => Some(Box::new(NotificationsWidget::new())),
            ModuleKind::ControlCenter => Some(Box::new(ControlCenterWidget::new())),
            ModuleKind::Weather       => Some(Box::new(WeatherWidget::new(cfg.weather.lat, cfg.weather.lon))),
            ModuleKind::Date          => Some(Box::new(DateWidget::new())),
            ModuleKind::Cava          => Some(Box::new(CavaWidget::new())),
            ModuleKind::Separator     => None, // layout marker only
        }
    }).collect()
}
