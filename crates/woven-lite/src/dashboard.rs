//! Dashboard coordinator for woven-lite.
//!
//! Owns all sub-state (weather, sysinfo, themes, thumbnails) and
//! drives the tiny-skia paint pass each frame.
//!
//! Layout (700 × 500 logical px):
//!
//!  ┌──────────────────────────────────────────┐  ← y=0
//!  │  [clock 14:32]   [date Wed Jun 12]   [⚙]│  top bar (56px)
//!  ├──────────────────────────────────────────┤  ← y=56
//!  │  Partly Cloudy  72°F  RH 65%            │  weather row (32px)
//!  ├──────────────────────────────────────────┤  ← y=88
//!  │  WS 1       WS 2       WS 3   …         │  workspace tabs (36px)
//!  ├──────────────────────────────────────────┤  ← y=124
//!  │                                          │
//!  │   [card]  [card]  [card]                 │  window grid (flex)
//!  │                                          │
//!  ├──────────────────────────────────────────┤  ← y=456
//!  │  Battery: 87%   CPU: 12%   RAM: 45%      │  info bar (44px)
//!  └──────────────────────────────────────────┘  ← y=500
//!
//! Theme picker slides in from the right when ⚙ is clicked.

use std::collections::HashMap;
use chrono::Local;
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform, FillRule, PathBuilder};
use woven_common::types::{Theme, Workspace};
use woven_render::thumbnail::{Thumbnail, ThumbnailCache};

use crate::sysinfo::{SysCollector, SysSnapshot};
use crate::theme::{BuiltinTheme, ThemePicker};
use crate::weather::WeatherCache;

// ── Layout constants (logical pixels at 700×500) ──────────────────────────────

const W: f32 = 700.0;
const H: f32 = 500.0;

const TOP_H:     f32 = 56.0;
const WEATHER_H: f32 = 32.0;
const TABS_H:    f32 = 36.0;
const INFO_H:    f32 = 44.0;

const GRID_Y:    f32 = TOP_H + WEATHER_H + TABS_H;
const GRID_H:    f32 = H - GRID_Y - INFO_H;

const PAD:       f32 = 16.0;
const CARD_W:    f32 = 200.0;
const CARD_H:    f32 = 140.0;
const CARD_GAP:  f32 = 12.0;

const GEAR_SIZE: f32 = 28.0;
const THEME_PANEL_W: f32 = 200.0;

// ── Hit areas (for click detection) ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HitTarget {
    GearButton,
    WorkspaceTab(u32),
    WindowCard { window_id: String },
    ThemeOption(usize),
    ClosePanel,
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

pub struct Dashboard {
    pub theme_picker: ThemePicker,
    sys:              SysCollector,
    weather:          WeatherCache,
    last_sys:         SysSnapshot,
    thumbnails:       ThumbnailCache,
    workspaces:       Vec<Workspace>,
    active_ws:        u32,
    /// Hit rects for click dispatch: Vec<(x, y, w, h, target)>
    hit_rects:        Vec<(f32, f32, f32, f32, HitTarget)>,
}

impl Dashboard {
    pub fn new() -> Self {
        let mut sys = SysCollector::new();
        let snap    = sys.snapshot();
        Self {
            theme_picker: ThemePicker::default(),
            sys,
            weather:    WeatherCache::new(),
            last_sys:   snap,
            thumbnails: HashMap::new(),
            workspaces: Vec::new(),
            active_ws:  0,
            hit_rects:  Vec::new(),
        }
    }

    pub fn current_theme(&self) -> Theme {
        self.theme_picker.theme()
    }

    pub fn update_workspaces(&mut self, ws: Vec<Workspace>) {
        if let Some(active) = ws.iter().find(|w| w.active) {
            self.active_ws = active.id;
        }
        self.workspaces = ws;
    }

    pub fn update_thumbnails(&mut self, cache: ThumbnailCache) {
        self.thumbnails = cache;
    }

    pub fn tick_sys(&mut self) {
        self.last_sys = self.sys.snapshot();
    }

    // ── Hit testing ───────────────────────────────────────────────────────────

    pub fn hit_test(&self, x: f64, y: f64) -> Option<HitTarget> {
        let (fx, fy) = (x as f32, y as f32);
        for (rx, ry, rw, rh, target) in &self.hit_rects {
            if fx >= *rx && fx <= *rx + *rw && fy >= *ry && fy <= *ry + *rh {
                return Some(target.clone());
            }
        }
        None
    }

    // ── Paint ─────────────────────────────────────────────────────────────────

