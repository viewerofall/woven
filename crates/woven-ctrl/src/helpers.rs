//! Shared helpers used by main.rs and setup.rs.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use woven_common::ipc::{IpcCommand, IpcResponse};

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

pub fn write_config(content: &str) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

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

pub fn lua_str(src: &str, key: &str) -> Option<String> {
    let needle = format!("{} = \"", key);
    let start  = src.find(&needle)? + needle.len();
    let end    = start + src[start..].find('"')?;
    Some(src[start..end].to_string())
}

pub fn lua_num(src: &str, key: &str) -> Option<String> {
    let needle = format!("{} = ", key);
    let start  = src.find(&needle)? + needle.len();
    let end    = start + src[start..]
    .find(|c: char| c == ',' || c == '\n' || c == '}')?;
    let val = src[start..end].trim().to_string();
    if val.is_empty() { None } else { Some(val) }
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
                             format!(
                                 concat!(
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
                                 ),
                                 bg, border, txt, accent, radius, opacity
                             )
                         }

                         pub fn splice_theme_into_config(config: &str, block: &str) -> String {
                             if let Some(start) = config.find("woven.theme({") {
                                 let rest = &config[start..];
                                 if let Some(rel) = rest.find("})") {
                                     let end = start + rel + 2;
                                     return format!("{}{}{}", &config[..start], block, &config[end..]);
                                 }
                             }
                             format!("{}\n{}\n", config.trim_end(), block)
                         }

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
