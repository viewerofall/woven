//! Swaync notification widget.
//! Shows bell icon + unread count. Click toggles the swaync panel.
//! Accent color when DND is off + has notifications, dim when DND on.

use super::{RenderCtx, Widget};
use crate::draw::{fill_rounded_rect, hex_color};

pub struct NotificationsWidget {
    count:    u32,
    dnd:      bool,
    last_ms:  u64,
}

impl NotificationsWidget {
    pub fn new() -> Self {
        Self { count: 0, dnd: false, last_ms: 0 }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 3000 { return; }
        self.last_ms = now;

        // DND state
        let dnd_out = run("swaync-client", &["-D"]);
        self.dnd = dnd_out.trim() == "true";

        // Notification count — swaync-client -c prints the count
        let count_out = run("swaync-client", &["-c"]);
        self.count = count_out.trim().parse().unwrap_or(0);
    }
}

impl Widget for NotificationsWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        // Wide enough for bell + 2-digit count
        (text.measure("\u{f09a4} 99", theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h   = ctx.height as f32;
        let ty  = (h - ctx.theme.font_size) / 2.0;
        let w   = ctx.text.measure("\u{f09a4} 99", ctx.theme.font_size) + 16.0;
        let pad = 4.0;

        // Background pill when there are unread notifications (and not DND)
        if self.count > 0 && !self.dnd {
            fill_rounded_rect(
                ctx.pixmap,
                x + pad, pad,
                w - pad * 2.0, h - pad * 2.0,
                ctx.theme.radius as f32,
                hex_color(&ctx.theme.dim),
            );
        }

        // nf-md-bell (f09a4) normal, nf-md-bell_off (f09a7) when DND
        let icon = if self.dnd { "\u{f09a7}" } else { "\u{f09a4}" };

        let color = if self.dnd {
            hex_color(&ctx.theme.dim)
        } else if self.count > 0 {
            hex_color(&ctx.theme.accent)
        } else {
            hex_color(&ctx.theme.foreground)
        };

        let label = if self.count > 0 {
            format!("{icon} {}", self.count)
        } else {
            icon.to_string()
        };

        let lw = ctx.text.measure(&label, ctx.theme.font_size);
        let lx = x + (w - lw) / 2.0;
        ctx.text.draw(ctx.pixmap, &label, lx, ty, ctx.theme.font_size, color);
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        // Toggle swaync panel — full path for stripped PATH environment
        let _ = std::process::Command::new("swaync-client")
            .arg("-t")
            .spawn();
        self.last_ms = 0; // force refresh on next frame
    }
}

fn run(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
