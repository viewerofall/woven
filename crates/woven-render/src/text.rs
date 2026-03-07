//! Dual-font text renderer.
//! Font 0 = regular text (DejaVu / Liberation / system sans)
//! Font 1 = Nerd Font for icons (JetBrainsMono NF, FiraCode NF, etc.)
//!
//! draw() and measure() automatically pick the right font per-codepoint:
//! if a glyph is in the private use area (U+E000–U+F8FF) or common NF ranges,
//! it uses the icon font. Everything else uses the text font.

use fontdue::{Font, FontSettings, layout::{CoordinateSystem, Layout, TextStyle}};
use tiny_skia::{Color, Pixmap};

pub struct TextRenderer {
    fonts:  Vec<Font>,  // [0] = text, [1] = icons (may be same as [0] if NF not found)
    layout: Layout,
    has_nerd_font: bool,
}

impl TextRenderer {
    pub fn new() -> Self {
        let text_data = load_font(&[
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        ]).unwrap_or_else(|| include_bytes!("../fonts/NotoSans-Regular.ttf").to_vec());

        let icon_data = load_font(&[
            // CachyOS / Arch common paths
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
            // fallback: user local fonts
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
        }
    }

    /// Draw text, returns advance width in pixels
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

        // Split into runs: icon codepoints vs text codepoints
        // Draw each run with the appropriate font
        let mut advance = 0.0f32;
        for (run_text, font_idx) in split_runs(text, self.has_nerd_font) {
            let fi = font_idx.min(self.fonts.len() - 1);

            self.layout.reset(&Default::default());
            self.layout.append(
                &self.fonts.iter().collect::<Vec<_>>(),
                               &TextStyle::new(&run_text, size, fi),
            );

            let pixels = pixmap.pixels_mut();
            for glyph in self.layout.glyphs() {
                let fi_actual = glyph.font_index.min(self.fonts.len() - 1);
                let (metrics, bitmap) = self.fonts[fi_actual].rasterize(glyph.parent, size);
                if metrics.width == 0 || metrics.height == 0 { continue; }

                let gx = (x + advance + glyph.x).round() as i32;
                let gy = (y + glyph.y).round() as i32;

                for row in 0..metrics.height {
                    for col in 0..metrics.width {
                        let px = gx + col as i32;
                        let py = gy + row as i32;
                        if px < 0 || py < 0 || px >= pw || py >= ph { continue; }

                        let coverage = bitmap[row * metrics.width + col];
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
            // advance by this run's rightmost glyph edge only — don't double count
            let run_w = self.layout.glyphs().iter()
            .map(|g| g.x + g.width as f32)
            .fold(0.0f32, f32::max);
            advance += run_w;
        }

        advance
    }

    /// Measure text width without drawing
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        if text.is_empty() || size < 1.0 { return 0.0; }
        let mut total = 0.0f32;
        for (run_text, font_idx) in split_runs(text, self.has_nerd_font) {
            let fi = font_idx.min(self.fonts.len() - 1);
            self.layout.reset(&Default::default());
            self.layout.append(
                &self.fonts.iter().collect::<Vec<_>>(),
                               &TextStyle::new(&run_text, size, fi),
            );
            let w = self.layout.glyphs().iter()
            .map(|g| g.x + g.width as f32)
            .fold(0.0f32, f32::max);
            total += w;
        }
        total
    }
}

/// Split text into (run_string, font_index) pairs.
/// font_index 1 = icon font (NF private use area), 0 = text font.
fn split_runs(text: &str, has_nerd_font: bool) -> Vec<(String, usize)> {
    if !has_nerd_font {
        return vec![(text.to_string(), 0)];
    }
    let mut runs: Vec<(String, usize)> = Vec::new();
    let mut current = String::new();
    let mut current_fi: usize = 0;

    for ch in text.chars() {
        let fi = if is_icon_codepoint(ch) { 1 } else { 0 };
        if fi != current_fi && !current.is_empty() {
            runs.push((current.clone(), current_fi));
            current.clear();
        }
        current_fi = fi;
        current.push(ch);
    }
    if !current.is_empty() {
        runs.push((current, current_fi));
    }
    if runs.is_empty() {
        runs.push((text.to_string(), 0));
    }
    runs
}

fn is_icon_codepoint(ch: char) -> bool {
    let c = ch as u32;
    // Nerd Font ranges
    matches!(c,
             0xE000..=0xF8FF   |  // Private Use Area (main NF icons)
    0xF0000..=0xFFFFF |  // Supplementary PUA-A
    0x100000..=0x10FFFF  // Supplementary PUA-B
    ) || matches!(c,
                  // Powerline / extra symbol ranges used by NF
                  0x2580..=0x259F |    // Block Elements
                  0x25A0..=0x25FF |    // Geometric Shapes
                  0x2600..=0x26FF |    // Misc Symbols
                  0x2700..=0x27BF |    // Dingbats
                  0xF200..=0xF2FF      // Pomicons
    )
}

fn load_font(paths: &[&str]) -> Option<Vec<u8>> {
    for path in paths {
        // expand ~ manually
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
