//! ChromeOS-style overview layout.
//!
//! Zones (top to bottom):
//!   TOP_BAR    (48px)  — hostname · distro · kernel · cpu/mem stats, close button
//!   WS_STRIP   (148px) — horizontal row of workspace thumbnail cards
//!   MAIN_VIEW  (flex)  — window cards for the selected workspace
//!   WIDGET_BAR (64px)  — clock, date, window count
//!
//! Workspace strip thumbnails come from a growing per-workspace screenshot cache.
//! Each time the user focuses a workspace, we screencopy it and store it keyed by
//! workspace id.  Unvisited workspaces show a placeholder.
//!
//! Window cards show (in priority order):
//!   1. Per-window screencopy thumbnail — captured when the overlay opens.
//!   2. XDG app icon — looked up from .desktop files + hicolor icon theme.
//!   3. Colored circle with first letter of app name (no NF glyphs, no dollar signs).
//!
//! The overlay backdrop is the most recent full-output screenshot, darkened.
//!
//! NO anti-aliased fill_path on small shapes. All rounded rects fall back to
//! plain fill_rect when too small, eliminating the tiny-skia hairline AA panic.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};
use woven_common::types::{AnimationConfig, BarPosition, DrawCmd, LayoutConfig, Theme, WidgetDef, WidgetSlot, Workspace, WorkspaceMetrics};
use crate::icons::IconCache;
use crate::text::TextRenderer;
use crate::thumbnail::{Thumbnail, ThumbnailCache, WorkspaceCache};
use tracing::warn;

// Default layout values — mirrors LayoutConfig::default().
// These are NOT used at runtime; each draw function reads from self.layout instead.
// Kept here only as a quick reference for what the defaults are.
#[allow(dead_code)] const DEF_TOP_H:           f32 = 48.0;
#[allow(dead_code)] const DEF_WS_STRIP_H:      f32 = 148.0;
#[allow(dead_code)] const DEF_WIDGET_H:        f32 = 56.0;
#[allow(dead_code)] const DEF_OUTER_PAD:       f32 = 20.0;
#[allow(dead_code)] const DEF_STRIP_GAP:       f32 = 12.0;
#[allow(dead_code)] const DEF_WS_THUMB_W:      f32 = 200.0;
#[allow(dead_code)] const DEF_WS_THUMB_H:      f32 = 110.0;
#[allow(dead_code)] const DEF_WS_LABEL_H:      f32 = 20.0;
#[allow(dead_code)] const DEF_WS_BTN_H:        f32 = 18.0;
#[allow(dead_code)] const DEF_CARD_PAD:        f32 = 16.0;
#[allow(dead_code)] const DEF_CARD_GAP:        f32 = 12.0;
#[allow(dead_code)] const DEF_CARD_THUMB_RATIO: f32 = 0.65;

// ── SysInfo ───────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct SysInfo {
    pub hostname:      String,
    pub distro:        String,
    pub kernel:        String,
    pub uptime_s:      u64,
    pub cpu_pct:       f32,
    pub cpu_temp_c:    Option<f32>,
    pub mem_used_kb:   u64,
    pub mem_total_kb:  u64,
    pub volume_pct:    Option<f32>,
    pub wifi_ssid:     Option<String>,
    pub bt_on:         bool,
    pub media_title:   Option<String>,
    pub media_artist:  Option<String>,
    pub media_playing: bool,
    pub gpu_temp_c:    Option<f32>,
    pub weather:       Option<String>,
}

impl SysInfo {
    pub fn collect() -> Self {
        let (media_title, media_artist, media_playing) = read_media();
        SysInfo {
            hostname:      read_file("/etc/hostname").trim().to_string(),
            distro:        read_os_key("PRETTY_NAME").unwrap_or("Linux".into()),
            kernel:        read_file("/proc/sys/kernel/osrelease").trim().to_string(),
            uptime_s:      read_uptime(),
            cpu_pct:       read_cpu_pct(),
            cpu_temp_c:    read_cpu_temp(),
            mem_used_kb:   { let (u,_) = read_mem(); u },
            mem_total_kb:  { let (_,t) = read_mem(); t },
            volume_pct:    read_volume(),
            wifi_ssid:     read_wifi(),
            bt_on:         read_bt_on(),
            media_title,
            media_artist,
            media_playing,
            gpu_temp_c:    read_gpu_temp(),
            weather:       read_weather(),
        }
    }
    pub fn uptime_str(&self) -> String {
        let h = self.uptime_s / 3600;
        let m = (self.uptime_s % 3600) / 60;
        if h > 0 { format!("{}h {}m", h, m) } else { format!("{}m", m) }
    }
}

fn read_file(p: &str) -> String { std::fs::read_to_string(p).unwrap_or_default() }
fn read_os_key(key: &str) -> Option<String> {
    for line in read_file("/etc/os-release").lines() {
        if line.starts_with(key) {
            return Some(line.split_once('=')?.1.trim_matches('"').to_string());
        }
    }
    None
}
fn read_uptime() -> u64 {
    read_file("/proc/uptime").split_whitespace().next()
    .and_then(|v| v.parse::<f64>().ok()).map(|v| v as u64).unwrap_or(0)
}
fn read_cpu_pct() -> f32 {
    use std::sync::Mutex;
    static PREV: Mutex<Option<(u64,u64)>> = Mutex::new(None);
    let snap = || -> Option<(u64,u64)> {
        let s = std::fs::read_to_string("/proc/stat").ok()?;
        let nums: Vec<u64> = s.lines().next()?.split_whitespace().skip(1)
        .filter_map(|v| v.parse().ok()).collect();
        if nums.len() < 4 { return None; }
        Some((nums[3], nums.iter().sum()))
    };
    let now = snap();
    let mut g = PREV.lock().unwrap();
    let r = match (*g, now) {
        (Some((i1,t1)), Some((i2,t2))) => {
            let dt = t2.saturating_sub(t1).max(1) as f32;
            let di = i2.saturating_sub(i1) as f32;
            ((1.0 - di/dt) * 100.0).clamp(0.0,100.0)
        }
        _ => 0.0,
    };
    *g = now; r
}
fn read_mem() -> (u64, u64) {
    let mut tot = 0u64; let mut avail = 0u64;
    for line in read_file("/proc/meminfo").lines() {
        if      line.starts_with("MemTotal:")     { tot   = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { avail = parse_kb(line); }
    }
    (tot.saturating_sub(avail), tot)
}
fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0)
}
fn read_volume() -> Option<f32> {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    static CACHE:   Mutex<Option<f32>> = Mutex::new(None);
    static RUNNING: AtomicBool         = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            let val = std::process::Command::new("wpctl")
                .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
                .output().ok()
                .and_then(|out| {
                    let s = String::from_utf8_lossy(&out.stdout);
                    s.split_whitespace().nth(1)
                        .and_then(|v| v.parse::<f32>().ok())
                        .map(|v| (v * 100.0).round())
                });
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().and_then(|g| *g)
}

fn read_cpu_temp() -> Option<f32> {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    static CACHE:   Mutex<Option<f32>> = Mutex::new(None);
    static RUNNING: AtomicBool         = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            // Try thermal_zone0..9; pick the first that looks like a CPU sensor.
            let val = (0u32..10).find_map(|i| {
                let type_path = format!("/sys/class/thermal/thermal_zone{}/type", i);
                let temp_path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
                let kind = std::fs::read_to_string(&type_path).unwrap_or_default();
                // Prefer zones labelled x86_pkg_temp, acpitz, cpu-thermal, etc.
                let is_cpu = kind.contains("pkg") || kind.contains("cpu") || kind.contains("acpi");
                if !is_cpu { return None; }
                let raw: f32 = std::fs::read_to_string(&temp_path).ok()?.trim().parse().ok()?;
                Some(raw / 1000.0)
            }).or_else(|| {
                // Fallback: just take thermal_zone0 if it exists.
                std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()
                    .and_then(|s| s.trim().parse::<f32>().ok())
                    .map(|v| v / 1000.0)
            });
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().and_then(|g| *g)
}

fn read_wifi() -> Option<String> {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    static CACHE:   Mutex<Option<String>> = Mutex::new(None);
    static RUNNING: AtomicBool            = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            let val = std::process::Command::new("nmcli")
                .args(["-t", "-f", "active,ssid", "dev", "wifi"])
                .output().ok()
                .and_then(|out| {
                    String::from_utf8_lossy(&out.stdout).lines()
                        .find(|l| l.starts_with("yes:"))
                        .map(|l| l.trim_start_matches("yes:").to_string())
                })
                .filter(|s| !s.is_empty());
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().and_then(|g| g.clone())
}

fn read_bt_on() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CACHE:   AtomicBool = AtomicBool::new(false);
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            let on = std::process::Command::new("bluetoothctl")
                .arg("show")
                .output().ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).contains("Powered: yes"))
                .unwrap_or(false);
            CACHE.store(on, Ordering::Relaxed);
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.load(Ordering::Relaxed)
}

fn read_media() -> (Option<String>, Option<String>, bool) {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    static CACHE:   Mutex<(Option<String>, Option<String>, bool)> = Mutex::new((None, None, false));
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            let status = std::process::Command::new("playerctl")
                .arg("status")
                .output().ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .unwrap_or_default();
            let playing = status == "Playing";
            let paused  = status == "Paused";
            let val = if playing || paused {
                let title = std::process::Command::new("playerctl")
                    .args(["metadata", "title"])
                    .output().ok()
                    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                    .filter(|s| !s.is_empty());
                let artist = std::process::Command::new("playerctl")
                    .args(["metadata", "artist"])
                    .output().ok()
                    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                    .filter(|s| !s.is_empty());
                (title, artist, playing)
            } else {
                (None, None, false)
            };
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().map(|g| g.clone()).unwrap_or((None, None, false))
}

fn read_gpu_temp() -> Option<f32> {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    static CACHE:   Mutex<Option<f32>> = Mutex::new(None);
    static RUNNING: AtomicBool         = AtomicBool::new(false);
    if !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            // NVIDIA
            let val = std::process::Command::new("nvidia-smi")
                .args(["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"])
                .output().ok()
                .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse::<f32>().ok())
                .or_else(|| {
                    // AMD — scan hwmon entries for a GPU card
                    (0..8u32).find_map(|card| {
                        (0..8u32).find_map(|hw| {
                            let p = format!("/sys/class/drm/card{}/device/hwmon/hwmon{}/temp1_input", card, hw);
                            std::fs::read_to_string(&p).ok()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .map(|v| v / 1000.0)
                        })
                    })
                });
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().and_then(|g| *g)
}

fn read_weather() -> Option<String> {
    use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
    // Refresh much less frequently — weather changes slowly.
    static CACHE:        Mutex<Option<String>> = Mutex::new(None);
    static RUNNING:      AtomicBool            = AtomicBool::new(false);
    static LAST_REFRESH: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let needs_refresh = LAST_REFRESH.lock().ok()
        .map(|t| t.is_none_or(|ts| ts.elapsed().as_secs() > 600))
        .unwrap_or(true);
    if needs_refresh && !RUNNING.swap(true, Ordering::Relaxed) {
        std::thread::spawn(|| {
            // wttr.in minimal format: temperature + condition text (no emoji)
            let val = std::process::Command::new("curl")
                .args(["-s", "--max-time", "6", "--user-agent", "curl/7.x",
                       "wttr.in?format=%t+%C"])
                .output().ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .filter(|s| {
                    !s.is_empty()
                        && !s.starts_with('<')
                        && !s.contains("Unknown Location")
                        && s.len() < 60
                });
            if let Ok(mut g) = CACHE.lock() { *g = val; }
            if let Ok(mut t) = LAST_REFRESH.lock() { *t = Some(std::time::Instant::now()); }
            RUNNING.store(false, Ordering::Relaxed);
        });
    }
    CACHE.lock().ok().and_then(|g| g.clone())
}

use crossbeam_channel::Sender;
use crate::thread::WindowAction;

// ── Hit rects ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct ButtonRect { x: f32, y: f32, w: f32, h: f32, action: WindowAction }
impl ButtonRect {
    fn hit(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x+self.w && my >= self.y && my <= self.y+self.h
    }
}

#[derive(Clone)]
struct CardRect { x: f32, y: f32, w: f32, h: f32, window_id: String }
impl CardRect {
    fn hit(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x+self.w && my >= self.y && my <= self.y+self.h
    }
}

#[derive(Clone)]
struct WsThumbRect { x: f32, y: f32, w: f32, h: f32, ws_idx: usize }
impl WsThumbRect {
    fn hit(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x+self.w && my >= self.y && my <= self.y+self.h
    }
}

#[derive(Clone)]
struct LaunchZone { x: f32, y: f32, w: f32, h: f32, cmd: String }
impl LaunchZone {
    fn hit(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x+self.w && my >= self.y && my <= self.y+self.h
    }
}

// ── Painter ───────────────────────────────────────────────────────────────────

pub struct Painter {
    theme:      Theme,
    #[allow(dead_code)]
    anims:      AnimationConfig,
    workspaces: Vec<Workspace>,
    #[allow(dead_code)]
    metrics:    Vec<WorkspaceMetrics>,
    sys:        SysInfo,
    /// Time-based sys refresh — shared between bar and overlay.
    sys_last:   std::time::Instant,
    text:       TextRenderer,
    icons:      IconCache,
    selected_ws: usize,
    mouse_x:    f32,
    mouse_y:    f32,
    buttons:    Vec<ButtonRect>,
    cards:      Vec<CardRect>,
    ws_thumbs:  Vec<WsThumbRect>,
    action_tx:  Sender<WindowAction>,
    pixmap:     Option<Pixmap>,
    anim_slide: f32,
    thumbnails: ThumbnailCache,
    output_thumb: Option<Thumbnail>,
    workspace_cache: WorkspaceCache,
    show_empty: bool,
    /// Keyboard-focused card index into `self.cards` (None = no keyboard focus).
    kb_win: Option<usize>,
    /// Whether the search box is currently active.
    search_active: bool,
    /// Current search query string.
    search_query:  String,
    // hover popup
    hovered_ws_idx: Option<usize>,
    // slide-out preview panel
    expanded_ws:       Option<u32>,
    pending_preview_ws: Option<u32>,
    panel_anim:        f32,
    /// Layout dimensions — configurable via `woven.layout({})`.
    layout:          LayoutConfig,
    /// Plugin bar widgets: definition + last-rendered draw commands.
    bar_widgets:     Vec<(WidgetDef, Vec<DrawCmd>)>,
    /// Click zones for overlay launcher widgets — rebuilt every paint.
    launcher_zones:  Vec<LaunchZone>,
    /// Per-app accent colors from `woven.rules({})` — class (lowercase) → Color.
    app_rules:       std::collections::HashMap<String, Color>,
    // persistent bar
    bar_pixmap:      Option<Pixmap>,
    bar_cards:       Vec<WsThumbRect>,
    bar_buttons:     Vec<ButtonRect>,
    bar_mouse_x:     f32,
    bar_mouse_y:     f32,
    /// Whether the bar is in expanded control-center mode.
    panel_expanded:  bool,
    /// Active error toast: (message, shown_at, duration_ms).
    toast: Option<(String, std::time::Instant, u32)>,
}

