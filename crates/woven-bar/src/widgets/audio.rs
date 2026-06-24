//! Audio volume widget — reads from wpctl (PipeWire/PulseAudio).

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct AudioWidget {
    cache:        Option<AudioInfo>,
    last_read_ms: u64,
}

#[derive(Clone)]
struct AudioInfo {
    pct:  u8,
    muted: bool,
}

impl AudioWidget {
    pub fn new() -> Self {
        Self { cache: None, last_read_ms: 0 }
    }

    fn read() -> Option<AudioInfo> {
        // wpctl get-volume @DEFAULT_AUDIO_SINK@
        // output: "Volume: 0.42" or "Volume: 0.42 [MUTED]"
        let out = std::process::Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let muted = s.contains("[MUTED]");
        let vol: f32 = s.split_whitespace()
            .find(|w| w.parse::<f32>().is_ok())
            .and_then(|w| w.parse().ok())
            .unwrap_or(0.0);
        Some(AudioInfo { pct: (vol * 100.0).min(100.0) as u8, muted })
    }

    fn info(&mut self) -> Option<&AudioInfo> {
        let now_ms = now_ms();
        if self.cache.is_none() || now_ms - self.last_read_ms > 1_000 {
            self.cache        = Self::read();
            self.last_read_ms = now_ms;
        }
        self.cache.as_ref()
    }
}

impl Widget for AudioWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let w = text.measure("\u{f057f} 100%", theme.font_size);
        (w + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        let Some(info) = self.info().cloned() else { return; };

        let icon = if info.muted {
            "\u{f0581}" // nf-md-volume_mute
        } else if info.pct >= 60 {
            "\u{f057f}" // nf-md-volume_high
        } else if info.pct >= 20 {
            "\u{f0580}" // nf-md-volume_medium
        } else {
            "\u{f057e}" // nf-md-volume_low
        };

        let color = if info.muted {
            hex_color(&ctx.theme.dim)
        } else {
            hex_color(&ctx.theme.foreground)
        };

        let label = format!("{icon} {}%", info.pct);
        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        // Toggle mute on left click
        let _ = std::process::Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .spawn();
        self.cache = None; // force refresh
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
