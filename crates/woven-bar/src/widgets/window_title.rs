//! Active window title widget — shows focused window title, left-aligned.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct WindowTitleWidget {
    max_chars: usize,
}

impl WindowTitleWidget {
    pub fn new() -> Self {
        Self { max_chars: 40 }
    }
}

impl Widget for WindowTitleWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        // Reserve for max_chars using 'a' as a narrower reference glyph
        (text.measure(&"a".repeat(self.max_chars), theme.font_size) + 12.0).min(260.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let title = ctx.state.active_title.clone();
        if title.is_empty() { return; }

        let display: String = if title.chars().count() > self.max_chars {
            let mut s: String = title.chars().take(self.max_chars - 1).collect();
            s.push('…');
            s
        } else {
            title
        };

        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        ctx.text.draw(
            ctx.pixmap, &display, x + 6.0, ty,
            ctx.theme.font_size,
            hex_color(&ctx.theme.foreground),
        );
    }
}
