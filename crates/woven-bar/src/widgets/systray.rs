//! Status indicators widget — pending updates + sunshine server status.
//! Shows update badge when checkupdates finds packages, and a sunshine icon
//! when the sunshine game-streaming process is running.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct SystrayWidget {
    updates:  u32,
    sunshine: bool,
    last_ms:  u64,
}

impl SystrayWidget {
    pub fn new() -> Self {
        // Defer first check by 30s so bar renders immediately on startup
        let start = now_ms().saturating_sub(570_000); // 600_000 - 30_000
        Self { updates: 0, sunshine: false, last_ms: start }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 600_000 { return; } // check every 10 min
        self.last_ms = now;

        // pacman -Qu uses cached DB — fast, no network. Exit 1 = no updates.
        let out = std::process::Command::new("pacman")
            .args(["-Qu", "--noconfirm"])
            .output()
            .unwrap_or_else(|_| std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: vec![],
                stderr: vec![],
            });
        self.updates = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count() as u32;

        // Sunshine: check if process is running
        self.sunshine = std::process::Command::new("pgrep")
            .args(["-x", "sunshine"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
}

impl Widget for SystrayWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        if self.updates == 0 && !self.sunshine {
            return 0;
        }
        let mut s = String::new();
        if self.updates > 0 { s.push_str("\u{f0540} 99"); } // nf-md-package-variant-closed
        if self.sunshine   {
            if !s.is_empty() { s.push(' '); }
            s.push('\u{f06e8}'); // nf-md-monitor
        }
        (text.measure(&s, theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        if self.updates == 0 && !self.sunshine { return; }

        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;
        let mut dx = x + 8.0;

        if self.updates > 0 {
            let s = format!("\u{f0540} {}", self.updates);
            let color = hex_color(&ctx.theme.accent);
            let w = ctx.text.measure(&s, ctx.theme.font_size);
            ctx.text.draw(ctx.pixmap, &s, dx, ty, ctx.theme.font_size, color);
            dx += w + 6.0;
        }

        if self.sunshine {
            let color = hex_color("#00e5c8"); // teal
            ctx.text.draw(ctx.pixmap, "\u{f06e8}", dx, ty, ctx.theme.font_size, color);
        }
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
