//! XDG app icon resolver.
//!
//! Lookup pipeline for a given `app_id` (Wayland app_id / Niri window class):
//!   1. Scan `{XDG_DATA_DIRS}/applications/*.desktop` → app_id/wmclass → icon name
//!   2. Find a PNG file in hicolor or pixmaps icon dirs
//!   3. Decode PNG → RGBA bytes, scale to `icon_size × icon_size`
//!   4. Cache result (None = not found — falls through to glyph / letter fallback)
//!
//! Desktop file scanning is deferred to first lookup. After that every hit and
//! miss is cached so subsequent renders cost only a HashMap lookup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Decoded icon: (width, height, RGBA bytes).  Always square after loading.
pub type IconData = (u32, u32, Vec<u8>);

pub struct IconCache {
    /// app_id (lowercase) → pixel data (None = confirmed not found)
    cache: HashMap<String, Option<IconData>>,
    /// icon_name → pixel data (shared across multiple apps that use the same icon)
    file_cache: HashMap<String, Option<IconData>>,
    /// Keys that map to icon names: wm_class, exec_basename, desktop_name, etc.
    desktop_map: HashMap<String, String>,
    desktop_scanned: bool,
    icon_size: u32,
    /// Plugin-registered overrides: class (lowercase) → absolute file path.
    /// Checked before the XDG scan — highest priority.
    overrides: HashMap<String, String>,
    /// Optional default icon path for classes not in `overrides` or XDG.
    override_default: Option<String>,
    /// Decoded cache for override paths (keyed by file path, not class).
    override_cache: HashMap<String, Option<IconData>>,
}

impl Default for IconCache {
    fn default() -> Self {
        Self {
            cache:            HashMap::new(),
            file_cache:       HashMap::new(),
            desktop_map:      HashMap::new(),
            desktop_scanned:  false,
            icon_size:        48,
            overrides:        HashMap::new(),
            override_default: None,
            override_cache:   HashMap::new(),
        }
    }
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plugin icon override.  `class` is matched case-insensitively.
    /// `path` may be absolute or relative to the process working directory.
    pub fn register_override(&mut self, class: String, path: String) {
        // Invalidate any cached result for this class so it re-resolves.
        self.cache.remove(&class.to_lowercase());
        self.overrides.insert(class.to_lowercase(), path);
    }

    /// Register a fallback icon used when no class-specific override matches
    /// and the XDG scan also finds nothing.
    pub fn register_override_default(&mut self, path: String) {
        self.override_default = Some(path);
        // Clear all confirmed misses so they get a second chance with the new default.
        self.cache.retain(|_, v| v.is_some());
    }

    fn load_override(&mut self, path: &str) -> Option<IconData> {
        if let Some(cached) = self.override_cache.get(path) {
            return cached.clone();
        }
        let result = load_png_rgba(std::path::Path::new(path))
            .map(|(w, h, px)| scale_rgba(w, h, &px, self.icon_size, self.icon_size));
        self.override_cache.insert(path.to_string(), result.clone());
        result
    }

    /// Return RGBA bytes for the best available icon for `app_id`, or `None`.
    /// Resolution order:
    ///   1. Plugin override for this specific class
    ///   2. XDG .desktop + hicolor + pixmaps
    ///   3. Plugin override default (if set)
    pub fn get(&mut self, app_id: &str) -> Option<&IconData> {
        let key = app_id.to_lowercase();

        // Return cached result (including confirmed misses).
        if self.cache.contains_key(&key) {
            return self.cache.get(&key).and_then(|v| v.as_ref());
        }

        // 1. Plugin override — highest priority.
        if let Some(path) = self.overrides.get(&key).cloned() {
            let data = self.load_override(&path);
            self.cache.insert(key.clone(), data);
            return self.cache.get(&key).and_then(|v| v.as_ref());
        }

        // 2. XDG scan.
        if !self.desktop_scanned {
            self.desktop_map = scan_desktop_files();
            self.desktop_scanned = true;
            debug!("icons: scanned {} desktop entries", self.desktop_map.len());
        }
        let icon_name = self.resolve_icon_name(&key);
        let data = if let Some(ref name) = icon_name { self.load_icon(name) } else { None };

        // 3. Fall through to plugin default if XDG found nothing.
        let data = data.or_else(|| {
            self.override_default.clone()
                .and_then(|p| self.load_override(&p))
        });

        self.cache.insert(key.clone(), data);
        self.cache.get(&key).and_then(|v| v.as_ref())
    }

