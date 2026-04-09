//! Shared helpers used by main.rs and setup.rs.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use woven_common::ipc::{IpcCommand, IpcResponse};
use mlua::Lua;

pub fn send_ipc(cmd: IpcCommand) -> Option<IpcResponse> {
    let mut stream = UnixStream::connect(woven_common::ipc::socket_path()).ok()?;
    let mut line   = serde_json::to_string(&cmd).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    let mut buf = String::new();
    BufReader::new(stream).read_line(&mut buf).ok()?;
    serde_json::from_str(buf.trim()).ok()
}

pub fn config_path() -> String {
    let base = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config",
            std::env::var("HOME").unwrap_or_else(|_| ".".into())));
    format!("{}/woven/woven.lua", base)
}

pub fn config_exists() -> bool {
    std::path::Path::new(&config_path()).exists()
}

pub fn read_config() -> String {
    std::fs::read_to_string(config_path()).unwrap_or_else(|_| default_config())
}

/// Compile `src` as a Lua 5.4 chunk without executing it.
/// Returns `Err(message)` on any syntax error so the caller can abort before
/// writing the file or restarting the daemon.
pub fn validate_lua_syntax(src: &str) -> Result<(), String> {
    let lua = Lua::new();
    lua.load(src)
        .set_name("woven.lua")
        .into_function()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn write_config(content: &str) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

// ── Lua parsers ───────────────────────────────────────────────────────────────

/// Find `key = "value"` on any line, tolerating any amount of alignment whitespace.
pub fn lua_str(src: &str, key: &str) -> Option<String> {
    for line in src.lines() {
        let t = line.trim_start();
        if !t.starts_with(key) { continue; }
        let after = &t[key.len()..];
        // Key must be followed by whitespace or '=' — not just a prefix of a longer key
        if !after.starts_with([' ', '\t', '=']) { continue; }
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('=')?;
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('"')?;
        let end   = after.find('"')?;
        return Some(after[..end].to_string());
    }
    None
}

/// Find `key = value` (unquoted number/bool) on any line, tolerating alignment whitespace.
pub fn lua_num(src: &str, key: &str) -> Option<String> {
    for line in src.lines() {
        let t = line.trim_start();
        if !t.starts_with(key) { continue; }
        let after = &t[key.len()..];
        if !after.starts_with([' ', '\t', '=']) { continue; }
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('=')?;
        let after = after.trim_start_matches([' ', '\t']);
        let end   = after.find([',', '\n', '}', ' ', '\t'])
                         .unwrap_or(after.len());
        let v = after[..end].trim();
        if v.is_empty() { return None; }
        return Some(v.to_string());
    }
    None
}

/// Find `key = true/false` on any line, tolerating alignment whitespace.
pub fn lua_bool(src: &str, key: &str) -> Option<bool> {
    let val = lua_num(src, key)?;
    match val.as_str() {
        "true"  => Some(true),
        "false" => Some(false),
        _       => None,
    }
}

/// Parse `key = { curve = "...", duration_ms = ... }` from within an animations block.
pub fn lua_anim(src: &str, key: &str) -> (String, String) {
    let needle = format!("{} ", key.trim());
    if let Some(line_start) = src.find(&needle) {
        let line_end = src[line_start..].find('\n')
            .map(|i| line_start + i)
            .unwrap_or(src.len());
        let line = &src[line_start..line_end];
        let curve = lua_str(line, "curve").unwrap_or_else(|| "ease_out_cubic".into());
        let ms    = lua_num(line, "duration_ms").unwrap_or_else(|| "180".into());
        return (curve, ms);
    }
    ("ease_out_cubic".into(), "180".into())
}

// ── Theme ─────────────────────────────────────────────────────────────────────

pub const PRESETS: &[&str] = &[
    "Catppuccin Mocha", "Dracula", "Nord", "Tokyo Night", "Gruvbox", "Custom",
];

pub fn preset_colors(preset: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match preset {
        "Catppuccin Mocha" => ("#1e1e2e", "#cba6f7", "#cdd6f4", "#6c7086"),
        "Dracula"          => ("#282a36", "#bd93f9", "#f8f8f2", "#6272a4"),
        "Nord"             => ("#2e3440", "#88c0d0", "#eceff4", "#4c566a"),
        "Tokyo Night"      => ("#1a1b26", "#7aa2f7", "#c0caf5", "#414868"),
        "Gruvbox"          => ("#282828", "#d79921", "#ebdbb2", "#504945"),
        _                  => ("#1e1e2e", "#cba6f7", "#cdd6f4", "#6c7086"),
    }
}

pub struct ParsedTheme {
    pub background:    String,
    pub border:        String,
    pub text:          String,
    pub accent:        String,
    pub opacity:       String,
    pub border_radius: String,
    pub preset:        String,
}

pub fn parse_theme_from_config() -> ParsedTheme {
    let raw  = read_config();
    let defs = woven_common::types::Theme::default();
    let bg  = lua_str(&raw, "background")   .unwrap_or(defs.background);
    let bd  = lua_str(&raw, "border")       .unwrap_or(defs.border);
    let txt = lua_str(&raw, "text")         .unwrap_or(defs.text);
    let ac  = lua_str(&raw, "accent")       .unwrap_or(defs.accent);
    let op  = lua_num(&raw, "opacity")      .unwrap_or_else(|| format!("{:.2}", defs.opacity));
    let rad = lua_num(&raw, "border_radius").unwrap_or_else(|| defs.border_radius.to_string());
    let preset = match (bg.as_str(), ac.as_str()) {
        ("#1e1e2e", "#cba6f7") => "Catppuccin Mocha",
        ("#282a36", "#bd93f9") => "Dracula",
        ("#2e3440", "#88c0d0") => "Nord",
        ("#1a1b26", "#7aa2f7") => "Tokyo Night",
        ("#282828", "#d79921") => "Gruvbox",
        _                     => "Custom",
    }.to_string();
    ParsedTheme { background: bg, border: bd, text: txt, accent: ac,
        opacity: op, border_radius: rad, preset }
}

pub fn build_theme_block(bg: &str, accent: &str, txt: &str, border: &str,
                         radius: u32, opacity: f32) -> String {
    format!(concat!(
        "woven.theme({{\n",
        "    background    = \"{}\",\n",
        "    border        = \"{}\",\n",
        "    text          = \"{}\",\n",
        "    accent        = \"{}\",\n",
        "    border_radius = {},\n",
        "    font          = \"JetBrainsMono Nerd Font\",\n",
        "    font_size     = 13,\n",
        "    opacity       = {:.2},\n",
        "}})",
    ), bg, border, txt, accent, radius, opacity)
}

pub fn splice_theme_into_config(config: &str, block: &str) -> String {
    splice_block(config, "woven.theme({", block)
}

// ── Bar ───────────────────────────────────────────────────────────────────────

pub const BAR_POSITIONS: &[&str] = &["right", "left", "top", "bottom"];

pub struct ParsedBar {
    pub enabled:  bool,
    pub position: String,
}

pub fn parse_bar_from_config() -> ParsedBar {
    let raw = read_config();
    ParsedBar {
        enabled:  lua_bool(&raw, "enabled").unwrap_or(true),
        position: lua_str(&raw, "position").unwrap_or_else(|| "right".into()),
    }
}

pub fn build_bar_block(enabled: bool, position: &str) -> String {
    format!(concat!(
        "woven.bar({{\n",
        "    enabled  = {},\n",
        "    position = \"{}\",\n",
        "}})",
    ), enabled, position)
}

pub fn splice_bar_into_config(config: &str, block: &str) -> String {
    splice_block(config, "woven.bar({", block)
}

// ── Overview / animations ─────────────────────────────────────────────────────

pub const ANIM_CURVES: &[&str] = &[
    "ease_out_cubic", "ease_in_cubic", "ease_in_out_cubic", "linear", "spring",
];

pub struct ParsedOverview {
    pub show_empty:         bool,
    pub scroll_dir:         String,
    pub anim_open_curve:    String,
    pub anim_open_ms:       String,
    pub anim_close_curve:   String,
    pub anim_close_ms:      String,
    pub anim_scroll_curve:  String,
    pub anim_scroll_ms:     String,
}

pub fn parse_overview_from_config() -> ParsedOverview {
    let raw = read_config();
    let (open_curve,   open_ms)   = lua_anim(&raw, "overlay_open");
    let (close_curve,  close_ms)  = lua_anim(&raw, "overlay_close");
    let (scroll_curve, scroll_ms) = lua_anim(&raw, "scroll");
    ParsedOverview {
        show_empty:        lua_bool(&raw, "show_empty").unwrap_or(false),
        scroll_dir:        lua_str(&raw, "scroll_dir").unwrap_or_else(|| "horizontal".into()),
        anim_open_curve:   open_curve,
        anim_open_ms:      open_ms,
        anim_close_curve:  close_curve,
        anim_close_ms:     close_ms,
        anim_scroll_curve: scroll_curve,
        anim_scroll_ms:    scroll_ms,
    }
}

pub fn build_workspaces_block(show_empty: bool) -> String {
    format!(concat!(
        "woven.workspaces({{\n",
        "    show_empty = {},\n",
        "    min_width  = 200,\n",
        "    max_width  = 400,\n",
        "}})",
    ), show_empty)
}

pub fn build_settings_block(scroll_dir: &str) -> String {
    format!(concat!(
        "woven.settings({{\n",
        "    scroll_dir      = \"{}\",\n",
        "    overlay_opacity = 0.92,\n",
        "}})",
    ), scroll_dir)
}

pub fn build_animations_block(
    open_curve: &str,  open_ms: &str,
    close_curve: &str, close_ms: &str,
    scroll_curve: &str, scroll_ms: &str,
) -> String {
    format!(concat!(
        "woven.animations({{\n",
        "    overlay_open  = {{ curve = \"{}\", duration_ms = {} }},\n",
        "    overlay_close = {{ curve = \"{}\", duration_ms = {} }},\n",
        "    scroll        = {{ curve = \"{}\", duration_ms = {} }},\n",
        "}})",
    ), open_curve, open_ms, close_curve, close_ms, scroll_curve, scroll_ms)
}

pub fn splice_workspaces_into_config(config: &str, block: &str) -> String {
    splice_block(config, "woven.workspaces({", block)
}

pub fn splice_settings_into_config(config: &str, block: &str) -> String {
    splice_block(config, "woven.settings({", block)
}

pub fn splice_animations_into_config(config: &str, block: &str) -> String {
    splice_block(config, "woven.animations({", block)
}

// ── Default config ────────────────────────────────────────────────────────────

pub fn default_config() -> String {
    concat!(
        "-- ~/.config/woven/woven.lua\n",
        "-- Reload live: woven-ctrl --reload\n",
        "\n",
        "woven.theme({\n",
        "    background    = \"#1e1e2e\",\n",
        "    border        = \"#6c7086\",\n",
        "    text          = \"#cdd6f4\",\n",
        "    accent        = \"#cba6f7\",\n",
        "    border_radius = 12,\n",
        "    font          = \"JetBrainsMono Nerd Font\",\n",
        "    font_size     = 13,\n",
        "    opacity       = 0.92,\n",
        "})\n",
        "\n",
        "woven.bar({\n",
        "    enabled  = true,\n",
        "    position = \"right\",\n",
        "})\n",
        "\n",
        "woven.workspaces({\n",
        "    show_empty = false,\n",
        "    min_width  = 200,\n",
        "    max_width  = 400,\n",
        "})\n",
        "\n",
        "woven.settings({\n",
        "    scroll_dir      = \"horizontal\",\n",
        "    overlay_opacity = 0.92,\n",
        "})\n",
        "\n",
        "woven.animations({\n",
        "    overlay_open  = { curve = \"ease_out_cubic\",    duration_ms = 180 },\n",
        "    overlay_close = { curve = \"ease_in_cubic\",     duration_ms = 120 },\n",
        "    scroll        = { curve = \"ease_in_out_cubic\", duration_ms = 200 },\n",
        "})\n",
    ).into()
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn splice_block(config: &str, marker: &str, block: &str) -> String {
    if let Some(start) = config.find(marker) {
        let rest = &config[start..];
        if let Some(rel) = rest.find("})") {
            let end = start + rel + 2;
            return format!("{}{}{}", &config[..start], block, &config[end..]);
        }
    }
    format!("{}\n{}\n", config.trim_end(), block)
}

// ── Plugin setup helpers ─────────────────────────────────────────────────

/// Extract the text between `{` and the matching `}` in a plugin's `setup({...})` call.
/// Handles nested braces (e.g. colors arrays, sub-tables).
pub fn extract_plugin_opts(config: &str, plugin: &str) -> String {
    for q in ['"', '\''] {
        let marker = format!("require({0}plugins.{1}{0}).setup({{", q, plugin);
        if let Some(pos) = config.find(&marker) {
            let content_start = pos + marker.len();
            let rest = &config[content_start..];
            let mut depth = 1u32;
            for (i, ch) in rest.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return rest[..i].to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    String::new()
}

/// Replace a plugin's entire `require("plugins.X").setup(...)` call with `block`.
/// Handles both single-line and multi-line setup calls with nested braces.
pub fn splice_plugin_setup(config: &str, plugin: &str, block: &str) -> String {
    for q in ['"', '\''] {
        let marker = format!("require({0}plugins.{1}{0}).setup(", q, plugin);
        if let Some(start) = config.find(&marker) {
            let after_paren = &config[start + marker.len()..];

            // setup() with no args
            if after_paren.starts_with(')') {
                let end = start + marker.len() + 1;
                return format!("{}{}{}", &config[..start], block, &config[end..]);
            }

            // setup({...}) — match braces to find the end
            if after_paren.starts_with('{') {
                let mut depth = 0u32;
                for (i, ch) in after_paren.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                let mut end = start + marker.len() + i + 1;
                                if config.get(end..end + 1) == Some(")") {
                                    end += 1;
                                }
                                return format!("{}{}{}", &config[..start], block, &config[end..]);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    format!("{}\n{}\n", config.trim_end(), block)
}

/// Parse `key = "value"` from a comma-separated (or line-separated) opts string.
/// Unlike `lua_str`, this works for single-line plugin opts.
pub fn opt_str(src: &str, key: &str) -> Option<String> {
    for seg in src.split([',', '\n']) {
        let t = seg.trim();
        if !t.starts_with(key) { continue; }
        let after = &t[key.len()..];
        if !after.starts_with([' ', '\t', '=']) { continue; }
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('=')?;
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('"')?;
        let end   = after.find('"')?;
        return Some(after[..end].to_string());
    }
    None
}

/// Parse `key = <number>` from a comma-separated (or line-separated) opts string.
pub fn opt_num(src: &str, key: &str) -> Option<String> {
    for seg in src.split([',', '\n']) {
        let t = seg.trim();
        if !t.starts_with(key) { continue; }
        let after = &t[key.len()..];
        if !after.starts_with([' ', '\t', '=']) { continue; }
        let after = after.trim_start_matches([' ', '\t']);
        let after = after.strip_prefix('=')?;
        let after = after.trim_start_matches([' ', '\t']);
        if after.starts_with('"') { continue; } // skip string values
        let end = after.find([',', ' ', '}', '\n']).unwrap_or(after.len());
        let v = after[..end].trim();
        if v.is_empty() { return None; }
        return Some(v.to_string());
    }
    None
}

/// Parse `["key"] = "value"` pairs from a Lua table snippet.
pub fn parse_lua_bracket_table(src: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if !t.starts_with("[\"") { continue; }
        if let Some(k_end) = t[2..].find("\"]") {
            let key = t[2..2 + k_end].to_string();
            let rest = &t[2 + k_end + 2..];
            let rest = rest.trim_start().trim_start_matches('=').trim_start();
            if let Some(stripped) = rest.strip_prefix('"') {
                if let Some(v_end) = stripped.find('"') {
                    results.push((key, stripped[..v_end].to_string()));
                }
            }
        }
    }
    results
}
