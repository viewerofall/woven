//! Control center widget — gear icon button, opens settings+power panel on click.

use super::{RenderCtx, Widget};
use crate::draw::{fill_rounded_rect, hex_color};

pub struct ControlCenterWidget;

impl ControlCenterWidget {
    pub fn new() -> Self { Self }
}

impl Widget for ControlCenterWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        (text.measure("\u{f0493}", theme.font_size) + 18.0) as u32 // nf-md-cog
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let h   = ctx.height as f32;
        let ty  = (h - ctx.theme.font_size) / 2.0;
        let w   = ctx.text.measure("\u{f0493}", ctx.theme.font_size) + 18.0;
        let pad = 4.0;

        fill_rounded_rect(
            ctx.pixmap,
            x + pad, pad,
            w - pad * 2.0, h - pad * 2.0,
            ctx.theme.radius as f32,
            hex_color(&ctx.theme.dim),
        );

        let iw = ctx.text.measure("\u{f0493}", ctx.theme.font_size);
        ctx.text.draw(
            ctx.pixmap, "\u{f0493}",
            x + (w - iw) / 2.0, ty,
            ctx.theme.font_size,
            hex_color(&ctx.theme.foreground),
        );
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        // woven-cc toggles itself: second invocation kills the running instance
        let _ = std::process::Command::new("/home/abyss/.local/bin/woven-cc")
            .spawn();
    }
}