    /// Render a full 700×500 ARGB frame. Returns raw pixel bytes.
    pub fn paint(&mut self, width: u32, height: u32, anim_t: f32) -> Vec<u8> {
        let mut px = Pixmap::new(width, height).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
        self.hit_rects.clear();

        let theme = self.theme_picker.theme();
        let bg    = parse_color(&theme.background, anim_t * theme.opacity);

        // ── background ────────────────────────────────────────────────────────
        px.fill(bg);
        // rounded rect border
        fill_rounded_rect(&mut px, 0.0, 0.0, width as f32, height as f32,
                          theme.border_radius as f32, parse_color(&theme.border, 0.3 * anim_t));

        let wf = width as f32;
        let hf = height as f32;

        // ── top bar ───────────────────────────────────────────────────────────
        self.paint_top_bar(&mut px, &theme, wf, anim_t);

        // ── weather row ───────────────────────────────────────────────────────
        self.paint_weather(&mut px, &theme, wf, anim_t);

        // ── workspace tabs ────────────────────────────────────────────────────
        self.paint_ws_tabs(&mut px, &theme, wf, anim_t);

        // ── window grid ───────────────────────────────────────────────────────
        self.paint_window_grid(&mut px, &theme, wf, anim_t);

        // ── info bar ──────────────────────────────────────────────────────────
        self.paint_info_bar(&mut px, &theme, wf, hf, anim_t);

        // ── theme picker panel (slides in from right) ─────────────────────────
        if self.theme_picker.open {
            self.paint_theme_panel(&mut px, &theme, wf, hf, anim_t);
        }

        px.data().to_vec()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Section painters
    // ─────────────────────────────────────────────────────────────────────────

    fn paint_top_bar(&mut self, px: &mut Pixmap, theme: &Theme, w: f32, anim_t: f32) {
        // Divider line
        fill_rect(px, 0.0, TOP_H - 1.0, w, 1.0, parse_color(&theme.border, 0.25 * anim_t));

        let now   = Local::now();
        let clock = now.format("%H:%M").to_string();
        let date  = now.format("%A, %B %-d").to_string();

        // Clock — left
        draw_text_simple(px, &clock, PAD, TOP_H / 2.0 - 12.0, 28.0,
                         parse_color(&theme.text, anim_t), theme);
        // Date — center
        let date_x = w / 2.0 - 60.0;
        draw_text_simple(px, &date, date_x, TOP_H / 2.0 - 8.0, 14.0,
                         parse_color(&theme.text, 0.7 * anim_t), theme);

        // Gear button — right
        let gx = w - PAD - GEAR_SIZE;
        let gy = (TOP_H - GEAR_SIZE) / 2.0;
        fill_rounded_rect(px, gx, gy, GEAR_SIZE, GEAR_SIZE, 6.0,
                          parse_color(&theme.accent, 0.2 * anim_t));
        draw_text_simple(px, "⚙", gx + 6.0, gy + 4.0, 16.0,
                         parse_color(&theme.accent, anim_t), theme);
        self.hit_rects.push((gx, gy, GEAR_SIZE, GEAR_SIZE, HitTarget::GearButton));
    }

    fn paint_weather(&mut self, px: &mut Pixmap, theme: &Theme, w: f32, anim_t: f32) {
        let y = TOP_H;
        fill_rect(px, 0.0, y + WEATHER_H - 1.0, w, 1.0,
                  parse_color(&theme.border, 0.15 * anim_t));

        let text = self.weather.get()
            .unwrap_or_else(|| "Fetching weather…".into());
        draw_text_simple(px, &text, PAD, y + 8.0, 13.0,
                         parse_color(&theme.text, 0.85 * anim_t), theme);
    }

    fn paint_ws_tabs(&mut self, px: &mut Pixmap, theme: &Theme, w: f32, anim_t: f32) {
        let y = TOP_H + WEATHER_H;
        fill_rect(px, 0.0, y + TABS_H - 1.0, w, 1.0,
                  parse_color(&theme.border, 0.2 * anim_t));

        let tab_w  = 100.0_f32;
        let tab_h  = TABS_H - 8.0;
        let tab_y  = y + 4.0;

        for (i, ws) in self.workspaces.iter().enumerate() {
            let tx   = PAD + i as f32 * (tab_w + 6.0);
            let active = ws.id == self.active_ws;
            let bg_a = if active { 0.35 * anim_t } else { 0.12 * anim_t };

            fill_rounded_rect(px, tx, tab_y, tab_w, tab_h, 6.0,
                              parse_color(&theme.accent, bg_a));
            if active {
                // active indicator stripe
                fill_rect(px, tx, tab_y + tab_h - 2.0, tab_w, 2.0,
                          parse_color(&theme.accent, anim_t));
            }

            let label = if ws.name.is_empty() {
                format!("WS {}", ws.id)
            } else {
                ws.name.chars().take(10).collect()
            };
            draw_text_simple(px, &label, tx + 6.0, tab_y + 8.0, 12.0,
                             parse_color(&theme.text, anim_t), theme);

            self.hit_rects.push((tx, tab_y, tab_w, tab_h, HitTarget::WorkspaceTab(ws.id)));
        }
    }

    fn paint_window_grid(&mut self, px: &mut Pixmap, theme: &Theme, _w: f32, anim_t: f32) {
        let active_ws = self.workspaces.iter().find(|ws| ws.id == self.active_ws);
        let windows = match active_ws {
            Some(ws) => ws.windows.clone(),
            None     => return,
        };

        for (i, win) in windows.iter().enumerate() {
            let col = i % 3;
            let row = i / 3;
            let cx  = PAD + col as f32 * (CARD_W + CARD_GAP);
            let cy  = GRID_Y + 8.0 + row as f32 * (CARD_H + CARD_GAP);

            if cy + CARD_H > GRID_Y + GRID_H { break; } // out of grid bounds

            // Card background
            fill_rounded_rect(px, cx, cy, CARD_W, CARD_H, 8.0,
                              parse_color(&theme.border, 0.12 * anim_t));

            // Thumbnail or placeholder
            let thumb_h = CARD_H * 0.60;
            if let Some((tw, th, pixels)) = self.thumbnails.get(&win.id) {
                blit_thumbnail(px, pixels, *tw, *th,
                               cx as u32, cy as u32, CARD_W as u32, thumb_h as u32);
            } else {
                // Placeholder: accent-tinted fill
                fill_rounded_rect(px, cx + 2.0, cy + 2.0, CARD_W - 4.0, thumb_h - 4.0, 6.0,
                                  parse_color(&theme.accent, 0.08 * anim_t));
                // App class initial letter
                let initial = win.class.chars().next().unwrap_or('?').to_uppercase().to_string();
                draw_text_simple(px, &initial,
                                 cx + CARD_W / 2.0 - 10.0,
                                 cy + thumb_h / 2.0 - 14.0,
                                 28.0, parse_color(&theme.accent, 0.5 * anim_t), theme);
            }

            // App class label
            let label: String = win.class.chars().take(18).collect();
            draw_text_simple(px, &label, cx + 6.0, cy + thumb_h + 6.0, 11.0,
                             parse_color(&theme.text, 0.9 * anim_t), theme);

            // Title (truncated)
            let title: String = win.title.chars().take(22).collect();
            draw_text_simple(px, &title, cx + 6.0, cy + thumb_h + 20.0, 10.0,
                             parse_color(&theme.text, 0.55 * anim_t), theme);

            self.hit_rects.push((cx, cy, CARD_W, CARD_H,
                                 HitTarget::WindowCard { window_id: win.id.clone() }));
        }
    }

    fn paint_info_bar(&self, px: &mut Pixmap, theme: &Theme, w: f32, h: f32, anim_t: f32) {
        let y = h - INFO_H;
        fill_rect(px, 0.0, y, w, 1.0, parse_color(&theme.border, 0.25 * anim_t));

        let sys = &self.last_sys;

        let cpu_str = format!("CPU: {:.0}%", sys.cpu_pct);
        let mem_str = format!("RAM: {:.0}%", sys.mem_pct());
        let bat_str = sys.battery_pct.map(|p| {
            let icon = if sys.charging { "⚡" } else { "🔋" };
            format!("{} {:.0}%", icon, p)
        }).unwrap_or_default();

        let items: Vec<&str> = if bat_str.is_empty() {
            vec![&cpu_str, &mem_str]
        } else {
            vec![&bat_str, &cpu_str, &mem_str]
        };

        let spacing = w / (items.len() as f32 + 1.0);
        for (i, item) in items.iter().enumerate() {
            let x = spacing * (i as f32 + 1.0) - 30.0;
            draw_text_simple(px, item, x, y + 14.0, 12.0,
                             parse_color(&theme.text, 0.8 * anim_t), theme);
        }
    }

    fn paint_theme_panel(&mut self, px: &mut Pixmap, theme: &Theme, w: f32, h: f32, anim_t: f32) {
        let px_left = w - THEME_PANEL_W;
        // Panel background
        fill_rounded_rect(px, px_left, 0.0, THEME_PANEL_W, h, 0.0,
                          parse_color(&theme.background, 0.97 * anim_t));
        fill_rect(px, px_left, 0.0, 1.0, h,
                  parse_color(&theme.border, 0.5 * anim_t));

        draw_text_simple(px, "Themes", px_left + 12.0, 16.0, 14.0,
                         parse_color(&theme.text, anim_t), theme);

        // Close button
        let close_x = w - 28.0;
        draw_text_simple(px, "✕", close_x, 14.0, 14.0,
                         parse_color(&theme.text, 0.7 * anim_t), theme);
        self.hit_rects.push((close_x, 8.0, 24.0, 24.0, HitTarget::ClosePanel));

        for (i, t) in BuiltinTheme::ALL.iter().enumerate() {
            let ty  = 52.0 + i as f32 * 44.0;
            let sel = *t == self.theme_picker.current;
            fill_rounded_rect(px, px_left + 8.0, ty, THEME_PANEL_W - 16.0, 36.0, 6.0,
                              parse_color(&theme.accent, if sel { 0.3 } else { 0.08 } * anim_t));
            draw_text_simple(px, t.name(), px_left + 16.0, ty + 10.0, 13.0,
                             parse_color(&theme.text, anim_t), theme);
            self.hit_rects.push((px_left + 8.0, ty, THEME_PANEL_W - 16.0, 36.0,
                                 HitTarget::ThemeOption(i)));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Draw helpers (tiny-skia, no fontdue dependency in this file)
// ─────────────────────────────────────────────────────────────────────────────

fn parse_color(hex: &str, alpha: f32) -> Color {
    let h = hex.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x888888);
    let r = ((v >> 16) & 0xff) as f32 / 255.0;
    let g = ((v >> 8)  & 0xff) as f32 / 255.0;
    let b = (v         & 0xff) as f32 / 255.0;
    Color::from_rgba(r, g, b, alpha.clamp(0.0, 1.0)).unwrap_or(Color::WHITE)
}

fn fill_rect(px: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 { return; }
    let mut paint = Paint::default();
    paint.set_color(color);
    if let Some(rect) = Rect::from_xywh(x, y, w, h) {
        px.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

fn fill_rounded_rect(px: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    if w <= 0.0 || h <= 0.0 { return; }
    if r <= 0.0 || w < r * 2.0 || h < r * 2.0 {
        fill_rect(px, x, y, w, h, color);
        return;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        px.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// Minimal bitmap text — renders each character as a filled rect block.
/// Real fontdue rendering lives in woven-render::text; this is a placeholder
/// until we wire up the shared TextRenderer here.
///
/// TODO: replace with TextRenderer from woven-render once the surface wiring is done.
fn draw_text_simple(px: &mut Pixmap, text: &str, x: f32, y: f32, _size: f32, color: Color, _theme: &Theme) {
    // Stub: draw a thin colored line indicating where text would be.
    // Will be replaced by proper fontdue rendering in the render pass.
    let w = (text.len() as f32 * 7.0).max(4.0);
    fill_rect(px, x, y, w, 2.0, color);
}

/// Blit a thumbnail (XRGB8888) into a region of the pixmap, nearest-neighbour scaled.
fn blit_thumbnail(
    px:     &mut Pixmap,
    pixels: &[u8],
    src_w:  u32,
    src_h:  u32,
    dst_x:  u32,
    dst_y:  u32,
    dst_w:  u32,
    dst_h:  u32,
) {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 { return; }
    let pw = px.width();
    let ph = px.height();
    let data = px.data_mut();

    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let ox = dst_x + dx;
            let oy = dst_y + dy;
            if ox >= pw || oy >= ph { continue; }

            let sx = ((dx as f32 / dst_w as f32) * src_w as f32) as usize;
            let sy = ((dy as f32 / dst_h as f32) * src_h as f32) as usize;
            let si = (sy * src_w as usize + sx) * 4;
            if si + 3 >= pixels.len() { continue; }

            // XRGB8888 LE → [B, G, R, X] in memory
            let b = pixels[si];
            let g = pixels[si + 1];
            let r = pixels[si + 2];

            // Pixmap is ARGB premultiplied; write fully opaque
            let di = ((oy * pw + ox) * 4) as usize;
            data[di]     = b;
            data[di + 1] = g;
            data[di + 2] = r;
            data[di + 3] = 255;
        }
    }
}