    fn resolve_icon_name(&self, app_id: &str) -> Option<String> {
        // 1. Direct match
        if let Some(n) = self.desktop_map.get(app_id) { return Some(n.clone()); }

        // 2. Strip common reverse-DNS prefix (org.gnome.Files → files → gnome-files)
        if app_id.contains('.') {
            let parts: Vec<&str> = app_id.split('.').collect();
            // Last segment (e.g. "Files")
            if let Some(last) = parts.last() {
                let low = last.to_lowercase();
                if let Some(n) = self.desktop_map.get(low.as_str()) { return Some(n.clone()); }
            }
            // Second-to-last + last (e.g. "gnome-files")
            if parts.len() >= 2 {
                let joined = format!("{}-{}", parts[parts.len()-2], parts[parts.len()-1]).to_lowercase();
                if let Some(n) = self.desktop_map.get(joined.as_str()) { return Some(n.clone()); }
            }
        }

        // 3. Try app_id itself as the icon name (many apps match directly)
        Some(app_id.to_string())
    }

    fn load_icon(&mut self, icon_name: &str) -> Option<IconData> {
        let key = icon_name.to_lowercase();

        if self.file_cache.contains_key(&key) {
            return self.file_cache.get(&key).and_then(|v| v.clone());
        }

        let result = find_icon_file(&key)
            .and_then(|path| load_png_rgba(&path))
            .map(|(w, h, px)| scale_rgba(w, h, &px, self.icon_size, self.icon_size));

        self.file_cache.insert(key, result.clone());
        result
    }
}

// ── .desktop file scanning ─────────────────────────────────────────────────────

fn scan_desktop_files() -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();

    let home = home_dir();
    let mut search_dirs = vec![
        format!("{home}/.local/share/applications"),
    ];

    let xdg_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in xdg_dirs.split(':') {
        if !d.is_empty() {
            search_dirs.push(format!("{d}/applications"));
        }
    }

    for dir in &search_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") { continue; }
            parse_desktop_file(&path, &mut map);
        }
    }

    map
}

fn parse_desktop_file(path: &Path, map: &mut HashMap<String, String>) {
    let Ok(content) = std::fs::read_to_string(path) else { return };

    let mut in_entry  = false;
    let mut icon_name: Option<String> = None;
    let mut keys: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry { continue; }

        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());

        match k {
            "Icon" => {
                icon_name = Some(v.to_string());
            }
            "Name" => {
                keys.push(v.to_lowercase());
            }
            "StartupWMClass" => {
                keys.push(v.to_lowercase());
            }
            "Exec" => {
                // Strip path and flags to get the bare executable name
                if let Some(exe) = v.split_whitespace().next() {
                    let base = exe.rsplit('/').next().unwrap_or(exe);
                    // Strip common wrappers (env, flatpak run, etc.)
                    let base = base.trim_end_matches(".sh");
                    keys.push(base.to_lowercase());
                }
            }
            _ => {}
        }
    }

    // Also register the .desktop filename stem (e.g. "org.gnome.Files" → "org.gnome.files")
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        keys.push(stem.to_lowercase());
        // And short form
        if stem.contains('.') {
            if let Some(last) = stem.rsplit('.').next() {
                keys.push(last.to_lowercase());
            }
        }
    }

    if let Some(icon) = icon_name {
        for key in keys {
            if !key.is_empty() {
                map.entry(key).or_insert_with(|| icon.clone());
            }
        }
    }
}

// ── icon file lookup ──────────────────────────────────────────────────────────

