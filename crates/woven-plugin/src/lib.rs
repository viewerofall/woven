pub mod loader;
pub mod manifest;
pub mod registry;

pub use loader::scan_plugin_dirs;
pub use manifest::{PluginManifest, PluginType};
pub use registry::PluginRegistry;
