//! Media player widget — shows now-playing via playerctl.
//! Polls every 2s. Click to play/pause.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct MediaWidget {
    title:    String,
    playing:  bool,
    last_ms:  u64,
    max_chars: usize,
}

impl MediaWidget {
    pub fn new() -> Self {
        Self { title: String::new(), playing: false, last_ms: 0, max_chars: 30 }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 2000 { return; }
        self.last_ms = now;

        // playerctl status → Playing / Paused / Stopped
        let status = run_playerctl(&["status"]);
        self.playing = status.trim() == "Playing";

        if self.playing || status.trim() == "Paused" {
            let artist = run_playerctl(&["metadata", "artist"]);
            let title  = run_playerctl(&["metadata", "title"]);
            let artist = artist.trim();
            let title  = title.trim();
            self.title = if !artist.is_empty() && !title.is_empty() {
                format!("{artist} — {title}")
            } else if !title.is_empty() {
                title.to_string()
            } else {
                String::new()
            };
        } else {
            self.title.clear();
        }
    }
}

impl Widget for MediaWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        if self.title.is_empty() { return 0; }
        let icon = if self.playing { "\u{f040a} " } else { "\u{f03e4} " }; // nf-md-play/pause
        let display = truncate(&self.title, self.max_chars);
        let s = format!("{icon}{display}");
        (text.measure(&s, theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        if self.title.is_empty() { return; }

        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        let icon = if self.playing { "\u{f040a}" } else { "\u{f03e4}" };
        let display = truncate(&self.title, self.max_chars);
        let label = format!("{icon} {display}");

        let color = if self.playing {
            hex_color(&ctx.theme.accent)
        } else {
            hex_color(&ctx.theme.dim)
        };

        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }

    fn on_click(&mut self, _x: f64, _y: f64) {
        let _ = std::process::Command::new("playerctl").arg("play-pause").spawn();
        self.last_ms = 0; // force refresh
    }
}

fn run_playerctl(args: &[&str]) -> String {
    std::process::Command::new("playerctl")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
