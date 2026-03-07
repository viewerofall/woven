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

/// Full animation config handed from Lua to Rust once at startup
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnimationConfig {
    pub overlay_open:  AnimationDef,
    pub overlay_close: AnimationDef,
    pub scroll:        AnimationDef,
}