impl Painter {
    pub fn new(theme: Theme, anims: AnimationConfig, action_tx: Sender<WindowAction>) -> Self {
        // Seed sys_last 5 seconds in the past so the first paint triggers a collect.
        let sys_last = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(5))
            .unwrap_or_else(std::time::Instant::now);
        Self {
            theme, anims,
            workspaces: vec![], metrics: vec![],
            sys: SysInfo::default(), sys_last,
            text: TextRenderer::new(), icons: IconCache::new(),
            selected_ws: 0,
            mouse_x: 0.0, mouse_y: 0.0,
            buttons: vec![], cards: vec![], ws_thumbs: vec![],
            action_tx,
            pixmap: None, anim_slide: 0.0,
            thumbnails: ThumbnailCache::new(),
            output_thumb: None,
            workspace_cache: WorkspaceCache::new(),
            show_empty: false,
            kb_win: None,
            search_active: false,
            search_query:  String::new(),
            hovered_ws_idx: None,
            expanded_ws: None,
            pending_preview_ws: None,
            panel_anim: 0.0,
            layout:         LayoutConfig::default(),
            bar_widgets:    vec![],
            app_rules:      std::collections::HashMap::new(),
            bar_pixmap:     None,
            bar_cards:      vec![],
            bar_buttons:    vec![],
            bar_mouse_x:    0.0,
            bar_mouse_y:    0.0,
            panel_expanded: false,
            toast: None,
            launcher_zones: vec![],
        }
    }

    pub fn set_panel_expanded(&mut self, v: bool) { self.panel_expanded = v; }

    /// Show an error toast for `duration_ms` milliseconds.
    pub fn show_toast(&mut self, message: String, duration_ms: u32) {
        self.toast = Some((message, std::time::Instant::now(), duration_ms));
    }
    /// Clear keyboard focus and search state — call when the overlay is hidden.
    pub fn reset_kb(&mut self) {
        self.kb_win        = None;
        self.search_active = false;
        self.search_query.clear();
    }

    pub fn update_theme(&mut self, t: Theme)         { self.theme  = t; }
    pub fn update_layout(&mut self, l: LayoutConfig) { self.layout = l; }
    pub fn update_icon_overrides(
        &mut self,
        map: std::collections::HashMap<String, String>,
        default_icon: Option<String>,
    ) {
        for (class, path) in map {
            self.icons.register_override(class, path);
        }
        if let Some(path) = default_icon {
            self.icons.register_override_default(path);
        }
    }

    pub fn update_app_rules(&mut self, rules: std::collections::HashMap<String, String>) {
        self.app_rules.clear();
        for (class, hex) in rules {
            self.app_rules.insert(class.to_lowercase(), parse_color(&hex, 1.0));
        }
    }

    /// Returns the accent color for an app class.
    /// Checks `woven.rules()` first, falls back to the hash-based color.
    fn app_color(&self, class: &str) -> Color {
        self.app_rules.get(&class.to_lowercase())
            .copied()
            .unwrap_or_else(|| class_color(class))
    }

    pub fn register_widget(&mut self, def: WidgetDef) {
        if let Some(entry) = self.bar_widgets.iter_mut().find(|(d,_)| d.id == def.id) {
            entry.0 = def; // update definition, keep existing draw cmds
        } else {
            self.bar_widgets.push((def, vec![]));
        }
    }

    pub fn update_widget_content(&mut self, id: String, cmds: Vec<DrawCmd>) {
        if let Some(entry) = self.bar_widgets.iter_mut().find(|(d,_)| d.id == id) {
            entry.1 = cmds;
        }
    }
    pub fn update_settings(&mut self, show_empty: bool) { self.show_empty = show_empty; }

    pub fn update_state(&mut self, ws: Vec<Workspace>, met: Vec<WorkspaceMetrics>) {
        self.text.clear_dynamic_cache();
        let all: Vec<Workspace> = if self.show_empty { ws }
        else { ws.into_iter().filter(|w| !w.windows.is_empty()).collect() };
        let active_idx = all.iter().position(|w| w.active).unwrap_or(0);
        if self.workspaces.is_empty() { self.selected_ws = active_idx; }
        else { self.selected_ws = self.selected_ws.min(all.len().saturating_sub(1)); }
        self.workspaces = all; self.metrics = met;
    }

    pub fn all_windows(&self) -> Vec<woven_common::types::Window> {
        self.workspaces.iter().flat_map(|ws| ws.windows.iter().cloned()).collect()
    }
    pub fn active_workspace_id(&self) -> Option<u32> {
        self.workspaces.iter().find(|w| w.active).map(|w| w.id)
    }

    pub fn update_thumbnails(&mut self, cache: ThumbnailCache) { self.thumbnails = cache; }
    pub fn update_output_thumbnail(&mut self, thumb: Option<Thumbnail>) {
        if let Some(t) = thumb { self.output_thumb = Some(t); }
    }
    pub fn update_workspace_cache(&mut self, cache: WorkspaceCache) {
        for (k, v) in cache {
            self.workspace_cache.insert(k, v);
            // If we navigated to a workspace just to capture it for the panel, open it now.
            if self.pending_preview_ws == Some(k) {
                self.pending_preview_ws = None;
                self.expanded_ws = Some(k);
            }
        }
    }

    pub fn next_page(&mut self) {
        if self.selected_ws + 1 < self.workspaces.len() { self.selected_ws += 1; self.kb_win = None; }
    }
    pub fn prev_page(&mut self) {
        if self.selected_ws > 0 { self.selected_ws -= 1; self.kb_win = None; }
    }

    /// Handle a key press. Returns `true` if the overlay should close.
    pub fn on_key(&mut self, keysym: u32) -> bool {
        const XKB_BACKSPACE: u32 = 0xff08;
        const XKB_TAB:       u32 = 0xff09;
        const XKB_RETURN:    u32 = 0xff0d;
        const XKB_ESCAPE:    u32 = 0xff1b;
        const XKB_SLASH:     u32 = 0x002f;
        const XKB_LEFT:      u32 = 0xff51;
        const XKB_UP:        u32 = 0xff52;
        const XKB_RIGHT:     u32 = 0xff53;
        const XKB_DOWN:      u32 = 0xff54;

        if self.search_active {
            // ── search mode ──────────────────────────────────────────────────
            match keysym {
                XKB_ESCAPE => {
                    if self.search_query.is_empty() {
                        self.search_active = false;
                    } else {
                        self.search_query.clear();
                    }
                    self.kb_win = None;
                }
                XKB_RETURN => {
                    if let Some(idx) = self.kb_win {
                        if let Some(card) = self.cards.get(idx) {
                            let _ = self.action_tx.try_send(WindowAction::Focus(card.window_id.clone()));
                        }
                    }
                    self.kb_win        = None;
                    self.search_active = false;
                    self.search_query.clear();
                    return true; // close overlay
                }
                XKB_BACKSPACE => {
                    self.search_query.pop();
                    self.kb_win = None;
                }
                XKB_UP | XKB_LEFT => {
                    let count = self.cards.len();
                    if count > 0 {
                        self.kb_win = Some(match self.kb_win {
                            None    => count.saturating_sub(1),
                            Some(0) => count - 1,
                            Some(i) => i - 1,
                        });
                    }
                }
                XKB_DOWN | XKB_RIGHT | XKB_TAB => {
                    let count = self.cards.len();
                    if count > 0 {
                        self.kb_win = Some(match self.kb_win {
                            None    => 0,
                            Some(i) => (i + 1) % count,
                        });
                    }
                }
                k @ 0x0020..=0x007e => {
                    if let Some(ch) = char::from_u32(k) {
                        self.search_query.push(ch);
                        self.kb_win = None;
                    }
                }
                _ => {} // ignore F-keys, modifiers, etc.
            }
            return false;
        }

        // ── normal mode ──────────────────────────────────────────────────────
        let card_count = self.cards.len();
        match keysym {
            XKB_SLASH => {
                self.search_active = true;
                self.search_query.clear();
                self.kb_win = None;
            }
            XKB_ESCAPE => {
                self.kb_win = None;
                return true;
            }
            XKB_RETURN => {
                if let Some(idx) = self.kb_win {
                    if let Some(card) = self.cards.get(idx) {
                        let _ = self.action_tx.try_send(WindowAction::Focus(card.window_id.clone()));
                    }
                }
                self.kb_win = None;
                return true;
            }
            XKB_TAB | XKB_RIGHT | XKB_DOWN => {
                if card_count > 0 {
                    self.kb_win = Some(match self.kb_win {
                        None    => 0,
                        Some(i) => (i + 1) % card_count,
                    });
                }
            }
            XKB_LEFT | XKB_UP => {
                if card_count > 0 {
                    self.kb_win = Some(match self.kb_win {
                        None    => card_count.saturating_sub(1),
                        Some(0) => card_count - 1,
                        Some(i) => i - 1,
                    });
                }
            }
            k @ 0x31..=0x39 => {
                let idx = (k - 0x31) as usize;
                if idx < self.workspaces.len() {
                    self.selected_ws = idx;
                    self.kb_win = None;
                }
            }
            _ => return true, // any other key closes
        }
        false
    }
    pub fn on_scroll(&mut self, _sx: f64, _dy: f64) {}
    pub fn on_motion(&mut self, x: f64, y: f64) { self.mouse_x = x as f32; self.mouse_y = y as f32; }

    pub fn on_press(&mut self, x: f64, y: f64) -> bool {
        let (mx, my) = (x as f32, y as f32);
        for btn in &self.buttons.clone() {
            if btn.hit(mx, my) {
                match &btn.action {
                    WindowAction::CloseOverlay => { return true; }
                    WindowAction::ClosePanel   => { self.expanded_ws = None; return false; }
                    WindowAction::PreviewWorkspace(ws_id) => {
                        let ws_id = *ws_id;
                        if self.workspace_cache.contains_key(&ws_id) {
                            self.expanded_ws = Some(ws_id);
                        } else {
                            // Navigate to it first; capture will auto-open the panel.
                            self.pending_preview_ws = Some(ws_id);
                            let _ = self.action_tx.try_send(WindowAction::FocusWorkspace(ws_id));
                        }
                        return false;
                    }
                    _ => { let _ = self.action_tx.try_send(btn.action.clone()); return true; }
                }
            }
        }
        for ws in &self.ws_thumbs.clone() {
            if ws.hit(mx, my) { self.selected_ws = ws.ws_idx; return false; }
        }
        // Launcher zones — spawn process and close overlay.
        for zone in &self.launcher_zones.clone() {
            if zone.hit(mx, my) {
                let cmd = zone.cmd.clone();
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if let Some((prog, args)) = parts.split_first() {
                    let _ = std::process::Command::new(prog).args(args).spawn();
                }
                return true;
            }
        }
        // Don't let card clicks pass through when the preview panel is covering the main view.
        if self.expanded_ws.is_none() {
            for card in &self.cards.clone() {
                if card.hit(mx, my) {
                    let _ = self.action_tx.try_send(WindowAction::Focus(card.window_id.clone()));
                    return true;
                }
            }
        }
        false
    }
    pub fn on_release(&mut self, _x: f64, _y: f64) {}

    // ── paint ─────────────────────────────────────────────────────────────────

    pub fn paint(&mut self, width: u32, height: u32, anim_t: f32) -> Vec<u8> {
        let needs_alloc = self.pixmap.as_ref()
        .map(|p| p.width() != width || p.height() != height).unwrap_or(true);
        if needs_alloc { self.pixmap = Pixmap::new(width, height); }
        let mut pm = match self.pixmap.take() {
            Some(mut p) => { p.fill(tiny_skia::Color::TRANSPARENT); p }
            None => {
                warn!("can't alloc {}x{}", width, height);
                return vec![0u8; (width * height * 4) as usize];
            }
        };
        if self.sys_last.elapsed() >= std::time::Duration::from_secs(3) {
            self.sys = SysInfo::collect();
            self.sys_last = std::time::Instant::now();
        }

        let sw = width as f32; let sh = height as f32;
        let theme = self.theme.clone();

        // Backdrop: dimmed workspace screenshot or solid fill
        if let Some((tw, th, ref px)) = self.output_thumb.clone() {
            draw_thumbnail_dimmed(&mut pm, px, tw, th, 0.0, 0.0, sw, sh, 0.40);
        } else {
            pm.fill(parse_color(&theme.background, theme.opacity));
        }

        self.anim_slide = (1.0 - anim_t) * -20.0;
        self.buttons.clear(); self.cards.clear(); self.ws_thumbs.clear();

        self.draw_top_bar(&mut pm, sw, sh);
        self.draw_ws_strip(&mut pm, sw, sh, &theme);
        if self.search_active {
            let theme2 = theme.clone();
            self.draw_search_results(&mut pm, sw, sh, &theme2);
        } else {
            self.draw_main_view(&mut pm, sw, sh, &theme, anim_t);
        }
        self.draw_widget_bar(&mut pm, sw, sh, &theme);

        // ── panel animation ───────────────────────────────────────────────
        let panel_target = if self.expanded_ws.is_some() { 1.0f32 } else { 0.0 };
        if self.panel_anim < panel_target {
            self.panel_anim = (self.panel_anim + 0.10).min(1.0);
        } else if self.panel_anim > panel_target {
            self.panel_anim = (self.panel_anim - 0.14).max(0.0);
        }
        if self.panel_anim > 0.01 {
            let theme2 = theme.clone();
            self.draw_ws_panel(&mut pm, sw, sh, &theme2);
        }

        // ── hover popup (top z-order) ─────────────────────────────────────
        if let Some(idx) = self.hovered_ws_idx {
            let theme2 = theme.clone();
            self.draw_hover_popup(&mut pm, sw, sh, idx, &theme2);
        }

        // ── error toast (always top z-order) ─────────────────────────────
        let toast_expired = if let Some((ref msg, shown_at, dur_ms)) = self.toast {
            if shown_at.elapsed().as_millis() as u32 >= dur_ms {
                true
            } else {
                self.draw_toast(&mut pm, sw, sh, msg.clone(), &theme);
                false
            }
        } else { false };
        if toast_expired { self.toast = None; }

        if anim_t < 0.999 {
            let a_scale = (anim_t * 255.0) as u32;
            for px in pm.pixels_mut() {
                let new_a = ((px.alpha() as u32 * a_scale) / 255) as u8;
                let r = ((px.red()   as u32 * a_scale) / 255) as u8;
                let g = ((px.green() as u32 * a_scale) / 255) as u8;
                let b = ((px.blue()  as u32 * a_scale) / 255) as u8;
                *px = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, new_a).unwrap_or(*px);
            }
        }
        let result = rgba_to_argb(pm.data());
        self.pixmap = Some(pm);
        result
    }

    // ── search ────────────────────────────────────────────────────────────────

    /// Collect windows matching the current query across all workspaces.
    /// Returns owned data to avoid borrow conflicts when calling text.draw in the loop.
    /// Empty query = return all windows.
    fn search_results(&self) -> Vec<(woven_common::types::Window, String, u32)> {
        let q = self.search_query.to_lowercase();
        self.workspaces.iter()
            .flat_map(|ws| ws.windows.iter().map(move |w| (w, ws)))
            .filter(|(w, _)| {
                q.is_empty()
                    || w.class.to_lowercase().contains(&q)
                    || w.title.to_lowercase().contains(&q)
            })
            .map(|(w, ws)| (w.clone(), ws.name.clone(), ws.id))
            .collect()
    }

    fn draw_search_box(&mut self, pm: &mut Pixmap, sw: f32, y: f32, theme: &Theme) {
        #[allow(non_snake_case)] let OUTER_PAD = self.layout.outer_padding;
        let x = OUTER_PAD;
        let w = sw - OUTER_PAD * 2.0;
        let h = 40.0_f32;
        let r = (theme.border_radius as f32).min(h / 2.0);
        // Border
        fill_rrect(pm, x - 1.5, y - 1.5, w + 3.0, h + 3.0, r + 1.5,
                   parse_color(&theme.accent, 0.85));
        // Background
        fill_rrect(pm, x, y, w, h, r, parse_color(&theme.background, 0.92));
        // Text content
        let fs  = 15.0_f32;
        let cy  = y + h / 2.0 - fs / 2.0;
        let pad = 14.0_f32;
        let prefix = "/ ";
        let pw = self.text.draw(pm, prefix, x + pad, cy, fs,
                                parse_color(&theme.accent, 0.60));
        if self.search_query.is_empty() {
            self.text.draw(pm, "type to search windows...", x + pad + pw, cy, fs,
                           parse_color(&theme.text, 0.22));
        } else {
            let display = format!("{}_", self.search_query);
            self.text.draw(pm, &display, x + pad + pw, cy, fs,
                           parse_color(&theme.text, 1.0));
        }
    }

    fn draw_search_results(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        #[allow(non_snake_case)] let TOP_H      = self.layout.top_bar_height;
        #[allow(non_snake_case)] let WS_STRIP_H = self.layout.ws_strip_height;
        #[allow(non_snake_case)] let WIDGET_H   = self.layout.widget_bar_height;
        #[allow(non_snake_case)] let OUTER_PAD  = self.layout.outer_padding;
        let slide   = self.anim_slide;
        let view_y  = TOP_H + WS_STRIP_H + slide;
        let view_h  = (sh - TOP_H - WS_STRIP_H - WIDGET_H).max(20.0);
        let box_h   = 40.0_f32;
        let box_y   = view_y + 12.0;

        self.draw_search_box(pm, sw, box_y, theme);

        let results_y = box_y + box_h + 10.0;
        let results_h = view_h - box_h - 22.0;
        let row_h     = 52.0_f32;
        let row_gap   = 6.0_f32;
        let row_x     = OUTER_PAD;
        let row_w     = sw - OUTER_PAD * 2.0;

        let results = self.search_results();

        if results.is_empty() && !self.search_query.is_empty() {
            let msg = "no matching windows";
            let mfs = 13.0_f32;
            let mw  = self.text.measure(msg, mfs);
            self.text.draw(pm, msg, sw / 2.0 - mw / 2.0,
                           results_y + results_h / 2.0 - mfs / 2.0,
                           mfs, parse_color(&theme.border, 0.35));
            return;
        }

        let mut new_cards: Vec<CardRect> = Vec::new();
        let mx = self.mouse_x;
        let my = self.mouse_y;

        for (i, (win, ws_name, ws_id)) in results.iter().enumerate() {
            let ry = results_y + i as f32 * (row_h + row_gap);
            if ry + row_h > results_y + results_h { break; }

            let hovered    = mx >= row_x && mx <= row_x + row_w
                          && my >= ry    && my <= ry + row_h;
            let kb_focused = self.kb_win == Some(new_cards.len());
            let r          = (theme.border_radius as f32).min(row_h / 2.0);

            // Focus ring
            if kb_focused {
                fill_rrect(pm, row_x - 2.0, ry - 2.0, row_w + 4.0, row_h + 4.0, r + 2.0,
                           parse_color(&theme.accent, 0.95));
            }

            // Card background
            fill_rrect(pm, row_x, ry, row_w, row_h, r,
                       parse_color(&theme.background, if hovered { 0.75 } else { 0.55 }));

            // Left accent strip
            fill_rrect(pm, row_x, ry, 3.0, row_h, 1.5, self.app_color(&win.class));

            // App class + title
            let text_x  = row_x + 14.0;
            let name_fs = 13.0_f32;
            let ttl_fs  = 11.0_f32;
            let name_y  = ry + row_h / 2.0 - name_fs - 1.0;
            let ttl_y   = ry + row_h / 2.0 + 2.0;
            self.text.draw(pm, if win.class.is_empty() { "unknown" } else { &win.class },
                           text_x, name_y, name_fs, parse_color(&theme.text, 1.0));
            self.text.draw(pm, &truncate(&win.title, 48),
                           text_x, ttl_y, ttl_fs, parse_color(&theme.text, 0.45));

            // Workspace badge (right side)
            let ws_label = if ws_name.is_empty() { format!("ws {}", ws_id) } else { ws_name.clone() };
            let badge_fs  = 10.0_f32;
            let badge_pad = 7.0_f32;
            let badge_w   = self.text.measure(&ws_label, badge_fs) + badge_pad * 2.0;
            let badge_h   = 18.0_f32;
            let badge_x   = row_x + row_w - badge_w - 10.0;
            let badge_y   = ry + row_h / 2.0 - badge_h / 2.0;
            fill_rrect(pm, badge_x, badge_y, badge_w, badge_h, badge_h / 2.0,
                       parse_color(&theme.accent, 0.18));
            self.text.draw(pm, &ws_label,
                           badge_x + badge_pad, badge_y + badge_h / 2.0 - badge_fs / 2.0,
                           badge_fs, parse_color(&theme.accent, 0.80));

            new_cards.push(CardRect { x: row_x, y: ry, w: row_w, h: row_h,
                                       window_id: win.id.clone() });
        }

        self.cards.extend(new_cards);
    }

    // ── top bar ───────────────────────────────────────────────────────────────

    fn draw_top_bar(&mut self, pm: &mut Pixmap, sw: f32, _sh: f32) {
        #[allow(non_snake_case)] let TOP_H = self.layout.top_bar_height;
        let theme = self.theme.clone(); let sys = self.sys.clone();
        let slide = self.anim_slide; let bar_h = TOP_H;
        fill_rect(pm, 0.0, slide, sw, bar_h, parse_color(&theme.background, 0.88));
        fill_rect(pm, 0.0, bar_h-1.0+slide, sw, 1.0, parse_color(&theme.border, 0.18));
        let (fs, sm) = (13.0f32, 11.0f32);
        let cy = bar_h/2.0+slide; let pad = 16.0f32;
        let accent = parse_color(&theme.accent, 1.0);
        let text_c = parse_color(&theme.text, 1.0);
        let dim_c  = parse_color(&theme.text, 0.45);
        let sep = "  ·  ";
        let mut cx = pad;
        cx += self.text.draw(pm, &sys.hostname, cx, cy-fs/2.0, fs, accent);
        cx += self.text.draw(pm, sep, cx, cy-sm/2.0, sm, dim_c);
        cx += self.text.draw(pm, &sys.distro, cx, cy-sm/2.0, sm, text_c);
        cx += self.text.draw(pm, sep, cx, cy-sm/2.0, sm, dim_c);
        let ks = sys.kernel.split('-').next().unwrap_or(&sys.kernel).to_string();
        cx += self.text.draw(pm, &ks, cx, cy-sm/2.0, sm, dim_c);
        cx += self.text.draw(pm, sep, cx, cy-sm/2.0, sm, dim_c);
        self.text.draw(pm, &format!("up {}", sys.uptime_str()), cx, cy-sm/2.0, sm, dim_c);
        // Close button
        let close_label = "  ✕  "; let close_fs = 14.0f32;
        let close_w = self.text.measure(close_label, close_fs) + 4.0;
        let close_h = bar_h*0.65;
        let close_x = sw - close_w - 8.0;
        let close_y = bar_h/2.0 - close_h/2.0 + slide;
        let hov = self.mouse_x >= close_x && self.mouse_x <= close_x+close_w
        && self.mouse_y >= close_y && self.mouse_y <= close_y+close_h;
        fill_rrect(pm, close_x, close_y, close_w, close_h, close_h/2.0,
                   Color::from_rgba8(243,139,168, if hov {60} else {25}));
        self.text.draw(pm, close_label, close_x, close_y+close_h/2.0-close_fs/2.0,
                       close_fs, Color::from_rgba8(243,139,168, if hov {255} else {180}));
        self.buttons.push(ButtonRect { x: close_x, y: close_y, w: close_w, h: close_h,
                                       action: WindowAction::CloseOverlay });
        // Stats
        let cpu_s = format!("cpu {:.0}%", sys.cpu_pct);
        let mem_s = format!("{:.1}G/{:.0}G", sys.mem_used_kb as f32/(1024.0*1024.0),
                            sys.mem_total_kb as f32/(1024.0*1024.0));
        let cpu_c = if sys.cpu_pct > 80.0 { Color::from_rgba8(243,139,168,255) }
        else if sys.cpu_pct > 50.0 { Color::from_rgba8(250,179,135,255) } else { accent };
        let mut rx = close_x - 12.0;
        let memw = self.text.measure(&mem_s, sm); rx -= memw;
        self.text.draw(pm, &mem_s, rx, cy-sm/2.0, sm, dim_c);
        let sepw = self.text.measure(sep, sm); rx -= sepw;
        self.text.draw(pm, sep, rx, cy-sm/2.0, sm, dim_c);
        let cpuw = self.text.measure(&cpu_s, sm); rx -= cpuw;
        self.text.draw(pm, &cpu_s, rx, cy-sm/2.0, sm, cpu_c);
    }

    // ── workspace strip ───────────────────────────────────────────────────────

    fn draw_ws_strip(&mut self, pm: &mut Pixmap, sw: f32, _sh: f32, theme: &Theme) {
        #[allow(non_snake_case)] let TOP_H      = self.layout.top_bar_height;
        #[allow(non_snake_case)] let WS_STRIP_H = self.layout.ws_strip_height;
        #[allow(non_snake_case)] let STRIP_GAP  = self.layout.strip_gap;
        #[allow(non_snake_case)] let WS_THUMB_W = self.layout.ws_thumb_width;
        #[allow(non_snake_case)] let WS_THUMB_H = self.layout.ws_thumb_height;
        #[allow(non_snake_case)] let WS_BTN_H   = self.layout.ws_btn_height;
        #[allow(non_snake_case)] let OUTER_PAD  = self.layout.outer_padding;
        let slide = self.anim_slide;
        let strip_y = TOP_H + slide; let strip_h = WS_STRIP_H;
        let thumb_y = strip_y + 8.0;
        fill_rect(pm, 0.0, strip_y, sw, strip_h, parse_color(&theme.background, 0.45));
        fill_rect(pm, 0.0, strip_y+strip_h-1.0, sw, 1.0, parse_color(&theme.border, 0.15));
        let workspaces = self.workspaces.clone();
        if workspaces.is_empty() {
            let msg = "no workspaces"; let mfs = 12.0f32;
            let mw = self.text.measure(msg, mfs);
            self.text.draw(pm, msg, sw/2.0-mw/2.0, strip_y+strip_h/2.0-mfs/2.0, mfs,
                           parse_color(&theme.border, 0.4));
            return;
        }
        let n = workspaces.len() as f32;
        let total_w = n*WS_THUMB_W + (n-1.0)*STRIP_GAP;
        let start_x = ((sw-total_w)/2.0).max(OUTER_PAD);
        let selected = self.selected_ws.min(workspaces.len().saturating_sub(1));
        let mx = self.mouse_x; let my = self.mouse_y;
        let mut new_ws_thumbs = Vec::new();
        for (i, ws) in workspaces.iter().enumerate() {
            let tx = start_x + i as f32 * (WS_THUMB_W + STRIP_GAP);
            let is_sel = i == selected; let is_active = ws.active;
            let hovered = mx >= tx && mx <= tx+WS_THUMB_W && my >= thumb_y && my <= thumb_y+WS_THUMB_H;
            new_ws_thumbs.push(WsThumbRect { x: tx, y: thumb_y, w: WS_THUMB_W, h: WS_THUMB_H, ws_idx: i });
            let border_col = if is_sel { parse_color(&theme.accent, 0.85) }
            else if is_active { parse_color(&theme.accent, 0.45) }
            else { parse_color(&theme.border, if hovered {0.45} else {0.22}) };
            let bw = if is_sel { 2.0f32 } else { 1.0 };
            let r = (theme.border_radius as f32 * 0.6).min(WS_THUMB_W/2.0).min(WS_THUMB_H/2.0);
            fill_rrect(pm, tx-bw, thumb_y-bw, WS_THUMB_W+bw*2.0, WS_THUMB_H+bw*2.0, r+bw, border_col);
            fill_rrect(pm, tx, thumb_y, WS_THUMB_W, WS_THUMB_H, r,
                       parse_color(&theme.background, 0.75));
            // Workspace screenshot if available
            let ws_shot = self.workspace_cache.get(&ws.id).cloned();
            if let Some((tw, th, ref px)) = ws_shot {
                draw_thumbnail_clipped(pm, px, tw, th, tx, thumb_y, WS_THUMB_W, WS_THUMB_H, r);
            } else {
                draw_ws_placeholder(pm, &mut self.text, tx, thumb_y, WS_THUMB_W, WS_THUMB_H, ws, theme);
            }
            if is_active {
                fill_circle(pm, tx+WS_THUMB_W-8.0, thumb_y+8.0, 4.0, parse_color(&theme.accent, 0.9));
            }
            // Label
            let label = if ws.name.is_empty() || ws.name == ws.id.to_string() {
                format!("{}", ws.id)
            } else { format!("{}  {}", ws.id, ws.name) };
            let lfs = 11.0f32; let lw = self.text.measure(&label, lfs);
            let ly = thumb_y + WS_THUMB_H + 4.0;
            self.text.draw(pm, &label, tx+WS_THUMB_W/2.0-lw/2.0, ly, lfs,
                           if is_sel { parse_color(&theme.accent, 1.0) }
                           else { parse_color(&theme.text, 0.55) });
            // Expand / preview button — top-right corner of each thumbnail
            {
                let exp_w = 34.0f32; let exp_h = 16.0f32;
                let exp_x = tx + WS_THUMB_W - exp_w - 4.0;
                let exp_y = thumb_y + 4.0;
                let exp_hov = mx >= exp_x && mx <= exp_x+exp_w
                    && my >= exp_y && my <= exp_y+exp_h;
                fill_rrect(pm, exp_x, exp_y, exp_w, exp_h, exp_h/2.0,
                           parse_color(&theme.background, if exp_hov {0.95} else {0.65}));
                let exp_label = "view"; let exp_fs = 9.0f32;
                let elw = self.text.measure(exp_label, exp_fs);
                self.text.draw(pm, exp_label,
                               exp_x+exp_w/2.0-elw/2.0, exp_y+exp_h/2.0-exp_fs/2.0,
                               exp_fs, parse_color(&theme.text, if exp_hov {1.0} else {0.70}));
                self.buttons.push(ButtonRect {
                    x: exp_x, y: exp_y, w: exp_w, h: exp_h,
                    action: WindowAction::PreviewWorkspace(ws.id),
                });
            }

            // Go-to button
            if is_sel {
                let btn_label = "go to workspace"; let btn_fs = 10.0f32;
                let btn_w = self.text.measure(btn_label, btn_fs) + 14.0;
                let btn_h = WS_BTN_H;
                let btn_x = tx + WS_THUMB_W/2.0 - btn_w/2.0;
                let btn_y2 = ly + lfs + 3.0;
                let btn_hov = mx >= btn_x && mx <= btn_x+btn_w && my >= btn_y2 && my <= btn_y2+btn_h;
                fill_rrect(pm, btn_x, btn_y2, btn_w, btn_h, btn_h/2.0,
                           parse_color(&theme.accent, if btn_hov {0.30} else {0.15}));
                let tlw = self.text.measure(btn_label, btn_fs);
                self.text.draw(pm, btn_label, btn_x+btn_w/2.0-tlw/2.0,
                               btn_y2+btn_h/2.0-btn_fs/2.0, btn_fs,
                               parse_color(&theme.accent, if btn_hov {1.0} else {0.80}));
                self.buttons.push(ButtonRect { x: btn_x, y: btn_y2, w: btn_w, h: btn_h,
                                              action: WindowAction::FocusWorkspace(ws.id) });
            }
        }
        self.ws_thumbs = new_ws_thumbs;
        // Update hover index for popup rendering (used after this method returns).
        self.hovered_ws_idx = self.ws_thumbs.iter().position(|wt| {
            self.mouse_x >= wt.x && self.mouse_x <= wt.x+wt.w &&
            self.mouse_y >= wt.y && self.mouse_y <= wt.y+wt.h
        });
    }

    // ── main view ─────────────────────────────────────────────────────────────

    fn draw_main_view(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme, anim_t: f32) {
        #[allow(non_snake_case)] let TOP_H            = self.layout.top_bar_height;
        #[allow(non_snake_case)] let WS_STRIP_H       = self.layout.ws_strip_height;
        #[allow(non_snake_case)] let WIDGET_H         = self.layout.widget_bar_height;
        #[allow(non_snake_case)] let CARD_PAD         = self.layout.card_padding;
        #[allow(non_snake_case)] let CARD_GAP         = self.layout.card_gap;
        #[allow(non_snake_case)] let CARD_THUMB_RATIO = self.layout.card_thumb_ratio;
        let slide  = self.anim_slide;
        let view_y = TOP_H + WS_STRIP_H + slide;
        let view_h = (sh - TOP_H - WS_STRIP_H - WIDGET_H).max(20.0);
        let view_w = sw;
        let workspaces = self.workspaces.clone();
        let selected   = self.selected_ws.min(workspaces.len().saturating_sub(1));
        let ws         = workspaces.get(selected);
        let windows: Vec<_> = ws.map(|w| w.windows.clone()).unwrap_or_default();

        if windows.is_empty() {
            let msg = if ws.is_some() { "no windows" } else { "no workspace" };
            let mfs = 13.0f32; let mw = self.text.measure(msg, mfs);
            self.text.draw(pm, msg, sw/2.0-mw/2.0, view_y+view_h/2.0-mfs/2.0, mfs,
                           parse_color(&theme.border, 0.35));
            return;
        }

        // Use zoom layout when the compositor has given us real window positions.
        // Threshold: at least one window with w ≥ 100 && h ≥ 100.
        let has_geom = windows.iter().any(|w| w.geometry.w >= 100 && w.geometry.h >= 100);
        if has_geom {
            self.draw_main_view_zoom(pm, sw, sh, theme, anim_t, view_y, view_h, view_w, ws.map(|w| w.id), &windows);
            return;
        }

        // ── grid fallback (no compositor geometry) ────────────────────────────
        let cols = if sw >= 1800.0 {4} else if sw >= 1200.0 {3} else {2};
        let rows = windows.len().div_ceil(cols);
        let grid_w = sw - CARD_PAD*2.0; let grid_h = view_h - CARD_PAD*2.0;
        let card_w = (grid_w - CARD_GAP*(cols as f32-1.0)) / cols as f32;
        let card_h = if rows > 0 { ((grid_h - CARD_GAP*(rows as f32-1.0)) / rows as f32).min(300.0) } else { 0.0 };
        if card_w < 10.0 || card_h < 10.0 { return; }
        let mut new_buttons: Vec<ButtonRect> = Vec::new();
        let mut new_cards:   Vec<CardRect>   = Vec::new();
        let mx = self.mouse_x; let my = self.mouse_y;
        for (i, win) in windows.iter().enumerate() {
            let col = i % cols; let row = i / cols;
            let cx  = CARD_PAD + col as f32*(card_w+CARD_GAP);
            let cy  = view_y + CARD_PAD + row as f32*(card_h+CARD_GAP);
            let r   = (theme.border_radius as f32).min(card_w/2.0).min(card_h/2.0);
            let hovered   = mx >= cx && mx <= cx+card_w && my >= cy && my <= cy+card_h;
            let kb_focused = self.kb_win == Some(new_cards.len());
            let border_col = if kb_focused { parse_color(&theme.accent, 0.95) }
                             else { parse_color(&theme.border, if hovered {0.45} else {0.20}) };
            let inset = if kb_focused { 2.0f32 } else { 1.0f32 };
            fill_rrect(pm, cx-inset, cy-inset, card_w+inset*2.0, card_h+inset*2.0, r+inset, border_col);
            fill_rrect(pm, cx, cy, card_w, card_h, r,
                       parse_color(&theme.background, if hovered {0.88} else {0.72}));
            new_cards.push(CardRect { x: cx, y: cy, w: card_w, h: card_h, window_id: win.id.clone() });
            let thumb_h = card_h * CARD_THUMB_RATIO;
            let info_h  = card_h - thumb_h;
            if let Some((tw, th, ref rgba)) = self.thumbnails.get(&win.id).cloned() {
                draw_thumbnail_clipped(pm, rgba, tw, th, cx, cy, card_w, thumb_h, r);
            } else {
                fill_rrect(pm, cx, cy, card_w, thumb_h, r,
                           with_alpha(self.app_color(&win.class), 0.10));
                self.draw_app_icon(pm, &win.class, cx, cy, card_w, thumb_h);
            }
            if hovered {
                fill_rrect(pm, cx, cy, card_w, thumb_h, r, parse_color(&theme.background, 0.55));
                let btn_h_ = (thumb_h*0.22).clamp(16.0, 26.0);
                let btn_fs = (btn_h_*0.44).clamp(8.0, 11.0);
                let btn_pad = 7.0f32; let btn_gap = 4.0f32;
                let id = win.id.clone();
                let btns: &[(&str, WindowAction, [u8;4])] = &[
                    ("focus",  WindowAction::Focus(id.clone()),            [166,227,161,255]),
                    ("float",  WindowAction::ToggleFloat(id.clone()),      [137,180,250,255]),
                    ("pin",    WindowAction::TogglePin(id.clone()),        [203,166,247,255]),
                    ("fs",     WindowAction::ToggleFullscreen(id.clone()), [250,179,135,255]),
                    ("✕",     WindowAction::Close(id.clone()),             [243,139,168,255]),
                ];
                let mut bx = cx+6.0; let mut bry = cy+4.0;
                for (label, action, rgba) in btns {
                    let lw = self.text.measure(label, btn_fs); let bw = lw+btn_pad*2.0;
                    if bx+bw > cx+card_w-4.0 { bx = cx+6.0; bry += btn_h_+btn_gap; }
                    if bry+btn_h_ > cy+thumb_h { break; }
                    fill_rrect(pm, bx, bry, bw, btn_h_, btn_h_/2.0,
                               Color::from_rgba8(rgba[0],rgba[1],rgba[2],50));
                    self.text.draw(pm, label, bx+btn_pad, bry+btn_h_/2.0-btn_fs/2.0, btn_fs,
                                   Color::from_rgba8(rgba[0],rgba[1],rgba[2],230));
                    new_buttons.push(ButtonRect { x: bx, y: bry, w: bw, h: btn_h_, action: action.clone() });
                    bx += bw + btn_gap;
                }
            }
            let info_y = cy + thumb_h + 4.0; let text_pad = 10.0f32;
            let cls_col = self.app_color(&win.class);
            let name_fs = (info_h*0.32).clamp(9.0, 13.0);
            let title_fs = (name_fs*0.82).clamp(8.0, 11.0);
            fill_rrect(pm, cx, cy+thumb_h+1.0, 3.0, card_h-thumb_h-1.0, 1.5, cls_col);
            self.text.draw(pm, if win.class.is_empty() {"unknown"} else {&win.class},
                           cx+text_pad, info_y, name_fs, parse_color(&theme.text, 1.0));
            self.text.draw(pm, &truncate(&win.title, 32), cx+text_pad, info_y+name_fs+2.0,
                           title_fs, parse_color(&theme.text, 0.45));
        }
        self.buttons.extend(new_buttons);
        self.cards.extend(new_cards);
    }

    // ── zoom overview (real compositor window positions) ──────────────────────

    #[allow(clippy::too_many_arguments)]
    fn draw_main_view_zoom(
        &mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme,
        anim_t: f32, view_y: f32, view_h: f32, view_w: f32,
        ws_id: Option<u32>, windows: &[woven_common::types::Window],
    ) {
        // Smoothstep the animation (0 = just opened, 1 = fully settled).
        let t = { let x = anim_t; x * x * (3.0 - 2.0 * x) };

        // Scale the full output (sw × sh) to fit view_w × view_h with padding.
        // At t=0 the canvas is slightly larger (feels like zooming out from the desktop).
        let base_zoom  = ((view_w / sw) * 0.87).min((view_h / sh) * 0.87);
        let zoom       = base_zoom * (1.0 + 0.10 * (1.0 - t)); // shrinks as it settles
        let canvas_w   = sw * zoom;
        let canvas_h   = sh * zoom;
        let canvas_x   = (view_w - canvas_w) / 2.0;
        let canvas_y   = view_y + (view_h - canvas_h) / 2.0;

        // Desktop backdrop: workspace screenshot or dim fill.
        fill_rrect(pm, canvas_x - 2.0, canvas_y - 2.0, canvas_w + 4.0, canvas_h + 4.0, 8.0,
                   parse_color(&theme.border, 0.22));
        if let Some(wid) = ws_id {
            if let Some((tw, th, ref px)) = self.workspace_cache.get(&wid).cloned() {
                draw_thumbnail_clipped(pm, px, tw, th, canvas_x, canvas_y, canvas_w, canvas_h, 6.0);
            } else {
                fill_rrect(pm, canvas_x, canvas_y, canvas_w, canvas_h, 6.0,
                           parse_color(&theme.background, 0.42));
            }
        }

        let mx = self.mouse_x; let my = self.mouse_y;
        let mut new_buttons: Vec<ButtonRect> = Vec::new();
        let mut new_cards:   Vec<CardRect>   = Vec::new();

        for win in windows {
            let g = &win.geometry;
            if g.w == 0 || g.h == 0 { continue; }

            // Map compositor coordinates → canvas coordinates.
            let wx = canvas_x + g.x as f32 * zoom;
            let wy = canvas_y + g.y as f32 * zoom;
            let ww = (g.w as f32 * zoom).max(32.0);
            let wh = (g.h as f32 * zoom).max(24.0);
            // Clip to canvas bounds.
            let wx = wx.max(canvas_x);
            let wy = wy.max(canvas_y);
            let ww = ww.min(canvas_x + canvas_w - wx);
            let wh = wh.min(canvas_y + canvas_h - wy);
            if ww < 4.0 || wh < 4.0 { continue; }

            let r   = 4.0f32.min(ww / 4.0).min(wh / 4.0);
            let hov        = mx >= wx && mx <= wx + ww && my >= wy && my <= wy + wh;
            let kb_focused = self.kb_win == Some(new_cards.len());
            let border_col = if kb_focused { parse_color(&theme.accent, 0.95) }
                             else { parse_color(&theme.border, if hov { 0.65 } else { 0.30 }) };
            let inset = if kb_focused { 2.5f32 } else { 1.5f32 };

            // Window border + fill.
            fill_rrect(pm, wx - inset, wy - inset, ww + inset*2.0, wh + inset*2.0, r + inset, border_col);
            fill_rrect(pm, wx, wy, ww, wh, r, parse_color(&theme.background, 0.92));

            // Screencopy thumbnail (full card; title bar drawn on top).
            let thumb_h = if wh > 30.0 { wh - 16.0 } else { wh };
            if let Some((tw, th, ref rgba)) = self.thumbnails.get(&win.id).cloned() {
                draw_thumbnail_clipped(pm, rgba, tw, th, wx, wy, ww, thumb_h, r);
            } else {
                fill_rrect(pm, wx, wy, ww, thumb_h, r,
                           with_alpha(self.app_color(&win.class), 0.10));
                if ww >= 50.0 && thumb_h >= 40.0 {
                    self.draw_app_icon(pm, &win.class, wx, wy, ww, thumb_h);
                }
            }

            // Title footer — class name + color stripe.
            if wh > 26.0 {
                let lbl_y = wy + wh - 16.0;
                fill_rect(pm, wx, lbl_y, ww, 16.0, parse_color(&theme.background, 0.78));
                fill_rect(pm, wx, lbl_y, 2.5, 16.0, self.app_color(&win.class));
                let lfs = 8.0f32;
                let cls  = if win.class.is_empty() { "unknown" } else { &win.class };
                self.text.draw(pm, &truncate(cls, 22), wx + 6.0, lbl_y + 8.0 - lfs / 2.0,
                               lfs, parse_color(&theme.text, 0.82));
            }

            // Hover: scrim + action buttons.
            if hov {
                fill_rrect(pm, wx, wy, ww, thumb_h, r, parse_color(&theme.background, 0.52));

                // Focus (center)
                if ww >= 44.0 && thumb_h >= 18.0 {
                    let label = "focus"; let fs = 8.5f32;
                    let bw = self.text.measure(label, fs) + 10.0; let bh = 16.0f32;
                    let bx = wx + ww / 2.0 - bw / 2.0;
                    let by = wy + thumb_h / 2.0 - bh / 2.0;
                    fill_rrect(pm, bx, by, bw, bh, bh / 2.0, parse_color(&theme.accent, 0.30));
                    let lw = self.text.measure(label, fs);
                    self.text.draw(pm, label, bx + bw/2.0 - lw/2.0, by + bh/2.0 - fs/2.0,
                                   fs, parse_color(&theme.accent, 1.0));
                    new_buttons.push(ButtonRect { x: bx, y: by, w: bw, h: bh,
                                                  action: WindowAction::Focus(win.id.clone()) });
                }

                // Close (top-right corner of card)
                if ww >= 26.0 {
                    let cbx = wx + ww - 16.0; let cby = wy + 3.0;
                    fill_rrect(pm, cbx, cby, 13.0, 13.0, 6.0, Color::from_rgba8(243,139,168,55));
                    let xfs = 7.5f32;
                    let xw = self.text.measure("✕", xfs);
                    self.text.draw(pm, "✕", cbx + 6.5 - xw/2.0, cby + 6.5 - xfs/2.0,
                                   xfs, Color::from_rgba8(243,139,168,215));
                    new_buttons.push(ButtonRect { x: cbx, y: cby, w: 13.0, h: 13.0,
                                                  action: WindowAction::Close(win.id.clone()) });
                }
            }

            new_cards.push(CardRect { x: wx, y: wy, w: ww, h: wh, window_id: win.id.clone() });
        }

        self.buttons.extend(new_buttons);
        self.cards.extend(new_cards);
    }

    // ── hover popup ───────────────────────────────────────────────────────────

    fn draw_hover_popup(&mut self, pm: &mut Pixmap, sw: f32, _sh: f32, ws_idx: usize, theme: &Theme) {
        let ws_rect = match self.ws_thumbs.get(ws_idx) { Some(r) => r.clone(), None => return };
        let workspaces = self.workspaces.clone();
        let ws = match workspaces.get(ws_idx) { Some(w) => w, None => return };
        let shot = self.workspace_cache.get(&ws.id).cloned();

        let pop_w = 800.0f32; let pop_h = 450.0f32; let r = 10.0f32;
        // Position: below the thumbnail strip, centered on hovered card.
        let px = (ws_rect.x + ws_rect.w/2.0 - pop_w/2.0).max(4.0).min(sw - pop_w - 4.0);
        let py = ws_rect.y + ws_rect.h + 14.0;

        // Drop shadow
        fill_rrect(pm, px+3.0, py+4.0, pop_w, pop_h, r, Color::from_rgba8(0,0,0,70));
        // Border
        fill_rrect(pm, px-1.5, py-1.5, pop_w+3.0, pop_h+3.0, r+1.5,
                   parse_color(&theme.border, 0.45));
        // Background
        fill_rrect(pm, px, py, pop_w, pop_h, r, parse_color(&theme.background, 0.96));

        // Screenshot or placeholder
        if let Some((tw, th, ref pxs)) = shot {
            draw_thumbnail_clipped(pm, pxs, tw, th, px, py, pop_w, pop_h, r);
        } else {
            draw_ws_placeholder(pm, &mut self.text, px, py, pop_w, pop_h, ws, theme);
        }

        // Label bar at bottom
        fill_rrect(pm, px, py+pop_h-22.0, pop_w, 22.0, 0.0,
                   parse_color(&theme.background, 0.75));
        let label = if ws.name.is_empty() || ws.name == ws.id.to_string() {
            format!("workspace {}", ws.id)
        } else { format!("workspace {}  —  {}", ws.id, ws.name) };
        let lfs = 11.0f32; let lw = self.text.measure(&label, lfs);
        self.text.draw(pm, &label, px+pop_w/2.0-lw/2.0, py+pop_h-22.0+11.0/2.0-lfs/2.0+2.0,
                       lfs, parse_color(&theme.text, 0.85));
    }

    // ── workspace preview panel ───────────────────────────────────────────────

    fn draw_ws_panel(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        #[allow(non_snake_case)] let TOP_H      = self.layout.top_bar_height;
        #[allow(non_snake_case)] let WS_STRIP_H = self.layout.ws_strip_height;
        #[allow(non_snake_case)] let WIDGET_H   = self.layout.widget_bar_height;
        let t = self.panel_anim;
        let slide = self.anim_slide;
        let panel_y = TOP_H + WS_STRIP_H + slide;
        let panel_h = (sh - TOP_H - WS_STRIP_H - WIDGET_H).max(20.0);

        let ws_id = match self.expanded_ws { Some(id) => id, None => return };
        let workspaces = self.workspaces.clone();
        let ws = match workspaces.iter().find(|w| w.id == ws_id) { Some(w) => w.clone(), None => return };
        let shot = self.workspace_cache.get(&ws_id).cloned();

        // Panel background
        fill_rect(pm, 0.0, panel_y, sw, panel_h,
                  parse_color(&theme.background, 0.94 * t));
        fill_rect(pm, 0.0, panel_y, sw, 1.0, parse_color(&theme.border, 0.30 * t));

        let pad = 24.0f32;
        let hdr_h = 40.0f32;

        // Header: workspace name + window count
        let label = if ws.name.is_empty() || ws.name == ws_id.to_string() {
            format!("workspace {}", ws_id)
        } else { format!("workspace {}  —  {}", ws_id, ws.name) };
        let lfs = 15.0f32;
        self.text.draw(pm, &label, pad, panel_y + hdr_h/2.0 - lfs/2.0 - 2.0,
                       lfs, with_alpha(parse_color(&theme.accent, 1.0), t));

        let cnt = format!("{} window{}", ws.windows.len(), if ws.windows.len()==1{""} else {"s"});
        let cfs = 10.0f32;
        let cnt_y = panel_y + hdr_h/2.0 - cfs/2.0 + 10.0;
        self.text.draw(pm, &cnt, pad, cnt_y, cfs,
                       with_alpha(parse_color(&theme.text, 0.45), t));

        // Close-panel button (added to front of buttons so it has priority)
        let close_label = "close"; let close_fs = 10.0f32;
        let close_w = self.text.measure(close_label, close_fs) + 18.0;
        let close_h = 22.0f32;
        let close_x = sw - close_w - pad;
        let close_y = panel_y + hdr_h/2.0 - close_h/2.0;
        let close_hov = self.mouse_x >= close_x && self.mouse_x <= close_x+close_w
            && self.mouse_y >= close_y && self.mouse_y <= close_y+close_h;
        fill_rrect(pm, close_x, close_y, close_w, close_h, close_h/2.0,
                   with_alpha(parse_color(&theme.border, if close_hov {0.55} else {0.28}), t));
        let clw = self.text.measure(close_label, close_fs);
        self.text.draw(pm, close_label, close_x+close_w/2.0-clw/2.0,
                       close_y+close_h/2.0-close_fs/2.0, close_fs,
                       with_alpha(parse_color(&theme.text, if close_hov {1.0} else {0.65}), t));
        // Insert at front so it's checked before window cards.
        self.buttons.insert(0, ButtonRect {
            x: close_x, y: close_y, w: close_w, h: close_h,
            action: WindowAction::ClosePanel,
        });

        // Screenshot area
        let img_y = panel_y + hdr_h + 8.0;
        let img_h = panel_h - hdr_h - 8.0 - 12.0;
        let img_w = sw - pad*2.0;
        if img_w > 10.0 && img_h > 10.0 {
            let r = 8.0f32;
            fill_rrect(pm, pad-2.0, img_y-2.0, img_w+4.0, img_h+4.0, r+2.0,
                       with_alpha(parse_color(&theme.border, 0.35), t));
            if let Some((tw, th, ref pxs)) = shot {
                draw_thumbnail_clipped(pm, pxs, tw, th, pad, img_y, img_w, img_h, r);
            } else {
                fill_rrect(pm, pad, img_y, img_w, img_h, r,
                           parse_color(&theme.background, 0.5 * t));
                let msg = if self.pending_preview_ws == Some(ws_id) {
                    "navigating to workspace…"
                } else {
                    "visit this workspace once to capture a screenshot"
                };
                let mfs = 12.0f32; let mw = self.text.measure(msg, mfs);
                self.text.draw(pm, msg, pad+img_w/2.0-mw/2.0, img_y+img_h/2.0-mfs/2.0,
                               mfs, with_alpha(parse_color(&theme.text, 0.40), t));
            }
        }
    }

    // ── persistent bar ────────────────────────────────────────────────────────

    pub fn paint_bar(&mut self, width: u32, height: u32, position: &BarPosition) -> Vec<u8> {
        let needs_alloc = self.bar_pixmap.as_ref()
            .map(|p| p.width() != width || p.height() != height).unwrap_or(true);
        if needs_alloc { self.bar_pixmap = Pixmap::new(width, height); }
        let mut pm = match self.bar_pixmap.take() {
            Some(mut p) => { p.fill(tiny_skia::Color::TRANSPARENT); p }
            None => return vec![0u8; (width * height * 4) as usize],
        };

        // Time-based sys refresh (shared with overlay paint()).
        if self.sys_last.elapsed() >= std::time::Duration::from_secs(3) {
            self.sys = SysInfo::collect();
            self.sys_last = std::time::Instant::now();
        }

        let sw = width as f32; let sh = height as f32;
        let theme = self.theme.clone();
        let is_vertical = matches!(position, BarPosition::Left | BarPosition::Right);

        // Common background + inward border.
        fill_rect(&mut pm, 0.0, 0.0, sw, sh, parse_color(&theme.background, 0.93));
        match position {
            BarPosition::Right  => fill_rect(&mut pm, 0.0, 0.0, 1.0, sh, parse_color(&theme.border, 0.22)),
            BarPosition::Left   => fill_rect(&mut pm, sw-1.0, 0.0, 1.0, sh, parse_color(&theme.border, 0.22)),
            BarPosition::Top    => fill_rect(&mut pm, 0.0, sh-1.0, sw, 1.0, parse_color(&theme.border, 0.22)),
            BarPosition::Bottom => fill_rect(&mut pm, 0.0, 0.0, sw, 1.0, parse_color(&theme.border, 0.22)),
        }

        if self.panel_expanded {
            self.paint_bar_expanded_inner(&mut pm, sw, sh, is_vertical, &theme.clone());
        } else {
            self.paint_bar_collapsed_inner(&mut pm, sw, sh, is_vertical, position, &theme.clone());
        }

        let result = rgba_to_argb(pm.data());
        self.bar_pixmap = Some(pm);
        result
    }

    // ── collapsed bar (52 px strip) ──────────────────────────────────────────

    fn paint_bar_collapsed_inner(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, is_vertical: bool, position: &BarPosition, theme: &Theme) {
        let workspaces = self.workspaces.clone();
        let mx = self.bar_mouse_x; let my = self.bar_mouse_y;
        let pad = 6.0f32; let gap = 5.0f32;
        let mut new_bar_cards: Vec<WsThumbRect> = Vec::new();
        let mut new_bar_btns: Vec<ButtonRect>   = Vec::new();

        if is_vertical {
            // Vertical layout (left / right)
            let thumb_w = sw - pad*2.0;
            let thumb_h = thumb_w;  // square cards — numbers read much better than 16:9
            let btn_sz  = thumb_w; let r = 5.0f32;

            // Total height of Bottom-slot widgets (gap above each one).
            let bottom_widgets_h: f32 = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Bottom)
                .map(|(d,_)| d.height as f32 + gap)
                .sum();
            // Total height of Top-slot widgets (gap below each one).
            let top_widgets_h: f32 = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Top)
                .map(|(d,_)| d.height as f32 + gap)
                .sum();

            // System info section height (below workspace cards, above hide button):
            //  separator(1) + clock(18) + cpu(12) + ram(12) + vol(12) + gap(4) + scratchpad(28) + gap(4) + hide(btn_sz) + pad
            let sys_sec_h = 1.0 + 18.0 + 12.0 + 12.0 + 12.0 + 4.0 + 28.0 + gap + btn_sz + gap + pad
                + bottom_widgets_h;

            // Toggle overlay button — top
            let tog_y = pad;
            let tog_hov = mx >= pad && mx <= pad+btn_sz && my >= tog_y && my <= tog_y+btn_sz;
            fill_rrect(pm, pad, tog_y, btn_sz, btn_sz, r,
                       parse_color(&theme.accent, if tog_hov {0.30} else {0.14}));
            let (cx, cy, dr) = (pad+btn_sz/2.0, tog_y+btn_sz/2.0, 2.5f32);
            for &dy in &[-4.0f32, 4.0] { for &dx in &[-4.0f32, 4.0] {
                fill_circle(pm, cx+dx, cy+dy, dr, parse_color(&theme.accent, if tog_hov {0.9} else {0.5}));
            }}
            new_bar_btns.push(ButtonRect { x: pad, y: tog_y, w: btn_sz, h: btn_sz,
                                           action: WindowAction::ToggleOverlay });

            // ── Top-slot plugin widgets ───────────────────────────────────
            let mut top_widget_y = tog_y + btn_sz + gap;
            let top_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Top)
                .cloned().collect();
            for (def, cmds) in &top_widgets {
                let wh = def.height as f32;
                self.execute_draw_cmds(pm, pad, top_widget_y, thumb_w, wh, cmds, theme);
                top_widget_y += wh + gap;
            }

            // Workspace thumbnails
            let ws_start_y = tog_y + btn_sz + gap + 4.0 + top_widgets_h;
            for (i, ws) in workspaces.iter().enumerate() {
                let ty = ws_start_y + i as f32 * (thumb_h + gap);
                if ty + thumb_h > sh - sys_sec_h { break; }
                let is_active = ws.active;
                let hov = mx >= pad && mx <= pad+thumb_w && my >= ty && my <= ty+thumb_h;
                let bw = if is_active { 2.0f32 } else { 1.0 };
                let bcol = if is_active { parse_color(&theme.accent, 0.85) }
                    else { parse_color(&theme.border, if hov {0.45} else {0.18}) };
                // Card fill — active gets a tinted background
                fill_rrect(pm, pad-bw, ty-bw, thumb_w+bw*2.0, thumb_h+bw*2.0, r+bw, bcol);
                fill_rrect(pm, pad, ty, thumb_w, thumb_h, r,
                           if is_active { parse_color(&theme.accent, 0.18) }
                           else { parse_color(&theme.background, if hov {0.65} else {0.40}) });
                // Workspace number + window count — fixed readable sizes
                let num = format!("{}", ws.id);
                let nfs = 13.0f32;  // consistent regardless of card size
                let cfs = 9.0f32;
                let win_count = ws.windows.len();
                let nc = if is_active { parse_color(&theme.accent, 1.0) }
                         else { parse_color(&theme.text, if hov {0.85} else {0.60}) };
                // Center vertically: if showing count, treat number+count as a block
                let block_h = if win_count > 0 { nfs + 1.0 + cfs } else { nfs };
                let block_y = ty + thumb_h/2.0 - block_h/2.0;
                let nw = self.text.measure(&num, nfs);
                self.text.draw(pm, &num, pad+thumb_w/2.0-nw/2.0, block_y, nfs, nc);
                if win_count > 0 {
                    let cnt = format!("{} win", win_count);
                    let cw  = self.text.measure(&cnt, cfs);
                    self.text.draw(pm, &cnt, pad+thumb_w/2.0-cw/2.0,
                                   block_y + nfs + 1.0, cfs,
                                   if is_active { parse_color(&theme.accent, 0.60) }
                                   else { parse_color(&theme.text, 0.35) });
                }
                if hov { fill_rrect(pm, pad, ty, thumb_w, thumb_h, r, parse_color(&theme.background, 0.18)); }
                new_bar_cards.push(WsThumbRect { x: pad, y: ty, w: thumb_w, h: thumb_h, ws_idx: i });
            }

            // ── Bottom-slot plugin widgets ────────────────────────────────
            // Anchored just above the system info separator.
            let hide_y_approx = sh - btn_sz - pad;
            let sys_start_approx = hide_y_approx - gap - 28.0 - 4.0 - 12.0 - 12.0 - 12.0 - 18.0 - 2.0;
            let mut bot_widget_y = sys_start_approx - bottom_widgets_h;
            let bottom_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Bottom)
                .cloned().collect();
            for (def, cmds) in &bottom_widgets {
                let wh = def.height as f32;
                self.execute_draw_cmds(pm, pad, bot_widget_y, thumb_w, wh, cmds, theme);
                bot_widget_y += wh + gap;
            }

            // ── System info widgets ───────────────────────────────────────
            let hide_y = sh - btn_sz - pad;
            let scratchpad_y = hide_y - gap - 28.0;
            let sys_start_y  = scratchpad_y - 4.0 - 12.0 - 12.0 - 12.0 - 18.0 - 2.0;

            // Separator
            fill_rect(pm, pad, sys_start_y, btn_sz, 1.0,
                      parse_color(&theme.border, 0.18));

            // Clock (local time)
            let clock_str = chrono::Local::now().format("%H:%M").to_string();
            let clock_fs = 12.0f32;
            let clock_y = sys_start_y + 2.0;
            let cw = self.text.measure(&clock_str, clock_fs);
            self.text.draw(pm, &clock_str, pad+btn_sz/2.0-cw/2.0, clock_y, clock_fs,
                           parse_color(&theme.text, 0.75));

            // CPU
            let sys = self.sys.clone();
            let cpu_str = format!("C {:.0}%", sys.cpu_pct);
            let cpu_fs = 9.0f32;
            let cpu_y = clock_y + 18.0;
            let cpu_col = if sys.cpu_pct > 80.0 { Color::from_rgba8(243,139,168,200) }
                          else if sys.cpu_pct > 50.0 { Color::from_rgba8(250,179,135,200) }
                          else { parse_color(&theme.text, 0.50) };
            let cpuw = self.text.measure(&cpu_str, cpu_fs);
            self.text.draw(pm, &cpu_str, pad+btn_sz/2.0-cpuw/2.0, cpu_y, cpu_fs, cpu_col);

            // RAM
            let ram_str = format!("{:.0}%", if sys.mem_total_kb > 0 {
                sys.mem_used_kb as f32 / sys.mem_total_kb as f32 * 100.0 } else { 0.0 });
            let ram_label = format!("M {}", ram_str);
            let ram_y = cpu_y + 12.0;
            let rw = self.text.measure(&ram_label, cpu_fs);
            let ram_col = if sys.mem_total_kb > 0 && sys.mem_used_kb as f32 / sys.mem_total_kb as f32 > 0.85 {
                Color::from_rgba8(243,139,168,200) } else { parse_color(&theme.text, 0.50) };
            self.text.draw(pm, &ram_label, pad+btn_sz/2.0-rw/2.0, ram_y, cpu_fs, ram_col);

            // Volume
            let vol_y = ram_y + 12.0;
            let vol_str = match sys.volume_pct {
                Some(v) => format!("V {:.0}%", v),
                None    => "V --".to_string(),
            };
            let vw = self.text.measure(&vol_str, cpu_fs);
            self.text.draw(pm, &vol_str, pad+btn_sz/2.0-vw/2.0, vol_y, cpu_fs,
                           parse_color(&theme.text, 0.50));

            // CPU visual bar — fills the slot where scratchpad button was
            {
                let cpu = sys.cpu_pct;
                let bar_x = pad; let bar_y = scratchpad_y;
                let bar_w = btn_sz; let bar_h = 28.0f32;
                let inner_pad = 3.0f32;
                let fill_y    = bar_y + bar_h - inner_pad - 6.0;
                let fill_w    = bar_w - inner_pad*2.0;
                // background card
                fill_rrect(pm, bar_x, bar_y, bar_w, bar_h, r,
                           parse_color(&theme.background, 0.30));
                // label
                let lbl = "cpu"; let lfs = 8.0f32;
                let lw = self.text.measure(lbl, lfs);
                self.text.draw(pm, lbl, bar_x+bar_w/2.0-lw/2.0, bar_y+inner_pad+1.0,
                               lfs, parse_color(&theme.text, 0.40));
                // percentage text
                let pct_s = format!("{:.0}%", cpu);
                let pfs = 9.0f32; let pw = self.text.measure(&pct_s, pfs);
                let pct_col = if cpu > 80.0 { Color::from_rgba8(243,139,168,220) }
                              else if cpu > 50.0 { Color::from_rgba8(250,179,135,220) }
                              else { parse_color(&theme.accent, 0.85) };
                self.text.draw(pm, &pct_s, bar_x+bar_w/2.0-pw/2.0,
                               fill_y - pfs - 2.0, pfs, pct_col);
                // track
                fill_rrect(pm, bar_x+inner_pad, fill_y, fill_w, 6.0, 3.0,
                           parse_color(&theme.border, 0.25));
                // fill
                let filled_w = (fill_w * cpu / 100.0).clamp(0.0, fill_w);
                if filled_w >= 1.0 {
                    fill_rrect(pm, bar_x+inner_pad, fill_y, filled_w, 6.0, 3.0, pct_col);
                }
            }

            // Expand panel button — bottom (replaces hide)
            let exp_hov = mx >= pad && mx <= pad+btn_sz && my >= hide_y && my <= hide_y+btn_sz;
            fill_rrect(pm, pad, hide_y, btn_sz, btn_sz, r,
                       parse_color(&theme.accent, if exp_hov {0.30} else {0.14}));
            let lbl = "→"; let fs = 12.0f32; let lw = self.text.measure(lbl, fs);
            self.text.draw(pm, lbl, pad+btn_sz/2.0-lw/2.0, hide_y+btn_sz/2.0-fs/2.0,
                           fs, parse_color(&theme.accent, if exp_hov {1.0} else {0.55}));
            new_bar_btns.push(ButtonRect { x: pad, y: hide_y, w: btn_sz, h: btn_sz,
                                           action: WindowAction::ExpandPanel });

        } else {
            // Horizontal layout (top / bottom)
            let thumb_h = sh - pad*2.0;
            let thumb_w = thumb_h;  // square cards
            let btn_sz  = thumb_h; let r = 5.0f32;

            // ── Right-anchored section (expand btn + system info) ────────
            // Laid out right-to-left so we know where workspace cards stop.

            let mut right_x = sw - pad; // cursor moving leftward

            // Expand panel button — rightmost
            right_x -= btn_sz;
            let exp_x = right_x;
            let exp_hov = mx >= exp_x && mx <= exp_x+btn_sz && my >= pad && my <= pad+btn_sz;
            fill_rrect(pm, exp_x, pad, btn_sz, btn_sz, r,
                       parse_color(&theme.accent, if exp_hov {0.30} else {0.14}));
            let exp_lbl = if matches!(position, BarPosition::Bottom) { "↑" } else { "↓" };
            let fs = 12.0f32; let lw = self.text.measure(exp_lbl, fs);
            self.text.draw(pm, exp_lbl, exp_x+btn_sz/2.0-lw/2.0, pad+btn_sz/2.0-fs/2.0,
                           fs, parse_color(&theme.accent, if exp_hov {1.0} else {0.55}));
            new_bar_btns.push(ButtonRect { x: exp_x, y: pad, w: btn_sz, h: btn_sz,
                                           action: WindowAction::ExpandPanel });
            right_x -= gap;

            // Separator before system info
            fill_rect(pm, right_x, pad+4.0, 1.0, thumb_h-8.0,
                      parse_color(&theme.border, 0.18));
            right_x -= gap;

            // Compact system info: clock | CPU | RAM | VOL
            let sys = self.sys.clone();
            let info_fs = 9.0f32;
            let info_items: Vec<(String, tiny_skia::Color)> = vec![
                (chrono::Local::now().format("%H:%M").to_string(),
                 parse_color(&theme.text, 0.75)),
                (format!("C{:.0}%", sys.cpu_pct),
                 if sys.cpu_pct > 80.0 { Color::from_rgba8(243,139,168,200) }
                 else if sys.cpu_pct > 50.0 { Color::from_rgba8(250,179,135,200) }
                 else { parse_color(&theme.text, 0.50) }),
                (format!("M{:.0}%", if sys.mem_total_kb > 0 {
                    sys.mem_used_kb as f32 / sys.mem_total_kb as f32 * 100.0 } else { 0.0 }),
                 parse_color(&theme.text, 0.50)),
                (match sys.volume_pct { Some(v) => format!("V{:.0}%", v), None => "V--".into() },
                 parse_color(&theme.text, 0.50)),
            ];
            for (label, col) in info_items.iter().rev() {
                let tw = self.text.measure(label, info_fs);
                right_x -= tw;
                self.text.draw(pm, label, right_x, pad+thumb_h/2.0-info_fs/2.0,
                               info_fs, *col);
                right_x -= gap + 2.0;
            }

            // Separator after system info
            fill_rect(pm, right_x, pad+4.0, 1.0, thumb_h-8.0,
                      parse_color(&theme.border, 0.18));
            right_x -= gap;

            // ── Bottom-slot widgets (right side, before sys info) ─────────
            let bottom_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Bottom)
                .cloned().collect();
            for (def, cmds) in bottom_widgets.iter().rev() {
                let ww = def.height as f32; // use widget "height" as width in horizontal
                right_x -= ww;
                self.execute_draw_cmds(pm, right_x, pad, ww, thumb_h, cmds, theme);
                right_x -= gap;
            }

            let ws_end_x = right_x; // workspace cards must not go past here

            // ── Left-anchored section ────────────────────────────────────
            let mut left_x = pad;

            // Toggle overlay button — leftmost
            let tog_hov = mx >= left_x && mx <= left_x+btn_sz && my >= pad && my <= pad+btn_sz;
            fill_rrect(pm, left_x, pad, btn_sz, btn_sz, r,
                       parse_color(&theme.accent, if tog_hov {0.30} else {0.14}));
            let (cx, cy_dot, dr) = (left_x+btn_sz/2.0, pad+btn_sz/2.0, 2.5f32);
            for &dy in &[-4.0f32, 4.0] { for &dx in &[-4.0f32, 4.0] {
                fill_circle(pm, cx+dx, cy_dot+dy, dr, parse_color(&theme.accent, if tog_hov {0.9} else {0.5}));
            }}
            new_bar_btns.push(ButtonRect { x: left_x, y: pad, w: btn_sz, h: btn_sz,
                                           action: WindowAction::ToggleOverlay });
            left_x += btn_sz + gap;

            // ── Top-slot widgets (left side, after toggle) ───────────────
            let top_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
                .filter(|(d,_)| d.slot == WidgetSlot::Top)
                .cloned().collect();
            for (def, cmds) in &top_widgets {
                let ww = def.height as f32; // use widget "height" as width in horizontal
                self.execute_draw_cmds(pm, left_x, pad, ww, thumb_h, cmds, theme);
                left_x += ww + gap;
            }

            // ── Workspace thumbnails (center area) ───────────────────────
            let ws_start_x = left_x + 4.0;
            for (i, ws) in workspaces.iter().enumerate() {
                let tx = ws_start_x + i as f32 * (thumb_w + gap);
                if tx + thumb_w > ws_end_x { break; }
                let is_active = ws.active;
                let hov = mx >= tx && mx <= tx+thumb_w && my >= pad && my <= pad+thumb_h;
                let bw = if is_active { 2.0f32 } else { 1.0 };
                let bcol = if is_active { parse_color(&theme.accent, 0.85) }
                    else { parse_color(&theme.border, if hov {0.45} else {0.18}) };
                fill_rrect(pm, tx-bw, pad-bw, thumb_w+bw*2.0, thumb_h+bw*2.0, r+bw, bcol);
                fill_rrect(pm, tx, pad, thumb_w, thumb_h, r,
                           if is_active { parse_color(&theme.accent, 0.18) }
                           else { parse_color(&theme.background, if hov {0.65} else {0.40}) });
                let num = format!("{}", ws.id);
                let nfs = 13.0f32;
                let cfs = 9.0f32;
                let win_count = ws.windows.len();
                let nc = if is_active { parse_color(&theme.accent, 1.0) }
                         else { parse_color(&theme.text, if hov {0.85} else {0.60}) };
                let block_h = if win_count > 0 { nfs + 1.0 + cfs } else { nfs };
                let block_y = pad + thumb_h/2.0 - block_h/2.0;
                let nw = self.text.measure(&num, nfs);
                self.text.draw(pm, &num, tx+thumb_w/2.0-nw/2.0, block_y, nfs, nc);
                if win_count > 0 {
                    let cnt = format!("{} win", win_count);
                    let cw  = self.text.measure(&cnt, cfs);
                    self.text.draw(pm, &cnt, tx+thumb_w/2.0-cw/2.0,
                                   block_y + nfs + 1.0, cfs,
                                   if is_active { parse_color(&theme.accent, 0.60) }
                                   else { parse_color(&theme.text, 0.35) });
                }
                if hov { fill_rrect(pm, tx, pad, thumb_w, thumb_h, r, parse_color(&theme.background, 0.18)); }
                if is_active { fill_circle(pm, tx+thumb_w-5.0, pad+5.0, 3.0, parse_color(&theme.accent, 0.9)); }
                new_bar_cards.push(WsThumbRect { x: tx, y: pad, w: thumb_w, h: thumb_h, ws_idx: i });
            }
        }

        self.bar_cards   = new_bar_cards;
        self.bar_buttons = new_bar_btns;
    }

    // ── expanded control center (300 px) ─────────────────────────────────────

    fn paint_bar_expanded_inner(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, is_vertical: bool, theme: &Theme) {
        if is_vertical {
            self.paint_bar_expanded_vertical(pm, sw, sh, theme);
        } else {
            self.paint_bar_expanded_horizontal(pm, sw, sh, theme);
        }
    }

    // ── Vertical expanded (300×screen_h column) ─────────────────────────────
    fn paint_bar_expanded_vertical(&mut self, pm: &mut Pixmap, sw: f32, _sh: f32, theme: &Theme) {
        let sys = self.sys.clone();
        let mx  = self.bar_mouse_x; let my = self.bar_mouse_y;
        let pad = 14.0f32; let r = 8.0f32;
        let inner_w = sw - pad * 2.0;
        let mut new_bar_btns:  Vec<ButtonRect>   = Vec::new();
        let mut new_bar_cards: Vec<WsThumbRect>  = Vec::new();
        let mut cy = pad;

        // helper: draw a section-label header
        macro_rules! section {
            ($label:expr) => {{
                let fs = 9.0f32;
                self.text.draw(pm, $label, pad, cy, fs, parse_color(&theme.text, 0.30));
                cy += fs + 5.0;
            }};
        }
        macro_rules! sep {
            () => {{
                cy += 6.0;
                fill_rect(pm, pad, cy, inner_w, 1.0, parse_color(&theme.border, 0.14));
                cy += 8.0;
            }};
        }

        // ─── Header ──────────────────────────────────────────────────────────
        let hdr_h = 36.0f32;
        let (lx, lcy, dr) = (pad + 8.0, cy + hdr_h/2.0, 3.0f32);
        for &dx in &[-5.0f32, 0.0, 5.0] {
            fill_circle(pm, lx+dx, lcy, dr, parse_color(&theme.accent, 0.65));
        }
        let lbl_fs = 13.0f32;
        self.text.draw(pm, "woven", lx + 16.0, cy + hdr_h/2.0 - lbl_fs/2.0,
                       lbl_fs, parse_color(&theme.accent, 0.90));

        let btn_h = 22.0f32; let btn_w = 30.0f32;
        let hide_x = sw - pad - btn_w;
        let hide_y = cy + hdr_h/2.0 - btn_h/2.0;
        let hov_hide = mx >= hide_x && mx <= hide_x+btn_w && my >= hide_y && my <= hide_y+btn_h;
        fill_rrect(pm, hide_x, hide_y, btn_w, btn_h, 5.0,
                   parse_color(&theme.border, if hov_hide {0.40} else {0.15}));
        let lw = self.text.measure("--", 9.0);
        self.text.draw(pm, "--", hide_x+btn_w/2.0-lw/2.0, hide_y+btn_h/2.0-4.5,
                       9.0, parse_color(&theme.text, if hov_hide {0.9} else {0.45}));
        new_bar_btns.push(ButtonRect { x: hide_x, y: hide_y, w: btn_w, h: btn_h,
                                       action: WindowAction::HideBar });

        let coll_x = hide_x - btn_w - 5.0; let coll_y = hide_y;
        let hov_coll = mx >= coll_x && mx <= coll_x+btn_w && my >= coll_y && my <= coll_y+btn_h;
        fill_rrect(pm, coll_x, coll_y, btn_w, btn_h, 5.0,
                   parse_color(&theme.accent, if hov_coll {0.30} else {0.13}));
        let lw = self.text.measure("<", 11.0);
        self.text.draw(pm, "<", coll_x+btn_w/2.0-lw/2.0, coll_y+btn_h/2.0-5.5,
                       11.0, parse_color(&theme.accent, if hov_coll {1.0} else {0.60}));
        new_bar_btns.push(ButtonRect { x: coll_x, y: coll_y, w: btn_w, h: btn_h,
                                       action: WindowAction::CollapsePanel });

        let tog_x = pad; let tog_y = cy + hdr_h/2.0 - btn_h/2.0; let tog_w = 28.0f32;
        let hov_tog = mx >= tog_x && mx <= tog_x+tog_w && my >= tog_y && my <= tog_y+btn_h;
        fill_rrect(pm, tog_x, tog_y, tog_w, btn_h, 5.0,
                   parse_color(&theme.accent, if hov_tog {0.28} else {0.12}));
        let (dcx, dcy, ddr) = (tog_x+tog_w/2.0, tog_y+btn_h/2.0, 2.2f32);
        for &ddy in &[-3.5f32, 3.5] { for &ddx in &[-3.5f32, 3.5] {
            fill_circle(pm, dcx+ddx, dcy+ddy, ddr,
                        parse_color(&theme.accent, if hov_tog {0.90} else {0.45}));
        }}
        new_bar_btns.push(ButtonRect { x: tog_x, y: tog_y, w: tog_w, h: btn_h,
                                       action: WindowAction::ToggleOverlay });

        cy += hdr_h;
        sep!();

        // ─── Clock + Date + Weather ───────────────────────────────────────────
        let now_local = chrono::Local::now();
        let clock_str = now_local.format("%H:%M").to_string();
        let date_str  = now_local.format("%A, %d %b").to_string();

        let clock_fs = 36.0f32;
        let cw = self.text.measure(&clock_str, clock_fs);
        self.text.draw(pm, &clock_str, sw/2.0-cw/2.0, cy, clock_fs,
                       parse_color(&theme.text, 0.96));
        cy += clock_fs + 4.0;

        let date_fs = 12.0f32;
        let dw = self.text.measure(&date_str, date_fs);
        self.text.draw(pm, &date_str, sw/2.0-dw/2.0, cy, date_fs,
                       parse_color(&theme.text, 0.50));
        cy += date_fs + 4.0;

        if let Some(ref wx_str) = sys.weather {
            let clean: String = wx_str.chars()
                .filter(|c| c.is_ascii() || *c == '\u{00B0}')
                .collect::<String>()
                .trim()
                .to_string();
            if !clean.is_empty() {
                let wx_fs = 10.5f32;
                let ww = self.text.measure(&clean, wx_fs);
                self.text.draw(pm, &clean, sw/2.0-ww/2.0, cy, wx_fs,
                               parse_color(&theme.text, 0.38));
                cy += wx_fs + 2.0;
            }
        }
        sep!();

        // ─── Media player ─────────────────────────────────────────────────────
        let has_media = sys.media_title.is_some() || sys.media_artist.is_some();
        if has_media {
            section!("NOW PLAYING");
            let card_h = 70.0f32;
            fill_rrect(pm, pad, cy, inner_w, card_h, r,
                       parse_color(&theme.background, 0.45));

            let title  = sys.media_title.as_deref().unwrap_or("Unknown");
            let artist = sys.media_artist.as_deref().unwrap_or("");
            let tfs = 11.5f32; let afs = 10.0f32;
            let text_pad = 10.0f32;
            self.text.draw(pm, &truncate(title, 26), pad+text_pad, cy+10.0,
                           tfs, parse_color(&theme.text, 0.92));
            self.text.draw(pm, &truncate(artist, 30), pad+text_pad, cy+10.0+tfs+3.0,
                           afs, parse_color(&theme.text, 0.55));

            let play_lbl = if sys.media_playing { "||" } else { "> " };
            let media_btns: &[(&str, WindowAction)] = &[
                ("|<", WindowAction::MediaPrev),
                (play_lbl, WindowAction::MediaPlayPause),
                (">|", WindowAction::MediaNext),
            ];
            let mbtn_w = 36.0f32; let mbtn_h = 20.0f32;
            let total_w = 3.0 * mbtn_w + 2.0 * 6.0;
            let mbtn_x0 = sw/2.0 - total_w/2.0;
            let mbtn_y  = cy + card_h - mbtn_h - 8.0;
            for (i, (lbl, action)) in media_btns.iter().enumerate() {
                let bx = mbtn_x0 + i as f32 * (mbtn_w + 6.0);
                let hov = mx >= bx && mx <= bx+mbtn_w && my >= mbtn_y && my <= mbtn_y+mbtn_h;
                fill_rrect(pm, bx, mbtn_y, mbtn_w, mbtn_h, 5.0,
                           parse_color(&theme.accent, if hov {0.35} else {0.16}));
                let lfs = if *lbl == "> " || *lbl == "||" { 10.0f32 } else { 9.0f32 };
                let lw2 = self.text.measure(lbl, lfs);
                self.text.draw(pm, lbl, bx+mbtn_w/2.0-lw2/2.0, mbtn_y+mbtn_h/2.0-lfs/2.0,
                               lfs, parse_color(&theme.accent, if hov {1.0} else {0.72}));
                new_bar_btns.push(ButtonRect { x: bx, y: mbtn_y, w: mbtn_w, h: mbtn_h,
                                               action: action.clone() });
            }
            cy += card_h;
            sep!();
        }

        // ─── Quick tiles: WiFi + BT ───────────────────────────────────────────
        section!("CONNECTIVITY");
        {
            let tile_h  = 52.0f32;
            let tile_w  = (inner_w - 8.0) / 2.0;
            let lbl_fs  = 8.5f32;
            let val_fs  = 11.0f32;

            let tiles: &[(&str, &str, bool, WindowAction)] = &[
                ("WiFi",
                 &sys.wifi_ssid.as_deref()
                     .map(|s| truncate(s, 12))
                     .unwrap_or_else(|| "Off".into()),
                 sys.wifi_ssid.is_some(),
                 WindowAction::WifiToggle),
                ("Bluetooth",
                 if sys.bt_on { "On" } else { "Off" },
                 sys.bt_on,
                 WindowAction::BtToggle),
            ];

            for (i, (label, value, active, action)) in tiles.iter().enumerate() {
                let tx = pad + i as f32 * (tile_w + 8.0);
                let hov = mx >= tx && mx <= tx+tile_w && my >= cy && my <= cy+tile_h;
                let accent = parse_color(&theme.accent, if *active { if hov {0.35} else {0.22} }
                                                        else if hov {0.18} else {0.10});
                let border  = parse_color(&theme.accent, if *active {0.70} else {0.20});
                fill_rrect(pm, tx-1.0, cy-1.0, tile_w+2.0, tile_h+2.0, r+1.0, border);
                fill_rrect(pm, tx, cy, tile_w, tile_h, r, accent);
                self.text.draw(pm, label, tx+9.0, cy+8.0, lbl_fs,
                               parse_color(&theme.text, 0.42));
                let vw = self.text.measure(value, val_fs);
                let vc = if *active { parse_color(&theme.accent, 1.0) }
                         else { parse_color(&theme.text, 0.38) };
                self.text.draw(pm, value, tx+tile_w/2.0-vw/2.0, cy+tile_h/2.0-val_fs/2.0+4.0,
                               val_fs, vc);
                new_bar_btns.push(ButtonRect { x: tx, y: cy, w: tile_w, h: tile_h,
                                               action: action.clone() });
            }
            cy += tile_h;
        }
        sep!();

        // ─── Power ────────────────────────────────────────────────────────────
        section!("POWER");
        {
            let pbtn_h = 30.0f32; let pbtn_fs = 9.5f32; let gap = 5.0f32;
            let rows: &[&[(&str, WindowAction, [u8;3])]] = &[
                &[
                    ("Suspend",  WindowAction::PowerSuspend,  [166,227,161]),
                    ("Reboot",   WindowAction::PowerReboot,   [250,179,135]),
                    ("Shutdown", WindowAction::PowerShutdown, [243,139,168]),
                ],
                &[
                    ("Lock",     WindowAction::PowerLock,     [137,180,250]),
                    ("Logout",   WindowAction::PowerLogout,   [203,166,247]),
                ],
            ];
            for row in rows.iter() {
                let n = row.len() as f32;
                let bw = (inner_w - gap*(n-1.0)) / n;
                for (i, (lbl, action, col)) in row.iter().enumerate() {
                    let bx = pad + i as f32 * (bw + gap);
                    let hov = mx >= bx && mx <= bx+bw && my >= cy && my <= cy+pbtn_h;
                    fill_rrect(pm, bx, cy, bw, pbtn_h, 6.0,
                               Color::from_rgba8(col[0],col[1],col[2], if hov {60} else {22}));
                    fill_rrect(pm, bx, cy, 3.0, pbtn_h, 3.0,
                               Color::from_rgba8(col[0],col[1],col[2], if hov {200} else {110}));
                    let tw2 = self.text.measure(lbl, pbtn_fs);
                    self.text.draw(pm, lbl, bx+bw/2.0-tw2/2.0+2.0, cy+pbtn_h/2.0-pbtn_fs/2.0,
                                   pbtn_fs, Color::from_rgba8(col[0],col[1],col[2],
                                                               if hov {240} else {170}));
                    new_bar_btns.push(ButtonRect { x: bx, y: cy, w: bw, h: pbtn_h,
                                                   action: action.clone() });
                }
                cy += pbtn_h + gap;
            }
            cy -= gap;
        }
        sep!();

        // ─── Workspaces ───────────────────────────────────────────────────────
        section!("WORKSPACES");
        {
            let workspaces = self.workspaces.clone();
            let n = workspaces.len().max(1) as f32;
            let card_sz = ((inner_w - (n-1.0)*5.0) / n).clamp(38.0, 60.0);
            let card_h  = card_sz + 14.0;
            let card_gap = 5.0f32;
            let total_w = n * card_sz + (n-1.0)*card_gap;
            let start_x = pad + (inner_w - total_w) / 2.0;

            for (i, ws) in workspaces.iter().enumerate() {
                let tx = start_x + i as f32 * (card_sz + card_gap);
                let is_active = ws.active;
                let hov = mx >= tx && mx <= tx+card_sz && my >= cy && my <= cy+card_h;
                if is_active {
                    fill_rrect(pm, tx-1.5, cy-1.5, card_sz+3.0, card_sz+3.0, 6.0,
                               parse_color(&theme.accent, 0.80));
                }
                fill_rrect(pm, tx, cy, card_sz, card_sz, 5.0,
                           parse_color(&theme.background, if is_active {0.15} else if hov {0.60} else {0.45}));
                if is_active {
                    fill_rrect(pm, tx, cy, card_sz, card_sz, 5.0,
                               parse_color(&theme.accent, 0.18));
                }
                let num = format!("{}", ws.id);
                let nfs = 14.0f32; let nw = self.text.measure(&num, nfs);
                let nc  = if is_active { parse_color(&theme.accent, 1.0) }
                          else { parse_color(&theme.text, if hov {0.85} else {0.60}) };
                let count = ws.windows.len();
                let block_h = if count > 0 { nfs + 1.0 + 9.0 } else { nfs };
                let by = cy + card_sz/2.0 - block_h/2.0;
                self.text.draw(pm, &num, tx+card_sz/2.0-nw/2.0, by, nfs, nc);
                if count > 0 {
                    let cnt_s = format!("{}", count);
                    let cw2   = self.text.measure(&cnt_s, 9.0);
                    let cnt_c = if is_active { parse_color(&theme.accent, 0.55) }
                                else { parse_color(&theme.text, 0.30) };
                    self.text.draw(pm, &cnt_s, tx+card_sz/2.0-cw2/2.0, by+nfs+1.0, 9.0, cnt_c);
                }
                if count > 0 {
                    let max_wins = 8usize;
                    let dot_r = 2.0f32; let dot_gap = 3.0f32;
                    let dots  = count.min(max_wins);
                    let total = dots as f32 * (dot_r*2.0 + dot_gap) - dot_gap;
                    let dx0   = tx + card_sz/2.0 - total/2.0 + dot_r;
                    for d in 0..dots {
                        fill_circle(pm, dx0 + d as f32*(dot_r*2.0+dot_gap),
                                    cy + card_sz - dot_r - 3.0, dot_r,
                                    parse_color(&theme.accent, if is_active {0.65} else {0.30}));
                    }
                }
                new_bar_cards.push(WsThumbRect { x: tx, y: cy, w: card_sz, h: card_h, ws_idx: i });
            }
            cy += card_h + 2.0;

            if workspaces.is_empty() {
                let msg = "no workspaces"; let mfs = 10.0f32;
                let mw = self.text.measure(msg, mfs);
                self.text.draw(pm, msg, sw/2.0-mw/2.0, cy, mfs, parse_color(&theme.text, 0.22));
                cy += mfs + 4.0;
            }
        }
        sep!();

        // ─── System stats ─────────────────────────────────────────────────────
        section!("SYSTEM");
        {
            let row_h   = 22.0f32; let bar_gap = 7.0f32;
            let lbl_w   = 56.0f32; let stat_fs = 9.0f32;
            let val_w   = 34.0f32;
            let track_x = pad + lbl_w + 4.0;
            let track_w = inner_w - lbl_w - 4.0 - val_w;
            let val_x   = track_x + track_w + 4.0;
            let bar_y_off = row_h/2.0 - 4.0;
            let bar_h_px = 7.0f32;

            let draw_stat = |pm: &mut Pixmap,
                                  text: &mut crate::text::TextRenderer,
                                  label: &str, pct: f32, col: Color, cy: f32| {
                text.draw(pm, label, pad, cy + row_h/2.0 - stat_fs/2.0,
                          stat_fs, parse_color(&theme.text, 0.42));
                fill_rrect(pm, track_x, cy+bar_y_off, track_w, bar_h_px, bar_h_px/2.0,
                           parse_color(&theme.border, 0.20));
                let fw = (track_w * pct.clamp(0.0,100.0) / 100.0).max(0.0);
                if fw >= 1.0 { fill_rrect(pm, track_x, cy+bar_y_off, fw, bar_h_px, bar_h_px/2.0, col); }
                let pct_s = format!("{:.0}%", pct);
                let pw = text.measure(&pct_s, stat_fs);
                text.draw(pm, &pct_s, val_x + val_w - pw, cy + row_h/2.0 - stat_fs/2.0,
                          stat_fs, col);
            };

            let cpu = sys.cpu_pct;
            let cpu_col = if cpu > 80.0 { Color::from_rgba8(243,139,168,210) }
                          else if cpu > 50.0 { Color::from_rgba8(250,179,135,210) }
                          else { parse_color(&theme.accent, 0.85) };
            let cpu_lbl = if let Some(t) = sys.cpu_temp_c {
                format!("CPU {:.0}C", t)
            } else { "CPU".into() };
            draw_stat(pm, &mut self.text, &cpu_lbl, cpu, cpu_col, cy);
            cy += row_h + bar_gap;

            if let Some(gt) = sys.gpu_temp_c {
                let gpu_pct = (gt / 100.0 * 100.0).clamp(0.0, 100.0);
                let gpu_col = if gt > 85.0 { Color::from_rgba8(243,139,168,210) }
                              else if gt > 65.0 { Color::from_rgba8(250,179,135,210) }
                              else { Color::from_rgba8(137,220,235,210) };
                let gpu_lbl = format!("GPU {:.0}C", gt);
                draw_stat(pm, &mut self.text, &gpu_lbl, gpu_pct, gpu_col, cy);
                cy += row_h + bar_gap;
            }

            let ram_pct = if sys.mem_total_kb > 0 {
                sys.mem_used_kb as f32 / sys.mem_total_kb as f32 * 100.0
            } else { 0.0 };
            let ram_col = if ram_pct > 85.0 { Color::from_rgba8(243,139,168,210) }
                          else { parse_color(&theme.text, 0.52) };
            let ram_used = sys.mem_used_kb as f32 / (1024.0 * 1024.0);
            let ram_total = sys.mem_total_kb as f32 / (1024.0 * 1024.0);
            let ram_lbl = format!("RAM {:.1}G", ram_used);
            let _ = ram_total;
            draw_stat(pm, &mut self.text, &ram_lbl, ram_pct, ram_col, cy);
            cy += row_h + bar_gap;

            let vol_pct = sys.volume_pct.unwrap_or(0.0);
            let vol_col = parse_color(&theme.accent, 0.65);
            draw_stat(pm, &mut self.text, "VOL", vol_pct, vol_col, cy);
            cy += row_h + bar_gap;
        }

        // ─── Panel-slot plugin widgets ─────────────────────────────────────────
        let panel_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
            .filter(|(d, _)| d.slot == WidgetSlot::Panel)
            .cloned().collect();
        if !panel_widgets.is_empty() {
            sep!();
            section!("WIDGETS");
            for (def, cmds) in &panel_widgets {
                let wh = def.height as f32;
                self.execute_draw_cmds(pm, pad, cy, inner_w, wh, cmds, &theme.clone());
                cy += wh + 6.0;
            }
        }

        self.bar_cards   = new_bar_cards;
        self.bar_buttons = new_bar_btns;
    }

    // ── Horizontal expanded (screen_w × 300 multi-column) ───────────────────
    fn paint_bar_expanded_horizontal(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        let sys = self.sys.clone();
        let mx  = self.bar_mouse_x; let my = self.bar_mouse_y;
        let pad = 14.0f32; let r = 8.0f32;
        let col_gap = 12.0f32;
        let inner_h = sh - pad * 2.0;
        let mut new_bar_btns:  Vec<ButtonRect>   = Vec::new();
        let mut new_bar_cards: Vec<WsThumbRect>  = Vec::new();

        // Split into 4 columns across the width
        let ncols = 4.0f32;
        let col_w = (sw - pad * 2.0 - col_gap * (ncols - 1.0)) / ncols;

        // Helper: vertical separator between columns
        let draw_col_sep = |pm: &mut Pixmap, x: f32| {
            fill_rect(pm, x, pad + 6.0, 1.0, inner_h - 12.0,
                      parse_color(&theme.border, 0.12));
        };

        // ═══════════════════════════════════════════════════════════════════
        // COLUMN 1: Header + Clock/Date + Workspaces
        // ═══════════════════════════════════════════════════════════════════
        let c1_x = pad;
        {
            let mut cy = pad;

            // Header row: logo + collapse/hide buttons
            let hdr_h = 28.0f32;
            let (lx, lcy, dr) = (c1_x + 8.0, cy + hdr_h/2.0, 3.0f32);
            for &dx in &[-5.0f32, 0.0, 5.0] {
                fill_circle(pm, lx+dx, lcy, dr, parse_color(&theme.accent, 0.65));
            }
            self.text.draw(pm, "woven", lx + 16.0, cy + hdr_h/2.0 - 6.5,
                           13.0, parse_color(&theme.accent, 0.90));

            // Collapse + hide buttons (right side of col 1)
            let btn_h = 20.0f32; let btn_w = 26.0f32;
            let hide_x = c1_x + col_w - btn_w;
            let hide_y = cy + hdr_h/2.0 - btn_h/2.0;
            let hov_hide = mx >= hide_x && mx <= hide_x+btn_w && my >= hide_y && my <= hide_y+btn_h;
            fill_rrect(pm, hide_x, hide_y, btn_w, btn_h, 5.0,
                       parse_color(&theme.border, if hov_hide {0.40} else {0.15}));
            let lw = self.text.measure("--", 9.0);
            self.text.draw(pm, "--", hide_x+btn_w/2.0-lw/2.0, hide_y+btn_h/2.0-4.5,
                           9.0, parse_color(&theme.text, if hov_hide {0.9} else {0.45}));
            new_bar_btns.push(ButtonRect { x: hide_x, y: hide_y, w: btn_w, h: btn_h,
                                           action: WindowAction::HideBar });

            let coll_x = hide_x - btn_w - 4.0;
            let hov_coll = mx >= coll_x && mx <= coll_x+btn_w && my >= hide_y && my <= hide_y+btn_h;
            fill_rrect(pm, coll_x, hide_y, btn_w, btn_h, 5.0,
                       parse_color(&theme.accent, if hov_coll {0.30} else {0.13}));
            let lw = self.text.measure("v", 11.0);
            self.text.draw(pm, "v", coll_x+btn_w/2.0-lw/2.0, hide_y+btn_h/2.0-5.5,
                           11.0, parse_color(&theme.accent, if hov_coll {1.0} else {0.60}));
            new_bar_btns.push(ButtonRect { x: coll_x, y: hide_y, w: btn_w, h: btn_h,
                                           action: WindowAction::CollapsePanel });

            // Toggle overlay dots
            let tog_x = c1_x; let tog_y = cy + hdr_h/2.0 - btn_h/2.0; let tog_w = 24.0f32;
            let hov_tog = mx >= tog_x && mx <= tog_x+tog_w && my >= tog_y && my <= tog_y+btn_h;
            fill_rrect(pm, tog_x, tog_y, tog_w, btn_h, 5.0,
                       parse_color(&theme.accent, if hov_tog {0.28} else {0.12}));
            let (dcx, dcy, ddr) = (tog_x+tog_w/2.0, tog_y+btn_h/2.0, 2.0f32);
            for &ddy in &[-3.0f32, 3.0] { for &ddx in &[-3.0f32, 3.0] {
                fill_circle(pm, dcx+ddx, dcy+ddy, ddr,
                            parse_color(&theme.accent, if hov_tog {0.90} else {0.45}));
            }}
            new_bar_btns.push(ButtonRect { x: tog_x, y: tog_y, w: tog_w, h: btn_h,
                                           action: WindowAction::ToggleOverlay });

            cy += hdr_h + 8.0;

            // Clock + Date
            let now_local = chrono::Local::now();
            let clock_str = now_local.format("%H:%M").to_string();
            let date_str  = now_local.format("%A, %d %b").to_string();

            let clock_fs = 32.0f32;
            let cw = self.text.measure(&clock_str, clock_fs);
            self.text.draw(pm, &clock_str, c1_x + col_w/2.0 - cw/2.0, cy, clock_fs,
                           parse_color(&theme.text, 0.96));
            cy += clock_fs + 3.0;

            let date_fs = 11.0f32;
            let dw = self.text.measure(&date_str, date_fs);
            self.text.draw(pm, &date_str, c1_x + col_w/2.0 - dw/2.0, cy, date_fs,
                           parse_color(&theme.text, 0.50));
            cy += date_fs + 3.0;

            if let Some(ref wx_str) = sys.weather {
                let clean: String = wx_str.chars()
                    .filter(|c| c.is_ascii() || *c == '\u{00B0}')
                    .collect::<String>().trim().to_string();
                if !clean.is_empty() {
                    let wx_fs = 10.0f32;
                    let ww = self.text.measure(&clean, wx_fs);
                    self.text.draw(pm, &clean, c1_x + col_w/2.0 - ww/2.0, cy, wx_fs,
                                   parse_color(&theme.text, 0.38));
                    cy += wx_fs + 2.0;
                }
            }
            cy += 8.0;

            // Workspaces row
            let sec_fs = 9.0f32;
            self.text.draw(pm, "WORKSPACES", c1_x, cy, sec_fs, parse_color(&theme.text, 0.30));
            cy += sec_fs + 5.0;
            {
                let workspaces = self.workspaces.clone();
                let n = workspaces.len().max(1) as f32;
                let card_sz = ((col_w - (n-1.0)*5.0) / n).clamp(30.0, 50.0);
                let card_h = card_sz + 12.0;
                let card_gap = 4.0f32;
                let total = n * card_sz + (n-1.0)*card_gap;
                let start = c1_x + (col_w - total) / 2.0;

                for (i, ws) in workspaces.iter().enumerate() {
                    let tx = start + i as f32 * (card_sz + card_gap);
                    if tx + card_sz > c1_x + col_w { break; }
                    let is_active = ws.active;
                    let hov = mx >= tx && mx <= tx+card_sz && my >= cy && my <= cy+card_h;
                    if is_active {
                        fill_rrect(pm, tx-1.5, cy-1.5, card_sz+3.0, card_sz+3.0, 5.0,
                                   parse_color(&theme.accent, 0.80));
                    }
                    fill_rrect(pm, tx, cy, card_sz, card_sz, 4.0,
                               parse_color(&theme.background, if is_active {0.15} else if hov {0.60} else {0.45}));
                    if is_active {
                        fill_rrect(pm, tx, cy, card_sz, card_sz, 4.0,
                                   parse_color(&theme.accent, 0.18));
                    }
                    let num = format!("{}", ws.id);
                    let nfs = 12.0f32; let nw = self.text.measure(&num, nfs);
                    let nc = if is_active { parse_color(&theme.accent, 1.0) }
                             else { parse_color(&theme.text, if hov {0.85} else {0.60}) };
                    self.text.draw(pm, &num, tx+card_sz/2.0-nw/2.0, cy+card_sz/2.0-nfs/2.0, nfs, nc);
                    new_bar_cards.push(WsThumbRect { x: tx, y: cy, w: card_sz, h: card_h, ws_idx: i });
                }
            }
        }

        draw_col_sep(pm, c1_x + col_w + col_gap/2.0);

        // ═══════════════════════════════════════════════════════════════════
        // COLUMN 2: Media + Connectivity
        // ═══════════════════════════════════════════════════════════════════
        let c2_x = c1_x + col_w + col_gap;
        {
            let mut cy = pad;

            // Media
            let has_media = sys.media_title.is_some() || sys.media_artist.is_some();
            if has_media {
                let sec_fs = 9.0f32;
                self.text.draw(pm, "NOW PLAYING", c2_x, cy, sec_fs, parse_color(&theme.text, 0.30));
                cy += sec_fs + 5.0;

                let card_h = 65.0f32;
                fill_rrect(pm, c2_x, cy, col_w, card_h, r,
                           parse_color(&theme.background, 0.45));

                let title  = sys.media_title.as_deref().unwrap_or("Unknown");
                let artist = sys.media_artist.as_deref().unwrap_or("");
                self.text.draw(pm, &truncate(title, 30), c2_x+10.0, cy+8.0,
                               11.0, parse_color(&theme.text, 0.92));
                self.text.draw(pm, &truncate(artist, 34), c2_x+10.0, cy+22.0,
                               9.5, parse_color(&theme.text, 0.55));

                let play_lbl = if sys.media_playing { "||" } else { "> " };
                let media_btns: &[(&str, WindowAction)] = &[
                    ("|<", WindowAction::MediaPrev),
                    (play_lbl, WindowAction::MediaPlayPause),
                    (">|", WindowAction::MediaNext),
                ];
                let mbtn_w = 34.0f32; let mbtn_h = 18.0f32;
                let total = 3.0 * mbtn_w + 2.0 * 5.0;
                let mbtn_x0 = c2_x + col_w/2.0 - total/2.0;
                let mbtn_y = cy + card_h - mbtn_h - 6.0;
                for (i, (lbl, action)) in media_btns.iter().enumerate() {
                    let bx = mbtn_x0 + i as f32 * (mbtn_w + 5.0);
                    let hov = mx >= bx && mx <= bx+mbtn_w && my >= mbtn_y && my <= mbtn_y+mbtn_h;
                    fill_rrect(pm, bx, mbtn_y, mbtn_w, mbtn_h, 4.0,
                               parse_color(&theme.accent, if hov {0.35} else {0.16}));
                    let lfs = if *lbl == "> " || *lbl == "||" { 10.0f32 } else { 9.0f32 };
                    let lw2 = self.text.measure(lbl, lfs);
                    self.text.draw(pm, lbl, bx+mbtn_w/2.0-lw2/2.0, mbtn_y+mbtn_h/2.0-lfs/2.0,
                                   lfs, parse_color(&theme.accent, if hov {1.0} else {0.72}));
                    new_bar_btns.push(ButtonRect { x: bx, y: mbtn_y, w: mbtn_w, h: mbtn_h,
                                                   action: action.clone() });
                }
                cy += card_h + 10.0;
            }

            // Connectivity
            let sec_fs = 9.0f32;
            self.text.draw(pm, "CONNECTIVITY", c2_x, cy, sec_fs, parse_color(&theme.text, 0.30));
            cy += sec_fs + 5.0;
            {
                let tile_h = 48.0f32;
                let tile_w = (col_w - 6.0) / 2.0;

                let tiles: &[(&str, &str, bool, WindowAction)] = &[
                    ("WiFi",
                     &sys.wifi_ssid.as_deref()
                         .map(|s| truncate(s, 10))
                         .unwrap_or_else(|| "Off".into()),
                     sys.wifi_ssid.is_some(),
                     WindowAction::WifiToggle),
                    ("Bluetooth",
                     if sys.bt_on { "On" } else { "Off" },
                     sys.bt_on,
                     WindowAction::BtToggle),
                ];

                for (i, (label, value, active, action)) in tiles.iter().enumerate() {
                    let tx = c2_x + i as f32 * (tile_w + 6.0);
                    let hov = mx >= tx && mx <= tx+tile_w && my >= cy && my <= cy+tile_h;
                    let accent = parse_color(&theme.accent, if *active { if hov {0.35} else {0.22} }
                                                            else if hov {0.18} else {0.10});
                    let border = parse_color(&theme.accent, if *active {0.70} else {0.20});
                    fill_rrect(pm, tx-1.0, cy-1.0, tile_w+2.0, tile_h+2.0, r+1.0, border);
                    fill_rrect(pm, tx, cy, tile_w, tile_h, r, accent);
                    self.text.draw(pm, label, tx+8.0, cy+7.0, 8.0, parse_color(&theme.text, 0.42));
                    let vw = self.text.measure(value, 10.0);
                    let vc = if *active { parse_color(&theme.accent, 1.0) }
                             else { parse_color(&theme.text, 0.38) };
                    self.text.draw(pm, value, tx+tile_w/2.0-vw/2.0, cy+tile_h/2.0-5.0+3.0,
                                   10.0, vc);
                    new_bar_btns.push(ButtonRect { x: tx, y: cy, w: tile_w, h: tile_h,
                                                   action: action.clone() });
                }
            }
        }

        draw_col_sep(pm, c2_x + col_w + col_gap/2.0);

        // ═══════════════════════════════════════════════════════════════════
        // COLUMN 3: System stats
        // ═══════════════════════════════════════════════════════════════════
        let c3_x = c2_x + col_w + col_gap;
        {
            let mut cy = pad;
            let sec_fs = 9.0f32;
            self.text.draw(pm, "SYSTEM", c3_x, cy, sec_fs, parse_color(&theme.text, 0.30));
            cy += sec_fs + 5.0;

            let row_h = 22.0f32; let bar_gap = 7.0f32;
            let lbl_w = 50.0f32; let stat_fs = 9.0f32;
            let val_w = 30.0f32;
            let track_x = c3_x + lbl_w + 4.0;
            let track_w = col_w - lbl_w - 4.0 - val_w;
            let val_x = track_x + track_w + 4.0;
            let bar_y_off = row_h/2.0 - 4.0;
            let bar_h_px = 7.0f32;

            let draw_stat = |pm: &mut Pixmap,
                                  text: &mut crate::text::TextRenderer,
                                  label: &str, pct: f32, col: Color, cy: f32| {
                text.draw(pm, label, c3_x, cy + row_h/2.0 - stat_fs/2.0,
                          stat_fs, parse_color(&theme.text, 0.42));
                fill_rrect(pm, track_x, cy+bar_y_off, track_w, bar_h_px, bar_h_px/2.0,
                           parse_color(&theme.border, 0.20));
                let fw = (track_w * pct.clamp(0.0,100.0) / 100.0).max(0.0);
                if fw >= 1.0 { fill_rrect(pm, track_x, cy+bar_y_off, fw, bar_h_px, bar_h_px/2.0, col); }
                let pct_s = format!("{:.0}%", pct);
                let pw = text.measure(&pct_s, stat_fs);
                text.draw(pm, &pct_s, val_x + val_w - pw, cy + row_h/2.0 - stat_fs/2.0,
                          stat_fs, col);
            };

            let cpu = sys.cpu_pct;
            let cpu_col = if cpu > 80.0 { Color::from_rgba8(243,139,168,210) }
                          else if cpu > 50.0 { Color::from_rgba8(250,179,135,210) }
                          else { parse_color(&theme.accent, 0.85) };
            let cpu_lbl = if let Some(t) = sys.cpu_temp_c {
                format!("CPU {:.0}C", t)
            } else { "CPU".into() };
            draw_stat(pm, &mut self.text, &cpu_lbl, cpu, cpu_col, cy);
            cy += row_h + bar_gap;

            if let Some(gt) = sys.gpu_temp_c {
                let gpu_pct = (gt / 100.0 * 100.0).clamp(0.0, 100.0);
                let gpu_col = if gt > 85.0 { Color::from_rgba8(243,139,168,210) }
                              else if gt > 65.0 { Color::from_rgba8(250,179,135,210) }
                              else { Color::from_rgba8(137,220,235,210) };
                let gpu_lbl = format!("GPU {:.0}C", gt);
                draw_stat(pm, &mut self.text, &gpu_lbl, gpu_pct, gpu_col, cy);
                cy += row_h + bar_gap;
            }

            let ram_pct = if sys.mem_total_kb > 0 {
                sys.mem_used_kb as f32 / sys.mem_total_kb as f32 * 100.0
            } else { 0.0 };
            let ram_col = if ram_pct > 85.0 { Color::from_rgba8(243,139,168,210) }
                          else { parse_color(&theme.text, 0.52) };
            let ram_lbl = format!("RAM {:.1}G", sys.mem_used_kb as f32 / (1024.0*1024.0));
            draw_stat(pm, &mut self.text, &ram_lbl, ram_pct, ram_col, cy);
            cy += row_h + bar_gap;

            let vol_pct = sys.volume_pct.unwrap_or(0.0);
            draw_stat(pm, &mut self.text, "VOL", vol_pct, parse_color(&theme.accent, 0.65), cy);
            cy += row_h + bar_gap + 8.0;

            // Panel-slot plugin widgets
            let panel_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
                .filter(|(d, _)| d.slot == WidgetSlot::Panel)
                .cloned().collect();
            if !panel_widgets.is_empty() {
                self.text.draw(pm, "WIDGETS", c3_x, cy, sec_fs, parse_color(&theme.text, 0.30));
                cy += sec_fs + 5.0;
                for (def, cmds) in &panel_widgets {
                    let wh = def.height as f32;
                    self.execute_draw_cmds(pm, c3_x, cy, col_w, wh, cmds, &theme.clone());
                    cy += wh + 6.0;
                }
            }
        }

        draw_col_sep(pm, c3_x + col_w + col_gap/2.0);

        // ═══════════════════════════════════════════════════════════════════
        // COLUMN 4: Power
        // ═══════════════════════════════════════════════════════════════════
        let c4_x = c3_x + col_w + col_gap;
        {
            let mut cy = pad;
            let sec_fs = 9.0f32;
            self.text.draw(pm, "POWER", c4_x, cy, sec_fs, parse_color(&theme.text, 0.30));
            cy += sec_fs + 5.0;

            let pbtn_h = 30.0f32; let pbtn_fs = 9.5f32; let gap = 5.0f32;
            let all_btns: &[(&str, WindowAction, [u8;3])] = &[
                ("Suspend",  WindowAction::PowerSuspend,  [166,227,161]),
                ("Lock",     WindowAction::PowerLock,     [137,180,250]),
                ("Reboot",   WindowAction::PowerReboot,   [250,179,135]),
                ("Logout",   WindowAction::PowerLogout,   [203,166,247]),
                ("Shutdown", WindowAction::PowerShutdown, [243,139,168]),
            ];
            // 2 per row
            let per_row = 2;
            for (idx, (lbl, action, col)) in all_btns.iter().enumerate() {
                let col_idx = idx % per_row;
                let bw = (col_w - gap) / per_row as f32;
                let bx = c4_x + col_idx as f32 * (bw + gap);
                let hov = mx >= bx && mx <= bx+bw && my >= cy && my <= cy+pbtn_h;
                fill_rrect(pm, bx, cy, bw, pbtn_h, 6.0,
                           Color::from_rgba8(col[0],col[1],col[2], if hov {60} else {22}));
                fill_rrect(pm, bx, cy, 3.0, pbtn_h, 3.0,
                           Color::from_rgba8(col[0],col[1],col[2], if hov {200} else {110}));
                let tw2 = self.text.measure(lbl, pbtn_fs);
                self.text.draw(pm, lbl, bx+bw/2.0-tw2/2.0+2.0, cy+pbtn_h/2.0-pbtn_fs/2.0,
                               pbtn_fs, Color::from_rgba8(col[0],col[1],col[2],
                                                           if hov {240} else {170}));
                new_bar_btns.push(ButtonRect { x: bx, y: cy, w: bw, h: pbtn_h,
                                               action: action.clone() });
                if col_idx == per_row - 1 || idx == all_btns.len() - 1 {
                    cy += pbtn_h + gap;
                }
            }
        }

        self.bar_cards   = new_bar_cards;
        self.bar_buttons = new_bar_btns;
    }

    pub fn on_bar_press(&self, x: f64, y: f64) -> Option<WindowAction> {
        let (mx, my) = (x as f32, y as f32);
        for btn in &self.bar_buttons {
            if btn.hit(mx, my) { return Some(btn.action.clone()); }
        }
        for card in &self.bar_cards {
            if card.hit(mx, my) {
                let ws = self.workspaces.get(card.ws_idx)?;
                return Some(WindowAction::FocusWorkspace(ws.id));
            }
        }
        None
    }

    pub fn on_bar_motion(&mut self, x: f64, y: f64) {
        self.bar_mouse_x = x as f32;
        self.bar_mouse_y = y as f32;
    }

    // ── app icon ──────────────────────────────────────────────────────────────

    /// Execute a plugin widget's draw command list onto `pm`, offset by (ox, oy).
    /// Commands use canvas-local coordinates (0,0 = top-left of the widget area).
    #[allow(clippy::too_many_arguments)]
    fn execute_draw_cmds(
        &mut self, pm: &mut Pixmap,
        ox: f32, oy: f32, canvas_w: f32, canvas_h: f32,
        cmds: &[DrawCmd], theme: &Theme,
    ) {
        // Clip guard — don't paint outside the canvas rect.
        let x1 = ox; let y1 = oy; let x2 = ox + canvas_w; let y2 = oy + canvas_h;
        if canvas_w < 1.0 || canvas_h < 1.0 { return; }

        for cmd in cmds {
            match cmd {
                DrawCmd::Clear { color, alpha } => {
                    let col = parse_color(color, *alpha);
                    fill_rrect(pm, x1, y1, canvas_w, canvas_h, 4.0, col);
                }
                DrawCmd::FillRect { x, y, w, h, color, alpha, radius } => {
                    let px = (ox + x).max(x1); let py = (oy + y).max(y1);
                    let pw = (w.min(x2 - px)).max(0.0);
                    let ph = (h.min(y2 - py)).max(0.0);
                    if pw > 0.0 && ph > 0.0 {
                        fill_rrect(pm, px, py, pw, ph, *radius, parse_color(color, *alpha));
                    }
                }
                DrawCmd::Text { content, x, y, size, color, alpha } => {
                    let px = ox + x; let py = oy + y;
                    if px < x2 && py < y2 {
                        self.text.draw(pm, content, px, py, *size, parse_color(color, *alpha));
                    }
                }
                DrawCmd::TextCentered { content, y, size, color, alpha } => {
                    let tw  = self.text.measure(content, *size);
                    let px  = ox + (canvas_w / 2.0 - tw / 2.0).max(0.0);
                    let py  = oy + y;
                    if px < x2 && py < y2 {
                        self.text.draw(pm, content, px, py, *size, parse_color(color, *alpha));
                    }
                }
                DrawCmd::AppIcon { class, x, y, size } => {
                    let px = if *x < 0.0 { ox + canvas_w / 2.0 - size / 2.0 } else { ox + x };
                    let py = oy + y;
                    // Render icon at full `size` — skip draw_app_icon's 0.45 scaling.
                    if let Some((iw, ih, rgba)) = self.icons.get(class).cloned() {
                        draw_icon_rgba(pm, &rgba, iw, ih, px, py, *size, *size);
                    } else {
                        let col = self.app_color(class);
                        fill_circle(pm, px + size/2.0, py + size/2.0, size/2.0,
                                    with_alpha(col, 0.22));
                        let letter = class.chars().find(|c| c.is_alphabetic())
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_else(|| "?".into());
                        let lfs = size * 0.55;
                        let lw  = self.text.measure(&letter, lfs);
                        self.text.draw(pm, &letter,
                                       px + size/2.0 - lw/2.0, py + size/2.0 - lfs/2.0,
                                       lfs, with_alpha(col, 0.85));
                    }
                }
                DrawCmd::Circle { cx, cy, r, color, alpha } => {
                    let px = ox + cx; let py = oy + cy;
                    if px >= x1 && px <= x2 && py >= y1 && py <= y2 {
                        fill_circle(pm, px, py, *r, parse_color(color, *alpha));
                    }
                }
            }
        }

        // Subtle border so the widget area is visually separated.
        if !cmds.is_empty() {
            fill_rrect(pm, x1-0.5, y1-0.5, canvas_w+1.0, canvas_h+1.0, 4.5,
                       parse_color(&theme.border, 0.12));
        }
    }

    fn draw_app_icon(&mut self, pm: &mut Pixmap, class: &str, cx: f32, cy: f32, cw: f32, ch: f32) {
        let icon_size = (ch * 0.45).clamp(24.0, 72.0) as u32;
        let ix = cx + cw/2.0 - icon_size as f32/2.0;
        let iy = cy + ch/2.0 - icon_size as f32/2.0;
        if let Some((iw, ih, rgba)) = self.icons.get(class).cloned() {
            draw_icon_rgba(pm, &rgba, iw, ih, ix, iy, icon_size as f32, icon_size as f32);
            return;
        }
        // Letter fallback — no glyphs, no dollar signs
        let col = self.app_color(class);
        fill_circle(pm, cx+cw/2.0, cy+ch/2.0, icon_size as f32/2.0, with_alpha(col, 0.22));
        let letter = class.chars().find(|c| c.is_alphabetic())
        .map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        let lfs = icon_size as f32 * 0.55;
        let lw  = self.text.measure(&letter, lfs);
        self.text.draw(pm, &letter, cx+cw/2.0-lw/2.0, cy+ch/2.0-lfs/2.0, lfs,
                       with_alpha(col, 0.75));
    }

    // ── widget bar ────────────────────────────────────────────────────────────

    fn draw_widget_bar(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        #[allow(non_snake_case)] let WIDGET_H = self.layout.widget_bar_height;
        let slide = self.anim_slide; let bar_y = sh - WIDGET_H + slide;
        fill_rect(pm, 0.0, bar_y-1.0, sw, 1.0, parse_color(&theme.border, 0.15));
        fill_rect(pm, 0.0, bar_y, sw, WIDGET_H, parse_color(&theme.background, 0.55));
        let cy = bar_y + WIDGET_H/2.0; let pad = 20.0f32;
        let now_local = chrono::Local::now();
        let clock_str = now_local.format("%H:%M").to_string();
        let date_str  = now_local.format("%a %d %b").to_string();
        let clock_fs = 22.0f32;
        let cw2 = self.text.measure(&clock_str, clock_fs);
        self.text.draw(pm, &clock_str, pad, cy-clock_fs/2.0, clock_fs,
                       parse_color(&theme.text, 0.90));
        let date_fs = 11.0f32;
        self.text.draw(pm, &date_str, pad+cw2+10.0, cy-date_fs/2.0, date_fs,
                       parse_color(&theme.text, 0.45));
        let workspaces = self.workspaces.clone();
        let sel = self.selected_ws.min(workspaces.len().saturating_sub(1));
        if let Some(ws) = workspaces.get(sel) {
            let cnt = format!("{} window{}", ws.windows.len(), if ws.windows.len()==1 {""} else {"s"});
            let cfs = 11.0f32;
            let rx = sw - pad - self.text.measure(&cnt, cfs);
            self.text.draw(pm, &cnt, rx, cy-cfs/2.0, cfs, parse_color(&theme.text, 0.35));
        }

        // Overlay-slot plugin widgets — fixed-width slots centered in the zone.
        let overlay_widgets: Vec<(WidgetDef, Vec<DrawCmd>)> = self.bar_widgets.iter()
            .filter(|(d, _)| d.slot == WidgetSlot::Overlay)
            .cloned().collect();
        self.launcher_zones.clear();
        if !overlay_widgets.is_empty() {
            let zone_x  = 200.0f32;
            let zone_w  = (sw - 400.0).max(100.0);
            let wh      = WIDGET_H - 6.0;
            let n       = overlay_widgets.len() as f32;
            // Cap each slot at 260px wide; centre the group within the zone.
            let slot_w  = (zone_w / n).min(260.0).floor();
            let group_w = slot_w * n;
            let group_x = (zone_x + (zone_w - group_w) / 2.0).floor();
            let theme2  = theme.clone();
            for (i, (def, cmds)) in overlay_widgets.iter().enumerate() {
                let wx = group_x + i as f32 * slot_w;
                let wy = bar_y + 3.0;
                self.execute_draw_cmds(pm, wx, wy, slot_w - 8.0, wh, cmds, &theme2);
                if let Some(cmd) = &def.onclick_cmd {
                    self.launcher_zones.push(LaunchZone {
                        x: wx, y: wy, w: slot_w - 8.0, h: wh,
                        cmd: cmd.clone(),
                    });
                }
            }
        }
    }

    // ── error toast ───────────────────────────────────────────────────────────

    fn draw_toast(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, msg: String, theme: &Theme) {
        let pad   = 16.0_f32;
        let h     = 44.0_f32;
        let w     = (sw - pad * 6.0).min(640.0_f32);
        let x     = (sw - w) / 2.0;
        let y     = sh - h - pad * 2.0;
        // dark red-tinted background
        let bg    = parse_color("#2a1520", 0.96);
        let border = parse_color("#f38ba8", 1.0); // red accent (catppuccin red)
        fill_rrect(pm, x, y, w, h, 8.0, bg);
        // 2px border
        fill_rrect(pm, x,     y,     w,   2.0, 0.0, border);
        fill_rrect(pm, x,     y+h-2.0, w, 2.0, 0.0, border);
        fill_rrect(pm, x,     y,     2.0, h,   0.0, border);
        fill_rrect(pm, x+w-2.0, y,   2.0, h,   0.0, border);
        // icon
        let icon_col = parse_color("#f38ba8", 1.0);
        self.text.draw(pm, "⚠", x + pad, y + h / 2.0 + 5.0, 15.0, icon_col);
        // message (truncated to fit)
        let max_chars = ((w - pad * 3.5 - 16.0) / 7.5) as usize;
        let label = truncate(&msg, max_chars.max(20));
        let text_col = parse_color(&theme.text, 0.95);
        self.text.draw(pm, &label, x + pad + 22.0, y + h / 2.0 + 5.0, 12.5, text_col);
    }
}