fn find_icon_file(icon_name: &str) -> Option<PathBuf> {
    let home = home_dir();

    // Preferred sizes in order (we want 48px but accept others and will scale)
    let sizes = ["48x48", "32x32", "64x64", "128x128", "256x256", "22x22", "16x16"];

    // hicolor is the universal fallback theme; also try user's local icons
    let icon_base_dirs = [
        format!("{home}/.local/share/icons/hicolor"),
        "/usr/share/icons/hicolor".into(),
    ];

    for base in &icon_base_dirs {
        for size in &sizes {
            let p = PathBuf::from(format!("{base}/{size}/apps/{icon_name}.png"));
            if p.exists() { return Some(p); }
        }
        // scalable/apps — SVG, skip (no SVG renderer available)
    }

    // Try the current GTK icon theme as well
    if let Some(theme) = gtk_icon_theme() {
        for size in &sizes {
            let p = PathBuf::from(format!("/usr/share/icons/{theme}/{size}/apps/{icon_name}.png"));
            if p.exists() { return Some(p); }
        }
    }

    // pixmaps flat directory
    let p = PathBuf::from(format!("/usr/share/pixmaps/{icon_name}.png"));
    if p.exists() { return Some(p); }

    // Some apps put a bare name without extension in pixmaps
    let p = PathBuf::from(format!("/usr/share/pixmaps/{icon_name}"));
    if p.exists() && p.extension().and_then(|e| e.to_str()) == Some("png") {
        return Some(p);
    }

    None
}

/// Read the GTK icon theme name from the user's GTK settings file.
fn gtk_icon_theme() -> Option<String> {
    let home = home_dir();
    let candidates = [
        format!("{home}/.config/gtk-3.0/settings.ini"),
        format!("{home}/.config/gtk-4.0/settings.ini"),
    ];
    for path in &candidates {
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("gtk-icon-theme-name") {
                if let Some(v) = rest.split('=').nth(1) {
                    let name = v.trim().trim_matches('"').trim().to_string();
                    if !name.is_empty() { return Some(name); }
                }
            }
        }
    }
    None
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".into())
}

// ── PNG loading ───────────────────────────────────────────────────────────────

fn load_png_rgba(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;
    let buf_size = reader.output_buffer_size();
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).ok()?;

    let w = info.width;
    let h = info.height;
    let n = (w * h) as usize;

    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => {
            let src = &buf[..n * 3];
            let mut out = Vec::with_capacity(n * 4);
            for chunk in src.chunks_exact(3) {
                out.push(chunk[0]); out.push(chunk[1]); out.push(chunk[2]); out.push(255);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let src = &buf[..n * 2];
            let mut out = Vec::with_capacity(n * 4);
            for chunk in src.chunks_exact(2) {
                let v = chunk[0];
                out.push(v); out.push(v); out.push(v); out.push(chunk[1]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let src = &buf[..n];
            let mut out = Vec::with_capacity(n * 4);
            for &v in src { out.push(v); out.push(v); out.push(v); out.push(255); }
            out
        }
        // Indexed (palette) not handled — too rare in practice for app icons
        _ => return None,
    };

    Some((w, h, rgba))
}

// ── scaling ───────────────────────────────────────────────────────────────────

fn scale_rgba(src_w: u32, src_h: u32, src: &[u8], dst_w: u32, dst_h: u32) -> IconData {
    if src_w == dst_w && src_h == dst_h {
        return (dst_w, dst_h, src.to_vec());
    }
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    for dy in 0..dst_h as usize {
        for dx in 0..dst_w as usize {
            let sx = (dx as f32 / dst_w as f32 * src_w as f32) as usize;
            let sy = (dy as f32 / dst_h as f32 * src_h as f32) as usize;
            let si = (sy * src_w as usize + sx) * 4;
            let di = (dy * dst_w as usize + dx) * 4;
            if si + 3 < src.len() && di + 3 < out.len() {
                out[di..di+4].copy_from_slice(&src[si..si+4]);
            }
        }
    }
    (dst_w, dst_h, out)
}
