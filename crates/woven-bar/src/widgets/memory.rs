//! RAM usage widget — reads /proc/meminfo.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;
use crate::widgets::cpu::usage_color;

pub struct MemoryWidget {
    used_mb:  u64,
    total_mb: u64,
    last_ms:  u64,
}

impl MemoryWidget {
    pub fn new() -> Self {
        let (u, t) = read_mem();
        Self { used_mb: u, total_mb: t, last_ms: 0 }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 2000 { return; }
        self.last_ms = now;
        let (u, t) = read_mem();
        self.used_mb  = u;
        self.total_mb = t;
    }
}

impl Widget for MemoryWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        (text.measure("\u{f035b} 16.0G", theme.font_size) + 16.0) as u32 // nf-md-memory
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h    = ctx.height as f32;
        let ty   = (h - ctx.theme.font_size) / 2.0;
        let pct  = self.used_mb as f32 / self.total_mb.max(1) as f32 * 100.0;
        let color = usage_color(pct, ctx.theme);

        let label = if self.used_mb >= 1024 {
            format!("\u{f035b} {:.1}G", self.used_mb as f32 / 1024.0)
        } else {
            format!("\u{f035b} {}M", self.used_mb)
        };
        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }
}

fn read_mem() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = parse_kb(v);
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            avail = parse_kb(v);
        }
    }
    let used_kb = total.saturating_sub(avail);
    (used_kb / 1024, total / 1024)
}

fn parse_kb(s: &str) -> u64 {
    s.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