// ── workspace placeholder ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_ws_placeholder(
    pm: &mut Pixmap, text: &mut TextRenderer,
    tx: f32, ty: f32, tw: f32, th: f32, ws: &Workspace, theme: &Theme,
) {
    let num_s = format!("{}", ws.id); let num_fs = (th*0.35).min(36.0);
    let num_w = text.measure(&num_s, num_fs);
    text.draw(pm, &num_s, tx+tw/2.0-num_w/2.0, ty+th/2.0-num_fs/2.0-6.0, num_fs,
              parse_color(&theme.border, 0.25));
    if !ws.windows.is_empty() {
        let cnt_s = format!("{} win", ws.windows.len()); let cnt_fs = 9.0f32;
        let cnt_w = text.measure(&cnt_s, cnt_fs);
        text.draw(pm, &cnt_s, tx+tw/2.0-cnt_w/2.0, ty+th/2.0+num_fs/2.0-4.0, cnt_fs,
                  parse_color(&theme.text, 0.30));
    }
}

// ── date helpers ──────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn epoch_ymd(epoch: u64) -> (u32, u32, u32) {
    let mut d = (epoch/86400) as u32; let mut y = 1970u32;
    loop {
        let dy = if y.is_multiple_of(4) && (!y.is_multiple_of(100)||y.is_multiple_of(400)) {366} else {365};
        if d < dy { break; } d -= dy; y += 1;
    }
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100)||y.is_multiple_of(400));
    let ml = [31u32, if leap {29} else {28}, 31,30,31,30,31,31,30,31,30,31];
    let mut mo = 1u32;
    for mlen in ml { if d < mlen { break; } d -= mlen; mo += 1; }
    (y, mo, d+1)
}
#[allow(dead_code)]
fn month_abbr(mo: u32) -> &'static str {
    ["","Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"]
    .get(mo as usize).copied().unwrap_or("???")
}

