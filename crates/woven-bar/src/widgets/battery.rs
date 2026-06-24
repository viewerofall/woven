//! Battery status widget — reads from /sys/class/power_supply or woven-session.

use super::{RenderCtx, Widget};
use crate::draw::{fill_rect, fill_rounded_rect, hex_color};
use woven_common::session::SessionClient;

pub struct BatteryWidget {
    cache:        Option<BatteryInfo>,
    last_read_ms: u64,
}

#[derive(Clone)]
struct BatteryInfo {
    pct:      u8,
    charging: bool,
}

impl BatteryWidget {
    pub fn new() -> Self {
        Self { cache: None, last_read_ms: 0 }
    }

    fn read() -> Option<BatteryInfo> {
        // Try woven-session first
        let mut client = SessionClient::new();
        if client.is_connected() {
            if let Some(battery) = client.get_battery() {
                let charging = battery.ac_online;
                return Some(BatteryInfo { pct: battery.percent, charging });
            }
        }

        // Fall back to sysfs
        let base = std::fs::read_dir("/sys/class/power_supply").ok()?;
        for entry in base.flatten() {
            let name = entry.file_name();
            let n    = name.to_string_lossy();
            if !n.starts_with("BAT") { continue; }
            let path  = entry.path();

            let cap  = read_int(path.join("capacity"))?;
            let status = std::fs::read_to_string(path.join("status"))
                .unwrap_or_default();
            let charging = status.trim() == "Charging" || status.trim() == "Full";

            return Some(BatteryInfo { pct: cap as u8, charging });
        }
        None
    }

    fn info(&mut self) -> Option<&BatteryInfo> {
        let now_ms = now_ms();
        if self.cache.is_none() || now_ms - self.last_read_ms > 10_000 {
            self.cache      = Self::read();
            self.last_read_ms = now_ms;
        }
        self.cache.as_ref()
    }
}

impl Widget for BatteryWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let s = text.measure("100%  \u{f0079}", theme.font_size); // wide enough
        (s + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let h = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        let Some(info) = self.info().cloned() else {
            // No battery — desktop, skip
            return;
        };

        // Icon: nf-md-battery_* variants
        let icon = if info.charging {
            "\u{f0084}" // nf-md-battery_charging
        } else {
            match info.pct {
                80..=100 => "\u{f0079}", // nf-md-battery
                60..=79  => "\u{f007a}",
                40..=59  => "\u{f007b}",
                20..=39  => "\u{f007c}",
                _        => "\u{f007d}", // nf-md-battery_alert
            }
        };

        let color = if info.pct < 20 && !info.charging {
            hex_color("#f07178") // red when low
        } else {
            hex_color(&ctx.theme.foreground)
        };

        let label = format!("{icon} {}%", info.pct);
        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }
}

fn read_int(path: impl AsRef<std::path::Path>) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
