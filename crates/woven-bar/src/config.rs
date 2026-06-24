//! woven-bar configuration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarConfig {
    #[serde(default = "enabled_default")]
    pub enabled: bool,

    #[serde(default = "default_height")]
    pub height: u32,

    #[serde(default)]
    pub position: BarPosition,

    #[serde(default)]
    pub style: BarStyle,

    #[serde(default)]
    pub theme: Theme,

    #[serde(default)]
    pub bubbles: BubbleSettings,

    #[serde(default)]
    pub modules: Modules,

    /// Where the bar reads its colors from.
    /// "config" (default) = use [theme] section.
    /// "wallpaper" = read from $XDG_RUNTIME_DIR/woven-theme.toml (written by woven-wall).
    #[serde(default)]
    pub theme_source: ThemeSource,

    #[serde(default)]
    pub weather: WeatherConfig,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            enabled:      true,
            height:       32,
            position:     BarPosition::Top,
            style:        BarStyle::Solid,
            theme:        Theme::default(),
            bubbles:      BubbleSettings::default(),
            modules:      Modules::default(),
            theme_source: ThemeSource::Config,
            weather:      WeatherConfig::default(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl BarPosition {
    pub fn is_vertical(&self) -> bool {
        matches!(self, BarPosition::Left | BarPosition::Right)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BarStyle {
    /// Traditional solid bar spanning the full width
    #[default]
    Solid,
    /// Separate rounded pill segments per module group
    Bubbles,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSource {
    /// Use colors from the [theme] section in bar.toml
    #[default]
    Config,
    /// Read colors from woven-wall's extracted wallpaper theme
    Wallpaper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BubbleSettings {
    /// Background color of each bubble pill (hex)
    pub background: String,
    /// Corner radius of each bubble
    pub radius: u32,
    /// Gap between adjacent bubbles
    pub gap: u32,
    /// Horizontal padding inside each bubble
    pub padding: u32,
    /// Vertical margin (top/bottom inset from bar edge)
    pub margin: u32,
}

impl Default for BubbleSettings {
    fn default() -> Self {
        Self {
            background: "#1a0a2e".into(),
            radius:     12,
            gap:        6,
            padding:    10,
            margin:     4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// Bar background color (hex)
    pub background: String,
    /// Primary text color (hex)
    pub foreground: String,
    /// Accent / active highlight color (hex)
    pub accent: String,
    /// Dimmed text / inactive color (hex)
    pub dim: String,
    /// Border radius for pill widgets (px)
    pub radius: u32,
    /// Font size in logical pixels
    pub font_size: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#0a0010".into(),
            foreground: "#cdd6f4".into(),
            accent:     "#c792ea".into(),
            dim:        "#4a3060".into(),
            radius:     6,
            font_size:  12.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherConfig {
    /// Latitude for weather lookup
    pub lat: f64,
    /// Longitude for weather lookup
    pub lon: f64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        // Falls back to env WOVEN_LAT / WOVEN_LON, or 0,0 (won't show useful data)
        let lat = std::env::var("WOVEN_LAT").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let lon = std::env::var("WOVEN_LON").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        Self { lat, lon }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Modules {
    pub left:   Vec<ModuleKind>,
    pub center: Vec<ModuleKind>,
    pub right:  Vec<ModuleKind>,
}

impl Default for Modules {
    fn default() -> Self {
        Self {
            left: vec![
                ModuleKind::Activities,
                ModuleKind::Workspaces,
            ],
            center: vec![
                ModuleKind::WindowTitle,
            ],
            right: vec![
                ModuleKind::Network,
                ModuleKind::Audio,
                ModuleKind::Battery,
                ModuleKind::Clock,
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModuleKind {
    Activities,
    Workspaces,
    WindowTitle,
    Network,
    Audio,
    Battery,
    Clock,
    Systray,
    Cpu,
    Memory,
    Disk,
    Temp,
    Media,
    Notifications,
    ControlCenter,
    Weather,
    Date,
    Cava,
    /// Bubble separator — splits modules into separate pill groups (bubbles mode only).
    /// Use "|" in module lists to create separate bubbles.
    #[serde(rename = "|")]
    Separator,
}

fn enabled_default() -> bool { true }
fn default_height() -> u32 { 32 }