// ── primitives ────────────────────────────────────────────────────────────────

fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if w<0.5||h<0.5||!x.is_finite()||!y.is_finite() { return; }
    let mut p = Paint::default(); p.set_color(c);
    if let Some(r) = Rect::from_xywh(x,y,w,h) { pm.fill_rect(r, &p, Transform::identity(), None); }
}

fn fill_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if w<1.0||h<1.0||!x.is_finite()||!y.is_finite() { return; }
    let r = r.min(w/2.0).min(h/2.0).max(0.0);
    if r<1.5||w<6.0||h<6.0 { fill_rect(pm,x,y,w,h,c); return; }
    let mut p = Paint::default(); p.set_color(c); p.anti_alias=false;
    if let Some(path) = rrect_path(x,y,w,h,r) {
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    if r<0.5||!cx.is_finite()||!cy.is_finite() { return; }
    let mut p = Paint::default(); p.set_color(c); p.anti_alias=false;
    if let Some(path) = PathBuilder::from_circle(cx,cy,r) {
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    b.move_to(x+r,y); b.line_to(x+w-r,y); b.quad_to(x+w,y, x+w,y+r);
    b.line_to(x+w,y+h-r); b.quad_to(x+w,y+h, x+w-r,y+h);
    b.line_to(x+r,y+h); b.quad_to(x,y+h, x,y+h-r);
    b.line_to(x,y+r); b.quad_to(x,y, x+r,y);
    b.close(); b.finish()
}

fn parse_color(hex: &str, alpha: f32) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len()<6 { return Color::from_rgba8(30,30,46,(alpha*255.0)as u8); }
    let r = u8::from_str_radix(&hex[0..2],16).unwrap_or(30);
    let g = u8::from_str_radix(&hex[2..4],16).unwrap_or(30);
    let b = u8::from_str_radix(&hex[4..6],16).unwrap_or(46);
    Color::from_rgba8(r, g, b, (alpha*255.0)as u8)
}

