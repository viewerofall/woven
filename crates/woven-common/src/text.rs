//! Dual-font text renderer with glyph rasterization cache.
//! Font 0 = regular text (Inconsolata / DejaVu / Liberation / system sans)
//! Font 1 = Nerd Font for icons (JetBrainsMono NF, FiraCode NF, etc.)
//!
//! draw() and measure() automatically pick the right font per-codepoint:
//! if a glyph is in the private use area (U+E000–U+F8FF) or common NF ranges,
//! it uses the icon font. Everything else uses the text font.

use fontdue::{Font, FontSettings, layout::{CoordinateSystem, Layout, TextStyle}};
use std::collections::HashMap;
use tiny_skia::{Color, Pixmap};

#[derive(Hash, PartialEq, Eq, Clone)]
struct GlyphKey(u8, char, u16);

struct CachedGlyph {
    width:   usize,
    height:  usize,
    bitmap:  Vec<u8>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct MeasureKey(Box<str>, u16);

struct LayoutEntry {
    glyphs:  Vec<(GlyphKey, f32, f32)>,
    advance: f32,
}

pub struct TextRenderer {
    fonts:          Vec<Font>,
    layout:         Layout,
    has_nerd_font:  bool,
    glyph_cache:    HashMap<GlyphKey, CachedGlyph>,
    measure_cache:  HashMap<MeasureKey, f32>,
    layout_cache:   HashMap<MeasureKey, LayoutEntry>,
}

impl Default for TextRenderer {
    fn default() -> Self { Self::new() }
}

impl TextRenderer {
    pub fn new() -> Self {
        let text_data = load_font(&[
            "/usr/share/fonts/TTF/Inconsolata-Regular.ttf",
            "/usr/share/fonts/TTF/Inconsolata.ttf",
            "/usr/share/fonts/truetype/inconsolata/Inconsolata-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ]).unwrap_or_else(|| include_bytes!("../fonts/NotoSans-Regular.ttf").to_vec());

        let icon_data = load_font(&[
            "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
            "/usr/share/fonts/TTF/JetBrainsMono Nerd Font Regular.ttf",
            "/usr/share/fonts/OTF/JetBrainsMonoNerdFont-Regular.otf",
            "/usr/share/fonts/TTF/FiraCodeNerdFont-Regular.ttf",
            "/usr/share/fonts/TTF/FiraMono-Regular.ttf",
            "/usr/share/fonts/TTF/Hack-Regular.ttf",
            "/usr/share/fonts/TTF/HackNerdFont-Regular.ttf",
            "/usr/share/fonts/TTF/NerdFontsSymbolsOnly.ttf",
            "/usr/share/fonts/TTF/SymbolsNerdFont-Regular.ttf",
            "/usr/share/fonts/TTF/SymbolsNerdFontMono-Regular.ttf",
            "~/.local/share/fonts/JetBrainsMonoNerdFont-Regular.ttf",
            "~/.local/share/fonts/FiraCodeNerdFont-Regular.ttf",
        ]);

        let has_nerd_font = icon_data.is_some();
        if !has_nerd_font {
            tracing::warn!(
                "No Nerd Font found — icons will show as placeholder boxes. \
                 Install ttf-jetbrains-mono-nerd (CachyOS: sudo pacman -S ttf-jetbrains-mono-nerd)"
            );
        }

        let text_font = Font::from_bytes(text_data.as_slice(), FontSettings::default())
            .expect("text font must parse");

        let fonts = if let Some(icon_bytes) = icon_data {
            match Font::from_bytes(icon_bytes.as_slice(), FontSettings::default()) {
                Ok(icon_font) => vec![text_font, icon_font],
                Err(_)        => vec![text_font],
            }
        } else {
            vec![text_font]
        };

        Self {
            fonts,
            layout: Layout::new(CoordinateSystem::PositiveYDown),
            has_nerd_font,
            glyph_cache:   HashMap::with_capacity(512),
            measure_cache: HashMap::with_capacity(256),
            layout_cache:  HashMap::with_capacity(256),
        }
    }

    /// Draw text, returns advance width in pixels.
    pub fn draw(
        &mut self,
        pixmap: &mut Pixmap,
        text:   &str,
        x:      f32,
        y:      f32,
        size:   f32,
        color:  Color,
    ) -> f32 {
        if text.is_empty() || size < 1.0 { return 0.0; }

        let r = (color.red()   * 255.0) as u8;
        let g = (color.green() * 255.0) as u8;
        let b = (color.blue()  * 255.0) as u8;
        let a = (color.alpha() * 255.0) as u8;
        if a == 0 { return 0.0; }

        let pw = pixmap.width()  as i32;
        let ph = pixmap.height() as i32;
        let size_key = (size * 2.0).round() as u16;
        let lkey = MeasureKey(text.into(), size_key);

        if !self.layout_cache.contains_key(&lkey) {
            let mut entry = LayoutEntry { glyphs: Vec::new(), advance: 0.0 };
            let fonts_ref: Vec<&Font> = self.fonts.iter().collect();
            let has_nf = self.has_nerd_font;
            for_each_run(text, has_nf, |run_text, font_idx| {
                let fi = font_idx.min(fonts_ref.len() - 1);
                self.layout.reset(&Default::default());
                self.layout.append(&fonts_ref, &TextStyle::new(run_text, size, fi));
                let base = entry.advance;
                for gl in self.layout.glyphs() {
                    let fi_a = gl.font_index.min(fonts_ref.len() - 1);
                    entry.glyphs.push((GlyphKey(fi_a as u8, gl.parent, size_key), base + gl.x, gl.y));
                }
                let run_w = self.layout.glyphs().iter()
                    .map(|g| g.x + g.width as f32).fold(0.0f32, f32::max);
                entry.advance += run_w;
            });
            self.layout_cache.insert(lkey.clone(), entry);
        }

        let (gl_list, advance) = {
            let e = &self.layout_cache[&lkey];
            (e.glyphs.clone(), e.advance)
        };

        let pixels = pixmap.pixels_mut();
        for (gk, rel_x, rel_y) in &gl_list {
            let cached = self.glyph_cache.entry(gk.clone()).or_insert_with(|| {
                let fi = gk.0 as usize;
                let (metrics, bitmap) = self.fonts[fi].rasterize(gk.1, size);
                CachedGlyph { width: metrics.width, height: metrics.height, bitmap }
            });
            if cached.width == 0 || cached.height == 0 { continue; }

            let gx = (x + rel_x).round() as i32;
            let gy = (y + rel_y).round() as i32;

            for row in 0..cached.height {
                for col in 0..cached.width {
                    let px = gx + col as i32;
                    let py = gy + row as i32;
                    if px < 0 || py < 0 || px >= pw || py >= ph { continue; }
                    let coverage = cached.bitmap[row * cached.width + col];
                    if coverage == 0 { continue; }
                    let idx   = (py * pw + px) as usize;
                    let dst   = &mut pixels[idx];
                    let src_a = (coverage as u16 * a as u16) / 255;
                    let inv_a = 255u16.saturating_sub(src_a);
                    let dr = ((r as u16 * src_a + dst.red()   as u16 * inv_a) / 255) as u8;
                    let dg = ((g as u16 * src_a + dst.green() as u16 * inv_a) / 255) as u8;
                    let db = ((b as u16 * src_a + dst.blue()  as u16 * inv_a) / 255) as u8;
                    let da = src_a.saturating_add(dst.alpha() as u16 * inv_a / 255) as u8;
                    *dst = tiny_skia::PremultipliedColorU8::from_rgba(dr, dg, db, da)
                        .unwrap_or(*dst);
                }
            }
        }

        advance
    }

    /// Clear layout + measure caches. Call when text content may have changed.
    pub fn clear_dynamic_cache(&mut self) {
        self.layout_cache.clear();
        self.measure_cache.clear();
    }

    /// Measure text width without drawing.
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        if text.is_empty() || size < 1.0 { return 0.0; }
        let size_key = (size * 2.0).round() as u16;
        let key = MeasureKey(text.into(), size_key);
        if let Some(&cached) = self.measure_cache.get(&key) {
            return cached;
        }
        let mut total = 0.0f32;
        for_each_run(text, self.has_nerd_font, |run_text, font_idx| {
            let fi = font_idx.min(self.fonts.len() - 1);
            self.layout.reset(&Default::default());
            self.layout.append(
                &self.fonts.iter().collect::<Vec<_>>(),
                &TextStyle::new(run_text, size, fi),
            );
            let w = self.layout.glyphs().iter()
                .map(|g| g.x + g.width as f32)
                .fold(0.0f32, f32::max);
            total += w;
        });
        self.measure_cache.insert(key, total);
        total
    }
}

fn for_each_run(text: &str, has_nerd_font: bool, mut f: impl FnMut(&str, usize)) {
    if !has_nerd_font {
        f(text, 0);
        return;
    }
    let mut run_start  = 0usize;
    let mut run_fi     = 0usize;
    let mut first_char = true;

    for (byte_pos, ch) in text.char_indices() {
        let fi = if is_icon_codepoint(ch) { 1 } else { 0 };
        if first_char {
            run_fi     = fi;
            first_char = false;
        } else if fi != run_fi {
            f(&text[run_start..byte_pos], run_fi);
            run_start = byte_pos;
            run_fi    = fi;
        }
    }
    if !first_char {
        f(&text[run_start..], run_fi);
    } else {
        f(text, 0);
    }
}

fn is_icon_codepoint(ch: char) -> bool {
    let c = ch as u32;
    matches!(c,
        0xE000..=0xF8FF   |
        0xF0000..=0xFFFFF |
        0x100000..=0x10FFFF
    ) || matches!(c,
        0x2580..=0x259F |
        0x25A0..=0x25FF |
        0x2600..=0x26FF |
        0x2700..=0x27BF |
        0xF200..=0xF2FF
    )
}

fn load_font(paths: &[&str]) -> Option<Vec<u8>> {
    for path in paths {
        let expanded = if path.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                format!("{}{}", home, &path[1..])
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };
        if let Ok(data) = std::fs::read(&expanded) {
            tracing::info!("font loaded: {}", expanded);
            return Some(data);
        }
    }
    None
}
