use super::{RenderCtx, Widget};
use crate::draw::hex_color;
use chrono::Local;

pub struct DateWidget;

impl DateWidget {
    pub fn new() -> Self { Self }
}

impl Widget for DateWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let s = Local::now().format("%a %d %b").to_string();
        (text.measure(&s, theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let s  = Local::now().format("%a %d %b").to_string();
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;
        ctx.text.draw(ctx.pixmap, &s, x + 8.0, ty, ctx.theme.font_size, hex_color(&ctx.theme.dim));
    }
}