fn class_color(class: &str) -> Color {
    let h = class.bytes().fold(5381u32, |a,b| a.wrapping_mul(33).wrapping_add(b as u32));
    Color::from_rgba8(
        100u8.saturating_add(((h>>16)&0x9F)as u8),
        100u8.saturating_add(((h>> 8)&0x9F)as u8),
        100u8.saturating_add(((h    )&0x9F)as u8), 255,
    )
}

fn with_alpha(mut c: Color, a: f32) -> Color { c.set_alpha(a); c }

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count()<=n { s.to_string() }
    else { format!("{}…", s.chars().take(n-1).collect::<String>()) }
}

fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) { out.push(px[2]); out.push(px[1]); out.push(px[0]); out.push(px[3]); }
    out
}

/// Blit XRGB8888 screenshot at reduced brightness as overlay backdrop.
#[allow(clippy::too_many_arguments)]
fn draw_thumbnail_dimmed(
    pm: &mut Pixmap, src: &[u8], sw: u32, sh: u32,
    bx: f32, by: f32, bw: f32, bh: f32, dim: f32,
) {
    if sw==0||sh==0||bw<1.0||bh<1.0 { return; }
    let pw=pm.width()as i32; let ph=pm.height()as i32;
    let ibw=bw as i32; let ibh=bh as i32;
    let ox=bx as i32; let oy=by as i32;
    let dim_u=(dim*255.0)as u32;
    let pixels=pm.pixels_mut();
    for dy in 0..ibh {
        let py=oy+dy; if py<0||py>=ph { continue; }
        for dx in 0..ibw {
            let px=ox+dx; if px<0||px>=pw { continue; }
            let sx=(dx as f32/ibw as f32*sw as f32)as u32;
            let sy=(dy as f32/ibh as f32*sh as f32)as u32;
            let si=((sy*sw+sx)*4)as usize;
            if si+3>=src.len() { continue; }
            // XRGB8888 LE: [B,G,R,X]
            let b=(src[si]   as u32*dim_u/255)as u8;
            let g=(src[si+1] as u32*dim_u/255)as u8;
            let r=(src[si+2] as u32*dim_u/255)as u8;
            let di=(py*pw+px)as usize;
            if di<pixels.len() {
                pixels[di]=tiny_skia::PremultipliedColorU8::from_rgba(r,g,b,255).unwrap_or(pixels[di]);
            }
        }
    }
}

