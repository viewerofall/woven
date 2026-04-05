use serde::{Deserialize, Serialize};

/// A compositor workspace / virtual desktop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id:      u32,
    pub name:    String,
    pub active:  bool,
    pub windows: Vec<Window>,
}

/// A single window / toplevel surface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub id:          String,   // compositor-assigned id
    pub pid:         Option<u32>,
    pub class:       String,
    pub title:       String,
    pub workspace:   u32,
    pub fullscreen:  bool,
    pub floating:    bool,
    pub xwayland:    bool,     // true = XWayland surface
    pub geometry:    Rect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Per-process resource snapshot from /proc
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessMetrics {
    pub pid:        u32,
    pub cpu_pct:    f32,   // 0.0 - 100.0 per core
    pub mem_kb:     u64,
}

/// Aggregated metrics for an entire workspace
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceMetrics {
    pub workspace_id: u32,
    pub cpu_total:    f32,
    pub mem_total_kb: u64,
    pub procs:        Vec<ProcessMetrics>,
}

/// Theme values parsed from Lua — Rust owns rendering, Lua owns these values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub background:    String,
    pub border:        String,
    pub text:          String,
    pub accent:        String,
    pub border_radius: u32,
    pub font:          String,
    pub font_size:     u32,
    pub opacity:       f32,
    pub blur:          bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background:    "#1e1e2e".into(),
            border:        "#cba6f7".into(),
            text:          "#cdd6f4".into(),
            accent:        "#89b4fa".into(),
            border_radius: 12,
            font:          "JetBrainsMono Nerd Font".into(),
            font_size:     13,
            opacity:       0.92,
            blur:          true,
        }
    }
}

/// Animation descriptor — Lua declares these, Rust executes them
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationDef {
    pub curve:       EasingCurve,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EasingCurve {
    Linear,
    EaseOutCubic,
    EaseInCubic,
    EaseInOutCubic,
    Spring { tension: f32 },
}

impl Default for AnimationDef {
    fn default() -> Self {
        Self {
            curve:       EasingCurve::EaseOutCubic,
            duration_ms: 180,
        }
    }
}

/// Persistent workspace bar configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarConfig {
    #[serde(default = "default_true")]
    pub enabled:  bool,
    #[serde(default)]
    pub position: BarPosition,
}

impl Default for BarConfig {
    fn default() -> Self { Self { enabled: false, position: BarPosition::Right } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BarPosition { Left, #[default] Right, Top, Bottom }

fn default_true() -> bool { true }

/// Full animation config handed from Lua to Rust once at startup
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnimationConfig {
    pub overlay_open:  AnimationDef,
    pub overlay_close: AnimationDef,
    pub scroll:        AnimationDef,
}

/// A single drawing operation produced by a plugin widget's `render` function.
/// The render thread executes these using its own text + skia stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawCmd {
    /// Fill the whole canvas with a solid color.
    Clear { color: String, alpha: f32 },
    /// Draw a filled rounded rect.
    FillRect { x: f32, y: f32, w: f32, h: f32, color: String, alpha: f32, radius: f32 },
    /// Draw a line of text.
    Text { content: String, x: f32, y: f32, size: f32, color: String, alpha: f32 },
    /// Draw a line of text horizontally centered in the widget canvas (x ignored).
    TextCentered { content: String, y: f32, size: f32, color: String, alpha: f32 },
    /// Draw a circle.
    Circle { cx: f32, cy: f32, r: f32, color: String, alpha: f32 },
    /// Draw an app icon (by wm_class) centered in a box at (x, y) with given size.
    /// Pass x = -1.0 to auto-center horizontally in the canvas.
    AppIcon { class: String, x: f32, y: f32, size: f32 },
}

/// Which section of the bar a widget occupies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSlot {
    /// Above workspace cards (vertical bar) / left side (horizontal bar).
    Top,
    /// Between workspace cards and system info (vertical bar) / right side (horizontal bar).
    #[default]
    Bottom,
    /// Expanded bar (control-center panel) — full inner width below SYSTEM stats.
    Panel,
    /// Overlay bottom widget strip — center zone of the horizontal strip.
    Overlay,
}

/// Registered widget definition — sent once on registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetDef {
    /// Unique widget id (arbitrary string set by the plugin).
    pub id:           String,
    pub slot:         WidgetSlot,
    /// Canvas height in logical pixels (for vertical bar).
    pub height:       u32,
    /// Re-render interval in seconds (0 = render once).
    pub interval_secs: u32,
    /// Optional shell command to spawn when the widget is clicked (overlay slot only).
    pub onclick_cmd:  Option<String>,
}

/// Layout dimensions configurable via `woven.layout({})` in the user's config.
/// All values are logical pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    /// Height of the top info bar                  (default 48)
    pub top_bar_height:    f32,
    /// Height of the workspace thumbnail strip      (default 148)
    pub ws_strip_height:   f32,
    /// Height of the bottom widget bar              (default 56)
    pub widget_bar_height: f32,
    /// Outer horizontal/vertical padding            (default 20)
    pub outer_padding:     f32,
    /// Gap between workspace thumbnails             (default 12)
    pub strip_gap:         f32,
    /// Width of each workspace thumbnail card       (default 200)
    pub ws_thumb_width:    f32,
    /// Height of each workspace thumbnail card      (default 110)
    pub ws_thumb_height:   f32,
    /// Height of the expand/view button on ws cards (default 18)
    pub ws_btn_height:     f32,
    /// Inner padding inside window cards            (default 16)
    pub card_padding:      f32,
    /// Gap between window cards in the grid         (default 12)
    pub card_gap:          f32,
    /// Fraction of card height used for screenshot  (default 0.65)
    pub card_thumb_ratio:  f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            top_bar_height:    48.0,
            ws_strip_height:   148.0,
            widget_bar_height: 56.0,
            outer_padding:     20.0,
            strip_gap:         12.0,
            ws_thumb_width:    200.0,
            ws_thumb_height:   110.0,
            ws_btn_height:     18.0,
            card_padding:      16.0,
            card_gap:          12.0,
            card_thumb_ratio:  0.65,
        }
    }
}
