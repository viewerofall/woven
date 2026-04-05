//! Plugin manifest — what a plugin declares about itself via `woven.plugin.register({})`.

use serde::{Deserialize, Serialize};

/// Declared plugin type — each type unlocks a different API surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    /// Provides app-class → image-file mappings.
    IconPack,
    /// Adds a widget to the persistent bar (left / center / right slot).
    BarWidget,
    /// Adds a panel section to the overlay.
    OverlayPanel,
    /// Provides a complete theme (color values).
    ThemeProvider,
}

/// Registered during plugin `init.lua` execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name:    String,
    pub version: String,
    pub kind:    PluginType,
    /// Absolute path to the plugin directory (set by the loader, not the plugin).
    pub dir:     String,
}