/// Blit XRGB8888 thumbnail with rounded-corner clip.
#[allow(clippy::too_many_arguments)]
fn draw_thumbnail_clipped(
    pm: &mut Pixmap, src: &[u8], sw: u32, sh: u32,
    bx: f32, by: f32, bw: f32, bh: f32, r: f32,
) {
    if sw==0||sh==0||bw<1.0||bh<1.0 { return; }
    let pw=pm.width()as i32; let ph=pm.height()as i32;
    let ibw=bw as i32; let ibh=bh as i32;
    let ox=bx as i32; let oy=by as i32; let r2=r*r;
    let pixels=pm.pixels_mut();
    for dy in 0..ibh {
        let py=oy+dy; if py<0||py>=ph { continue; }
        for dx in 0..ibw {
            let px=ox+dx; if px<0||px>=pw { continue; }
            let fx=dx as f32; let fy=dy as f32;
            if r>=1.5 {
                let in_c=|cx:f32,cy:f32|{let ddx=fx-cx;let ddy=fy-cy;ddx*ddx+ddy*ddy>r2};
                if (fx<r&&fy<r&&in_c(r,r))||(fx>bw-r-1.0&&fy<r&&in_c(bw-r-1.0,r))
                ||(fx<r&&fy>bh-r-1.0&&in_c(r,bh-r-1.0))||(fx>bw-r-1.0&&fy>bh-r-1.0&&in_c(bw-r-1.0,bh-r-1.0))
                { continue; }
            }
            let sx=(dx as f32/ibw as f32*sw as f32)as u32;
            let sy=(dy as f32/ibh as f32*sh as f32)as u32;
            let si=((sy*sw+sx)*4)as usize;
            if si+3>=src.len() { continue; }
            let b=src[si]; let g=src[si+1]; let rb=src[si+2];
            let di=(py*pw+px)as usize;
            if di<pixels.len() {
                pixels[di]=tiny_skia::PremultipliedColorU8::from_rgba(rb,g,b,255).unwrap_or(pixels[di]);
            }
        }
    }
}

