//! Compositor config detection and safe keybind injection.
//!
//! SAFETY CONTRACT: This module ONLY ever appends to existing files.
//! It never truncates, overwrites, or creates compositor config files.
//! If anything looks wrong it returns an error and does nothing.

use std::io::Write;

// ── Compositor definitions ────────────────────────────────────────────────────

pub struct CompositorDef {
    pub name:        &'static str,
    /// Binary to check for installation (checked via PATH)
    pub binary:      &'static str,
    /// Default config path relative to $HOME
    pub config_rel:  &'static str,
    /// Env var whose presence means this compositor is the running session
    pub session_env: &'static str,
    /// Lines to append for the keybind (toggle) — prepended with a blank line
    pub keybind_snippet: &'static str,
    /// Lines to append for autostart — prepended with a blank line
    pub autostart_snippet: &'static str,
}

pub const COMPOSITORS: &[CompositorDef] = &[
    CompositorDef {
        name:        "Hyprland",
        binary:      "hyprctl",
        config_rel:  ".config/hypr/hyprland.conf",
        session_env: "HYPRLAND_INSTANCE_SIGNATURE",
        keybind_snippet:   "\n# woven — workspace overlay toggle\nbind = SUPER, grave, exec, woven-ctrl --toggle\n",
        autostart_snippet: "\n# woven — start overlay daemon\nexec-once = woven\n",
    },
    CompositorDef {
        name:        "Niri",
        binary:      "niri",
        config_rel:  ".config/niri/config.kdl",
        session_env: "NIRI_SOCKET",
        keybind_snippet:   "\n// woven — workspace overlay toggle\nMod+Grave { spawn \"woven-ctrl\" \"--toggle\"; }\n",
        autostart_snippet: "\n// woven — start overlay daemon\nspawn-at-startup \"woven\"\n",
    },
    CompositorDef {
        name:        "Sway",
        binary:      "sway",
        config_rel:  ".config/sway/config",
        session_env: "SWAYSOCK",
        keybind_snippet:   "\n# woven — workspace overlay toggle\nbindsym $mod+grave exec woven-ctrl --toggle\n",
        autostart_snippet: "\n# woven — start overlay daemon\nexec woven\n",
    },
    CompositorDef {
        name:        "River",
        binary:      "riverctl",
        config_rel:  ".config/river/init",
        session_env: "RIVER_SESSION",
        keybind_snippet:   "\n# woven — workspace overlay toggle\nriverctl map normal Super grave spawn 'woven-ctrl --toggle'\n",
        autostart_snippet: "\n# woven — start overlay daemon\nriverctl spawn woven\n",
    },
];

// ── Per-compositor status ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CompositorStatus {
    pub name:             String,
    /// Binary found in PATH — compositor is installed
    pub installed:        bool,
    /// Config file exists at the expected path
    pub config_exists:    bool,
    /// Full expanded config path (for display)
    pub config_path:      String,
    /// `woven-ctrl --toggle` found in config
    pub keybind_present:  bool,
    /// `woven` found on an exec/spawn/autostart line in config
    pub autostart_present: bool,
    /// This compositor is the current running session
    pub is_running:       bool,
}

pub fn detect_all() -> Vec<CompositorStatus> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    COMPOSITORS.iter().map(|def| {
        let installed      = binary_in_path(def.binary);
        let config_path    = format!("{}/{}", home, def.config_rel);
        let config_exists  = std::path::Path::new(&config_path).exists();
        let is_running     = !std::env::var(def.session_env)
                                 .unwrap_or_default().is_empty();
        let (keybind_present, autostart_present) = if config_exists {
            scan_config(&config_path)
        } else {
            (false, false)
        };
        CompositorStatus {
            name:             def.name.into(),
            installed,
            config_exists,
            config_path,
            keybind_present,
            autostart_present,
            is_running,
        }
    }).collect()
}

