//! Scans `~/.config/woven/plugins/` and collects plugin directories.
//! Actual Lua execution happens in woven-sys (which owns the mlua runtime).
//! The loader's job is purely discovery — finding which plugin dirs exist.

use std::path::{Path, PathBuf};
use tracing::warn;

/// Returns a list of plugin directories found under `plugins_dir`.
/// Each entry is the path to a directory that contains an `init.lua`.
pub fn scan_plugin_dirs(plugins_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return dirs; // plugins dir doesn't exist — that's fine
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let init = path.join("init.lua");
        if init.exists() {
            dirs.push(path);
        } else {
            warn!("plugin dir {} has no init.lua — skipping", path.display());
        }
    }

    dirs.sort(); // deterministic load order
    dirs
}
