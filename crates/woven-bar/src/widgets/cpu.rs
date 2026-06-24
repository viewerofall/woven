//! CPU usage widget — reads /proc/stat, computes delta between polls.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct CpuWidget {
    prev_idle:  u64,
    prev_total: u64,
    pct:        f32,
    last_ms:    u64,
}

impl CpuWidget {
    pub fn new() -> Self {
        let (idle, total) = read_stat();
        Self { prev_idle: idle, prev_total: total, pct: 0.0, last_ms: 0 }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 1000 { return; }
        self.last_ms = now;

        let (idle, total) = read_stat();
        let d_total = total.saturating_sub(self.prev_total);
        let d_idle  = idle.saturating_sub(self.prev_idle);
        if d_total > 0 {
            self.pct = (d_total - d_idle) as f32 / d_total as f32 * 100.0;
        }
        self.prev_idle  = idle;
        self.prev_total = total;
    }
}

impl Widget for CpuWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        (text.measure("\u{f4bc} 100%", theme.font_size) + 16.0) as u32 // nf-md-cpu-64-bit
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;
        let color = usage_color(self.pct, ctx.theme);
        let label = format!("\u{f4bc} {:.0}%", self.pct);
        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }
}

fn read_stat() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let line = s.lines().next().unwrap_or("");
    let nums: Vec<u64> = line.split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if nums.len() < 4 { return (0, 1); }
    let idle  = nums[3];
    let total: u64 = nums.iter().sum();
    (idle, total)
}

pub fn usage_color(pct: f32, theme: &crate::config::Theme) -> tiny_skia::Color {
    if pct > 85.0 { hex_color("#f07178") }       // red
    else if pct > 60.0 { hex_color("#ffcb6b") }  // yellow
    else { hex_color(&theme.foreground) }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