/// Scan a config file for woven references.
/// Returns (keybind_present, autostart_present).
fn scan_config(path: &str) -> (bool, bool) {
    let Ok(content) = std::fs::read_to_string(path) else { return (false, false) };
    let keybind   = content.contains("woven-ctrl --toggle");
    let autostart = content.lines().any(|line| {
        let l = line.trim();
        // Match exec/spawn/autostart lines that reference `woven` but not `woven-ctrl`
        let is_exec = l.starts_with("exec")
            || l.starts_with("spawn")
            || l.starts_with("riverctl spawn")
            || l.starts_with("spawn-at-startup");
        is_exec && l.contains("woven") && !l.contains("woven-ctrl")
    });
    (keybind, autostart)
}

fn binary_in_path(bin: &str) -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|dir| std::path::Path::new(dir).join(bin).exists())
}

// ── Append-only injection ─────────────────────────────────────────────────────

/// Append the keybind snippet to the compositor config.
/// NEVER truncates or overwrites — opens in append mode only.
/// Returns Err if the file doesn't exist, already contains the keybind,
/// or the write fails.
pub fn inject_keybind(status: &CompositorStatus) -> Result<(), String> {
    if !status.config_exists {
        return Err(format!("Config file not found: {}", status.config_path));
    }
    if status.keybind_present {
        return Err("Keybind already present — nothing to do.".into());
    }
    let def = compositor_def(&status.name)
        .ok_or_else(|| format!("Unknown compositor: {}", status.name))?;

    // Double-check before writing — re-read the file right now
    let current = std::fs::read_to_string(&status.config_path)
        .map_err(|e| format!("Could not read {}: {e}", status.config_path))?;
    if current.contains("woven-ctrl --toggle") {
        return Err("Keybind already present — nothing to do.".into());
    }

    // Append only
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&status.config_path)
        .map_err(|e| format!("Could not open {} for appending: {e}", status.config_path))?;

    file.write_all(def.keybind_snippet.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    Ok(())
}

/// Append the autostart snippet to the compositor config.
/// Same safety contract as inject_keybind.
pub fn inject_autostart(status: &CompositorStatus) -> Result<(), String> {
    if !status.config_exists {
        return Err(format!("Config file not found: {}", status.config_path));
    }
    if status.autostart_present {
        return Err("Autostart already present — nothing to do.".into());
    }
    let def = compositor_def(&status.name)
        .ok_or_else(|| format!("Unknown compositor: {}", status.name))?;

    let current = std::fs::read_to_string(&status.config_path)
        .map_err(|e| format!("Could not read {}: {e}", status.config_path))?;
    // Extra guard: check any woven autostart reference, not just exact snippet
    if current.lines().any(|l| {
        let l = l.trim();
        (l.starts_with("exec") || l.starts_with("spawn") || l.starts_with("riverctl spawn")
            || l.starts_with("spawn-at-startup"))
            && l.contains("woven") && !l.contains("woven-ctrl")
    }) {
        return Err("Autostart already present — nothing to do.".into());
    }

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&status.config_path)
        .map_err(|e| format!("Could not open {} for appending: {e}", status.config_path))?;

    file.write_all(def.autostart_snippet.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    Ok(())
}

fn compositor_def(name: &str) -> Option<&'static CompositorDef> {
    COMPOSITORS.iter().find(|d| d.name == name)
}

/// Trigger a live config reload for the currently running compositor.
/// No-op if the compositor doesn't support it or isn't running.
pub fn reload_compositor(status: &CompositorStatus) {
    if !status.is_running { return; }
    let _ = match status.name.as_str() {
        "Hyprland" => std::process::Command::new("hyprctl").arg("reload").spawn(),
        "Niri"     => std::process::Command::new("niri").args(["msg", "action", "reload-config"]).spawn(),
        "Sway"     => std::process::Command::new("swaymsg").arg("reload").spawn(),
        "River"    => return, // River re-reads init on restart, no live reload
        _          => return,
    };
}
