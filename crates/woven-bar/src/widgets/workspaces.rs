//! Workspace indicator — numbered pills, accent for active, dim for inactive.
//! Caches actual workspace count so width() matches what's rendered.

use super::{RenderCtx, Widget};
use crate::draw::{fill_rounded_rect, hex_color};

pub struct WorkspacesWidget {
    last_count: usize,
}

impl WorkspacesWidget {
    pub fn new() -> Self { Self { last_count: 1 } }

    fn pill_w(text: &mut crate::text::TextRenderer, font_size: f32) -> f32 {
        text.measure("9", font_size) + 14.0
    }
}

impl Widget for WorkspacesWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let pw = Self::pill_w(text, theme.font_size);
        let n  = self.last_count.max(1) as f32;
        (pw * n + 4.0 * (n - 1.0) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.last_count = ctx.state.workspaces.len().max(1);
        if ctx.state.workspaces.is_empty() { return; }

        let h      = ctx.height as f32;
        let pad_v  = 5.0f32;
        let pill_h = h - pad_v * 2.0;
        let gap    = 4.0f32;
        let pw     = Self::pill_w(ctx.text, ctx.theme.font_size);
        let ty     = (h - ctx.theme.font_size) / 2.0;
        let mut dx = x + 8.0;

        for ws in &ctx.state.workspaces {
            let (bg, fg) = if ws.active {
                (hex_color(&ctx.theme.accent), hex_color(&ctx.theme.background))
            } else if ws.urgent {
                (hex_color("#f07178"), hex_color(&ctx.theme.background))
            } else {
                (hex_color(&ctx.theme.dim), hex_color(&ctx.theme.foreground))
            };

            fill_rounded_rect(ctx.pixmap, dx, pad_v, pw, pill_h, ctx.theme.radius as f32, bg);

            let label = ws.num.to_string();
            let lw = ctx.text.measure(&label, ctx.theme.font_size);
            ctx.text.draw(ctx.pixmap, &label, dx + (pw - lw) / 2.0, ty, ctx.theme.font_size, fg);

            dx += pw + gap;
        }
    }
}