/// Blit RGBA icon with source-over alpha compositing.
#[allow(clippy::too_many_arguments)]
fn draw_icon_rgba(
    pm: &mut Pixmap, src: &[u8], sw: u32, sh: u32,
    bx: f32, by: f32, bw: f32, bh: f32,
) {
    if sw==0||sh==0||bw<1.0||bh<1.0 { return; }
    let pw=pm.width()as i32; let ph=pm.height()as i32;
    let ibw=bw as i32; let ibh=bh as i32;
    let ox=bx as i32; let oy=by as i32;
    let pixels=pm.pixels_mut();
    for dy in 0..ibh {
        let py=oy+dy; if py<0||py>=ph { continue; }
        for dx in 0..ibw {
            let px=ox+dx; if px<0||px>=pw { continue; }
            let sx=(dx as f32/ibw as f32*sw as f32)as u32;
            let sy=(dy as f32/ibh as f32*sh as f32)as u32;
            let si=((sy*sw+sx)*4)as usize;
            if si+3>=src.len() { continue; }
            let sr=src[si]; let sg=src[si+1]; let sb=src[si+2]; let sa=src[si+3];
            if sa==0 { continue; }
            let di=(py*pw+px)as usize;
            if di>=pixels.len() { continue; }
            let inv_a=255-sa as u32; let dst=pixels[di];
            let out_r=((sr as u32*sa as u32/255)+dst.red()   as u32*inv_a/255)as u8;
            let out_g=((sg as u32*sa as u32/255)+dst.green() as u32*inv_a/255)as u8;
            let out_b=((sb as u32*sa as u32/255)+dst.blue()  as u32*inv_a/255)as u8;
            let out_a=(sa as u32+dst.alpha() as u32*inv_a/255)as u8;
            pixels[di]=tiny_skia::PremultipliedColorU8::from_rgba(out_r,out_g,out_b,out_a).unwrap_or(dst);
        }
    }
}
