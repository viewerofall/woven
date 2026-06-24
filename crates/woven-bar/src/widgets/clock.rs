//! Date/time widget — centers on bar, opens cal popup on click.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;
use chrono::Local;

pub struct ClockWidget {
    format: String,
}

impl ClockWidget {
    pub fn new() -> Self {
        Self { format: "%a %d %b  %H:%M".into() }
    }
}

impl Widget for ClockWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let s = Local::now().format(&self.format).to_string();
        (text.measure(&s, theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let s  = Local::now().format(&self.format).to_string();
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;
        ctx.text.draw(ctx.pixmap, &s, x + 8.0, ty, ctx.theme.font_size, hex_color(&ctx.theme.foreground));
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        let _ = std::process::Command::new("kitty")
            .args([
                "--app-id", "woven-popup",
                "--override", "initial_window_width=420",
                "--override", "initial_window_height=180",
                "-e", "cal", "-3",
            ])
            .spawn();
    }
}
