//! Collected plugin registrations — populated by the loader, consumed by renderers.

use std::collections::HashMap;
use crate::manifest::PluginManifest;

/// Mapping from app class name (lowercase) to absolute icon file path.
/// Built up by icon-pack plugins during their `init.lua` execution.
pub type IconMap = HashMap<String, String>;

/// Everything woven collected from all loaded plugins.
#[derive(Debug, Default, Clone)]
pub struct PluginRegistry {
    /// All successfully loaded plugin manifests.
    pub plugins: Vec<PluginManifest>,
    /// Merged icon map — later plugins override earlier ones for the same class.
    pub icons: IconMap,
    /// Default icon path for classes not in `icons` (set by the active icon pack).
    pub default_icon: Option<String>,
}

impl PluginRegistry {
    pub fn new() -> Self { Self::default() }

    /// Merge an icon map from a plugin (later entries win).
    pub fn merge_icons(&mut self, map: IconMap, default: Option<String>) {
        self.icons.extend(map);
        if default.is_some() { self.default_icon = default; }
    }

    /// Look up an icon path by app class.
    pub fn icon_for(&self, class: &str) -> Option<&str> {
        self.icons.get(&class.to_lowercase())
            .map(String::as_str)
            .or(self.default_icon.as_deref())
    }
}
