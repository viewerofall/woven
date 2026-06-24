//! Activities button — launches the woven overlay when clicked.

use super::{RenderCtx, Widget};
use crate::draw::{fill_rounded_rect, hex_color};

pub struct ActivitiesWidget {
    label: String,
}

impl ActivitiesWidget {
    pub fn new() -> Self {
        Self { label: " \u{f00b} ".into() } // nf-fa-th grid — reliable across all NF fonts
    }
}

impl Widget for ActivitiesWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let w = text.measure(&self.label, theme.font_size);
        (w + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let h   = ctx.height as f32;
        let w   = ctx.text.measure(&self.label, ctx.theme.font_size) + 16.0;
        let pad = 4.0;

        fill_rounded_rect(
            ctx.pixmap,
            x + pad, pad,
            w - pad * 2.0, h - pad * 2.0,
            ctx.theme.radius as f32,
            hex_color(&ctx.theme.dim),
        );

        let tw = ctx.text.measure(&self.label, ctx.theme.font_size);
        let tx = x + (w - tw) / 2.0;
        let ty = (h - ctx.theme.font_size) / 2.0;
        ctx.text.draw(ctx.pixmap, &self.label, tx, ty, ctx.theme.font_size, hex_color(&ctx.theme.accent));
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        // Full path — woven-bar inherits sway's stripped PATH
        let _ = std::process::Command::new("/home/abyss/.local/bin/woven-ctrl")
            .arg("--toggle")
            .spawn();
    }
}
