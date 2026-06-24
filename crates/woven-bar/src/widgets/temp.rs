//! Temperature widget — CPU (k10temp) + GPU (amdgpu).
//! Reads from hwmon by driver name so it survives reboots even if hwmon index shifts.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct TempWidget {
    cpu_c:   Option<f32>,
    gpu_c:   Option<f32>,
    last_ms: u64,
}

impl TempWidget {
    pub fn new() -> Self {
        let mut w = Self { cpu_c: None, gpu_c: None, last_ms: 0 };
        w.refresh_inner();
        w
    }

    fn refresh_inner(&mut self) {
        self.cpu_c = read_hwmon_temp("k10temp", "temp1");  // Tdie / Tccd
        self.gpu_c = read_hwmon_temp("amdgpu", "temp2");   // Junction (hotspot) temp
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 2000 { return; }
        self.last_ms = now;
        self.refresh_inner();
    }
}

impl Widget for TempWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        (text.measure("\u{f0290} 99° \u{f0c7e} 99°", theme.font_size) + 16.0) as u32
        // nf-md-thermometer + nf-md-gpu
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        let mut parts: Vec<String> = Vec::new();

        if let Some(c) = self.cpu_c {
            let color = temp_color(c, ctx.theme);
            let s = format!("\u{f0290} {:.0}°", c);
            let tw = ctx.text.measure(&s, ctx.theme.font_size);
            ctx.text.draw(ctx.pixmap, &s, x + 8.0, ty, ctx.theme.font_size, color);
            parts.push((tw + 8.0).to_string());
        }

        if let Some(g) = self.gpu_c {
            let color = temp_color(g, ctx.theme);
            let cpu_w = self.cpu_c.map(|c| {
                ctx.text.measure(&format!("\u{f0290} {:.0}°", c), ctx.theme.font_size) + 8.0
            }).unwrap_or(0.0);
            let s = format!("\u{f0c7e} {:.0}°", g);
            ctx.text.draw(ctx.pixmap, &s, x + 8.0 + cpu_w, ty, ctx.theme.font_size, color);
        }
    }
}

fn temp_color(c: f32, theme: &crate::config::Theme) -> tiny_skia::Color {
    if c > 85.0      { hex_color("#f07178") }   // red — hot
    else if c > 70.0 { hex_color("#ffcb6b") }   // yellow — warm
    else             { hex_color(&theme.foreground) }
}

/// Find hwmon entry by driver name and read a temp_input file.
/// Returns degrees Celsius (raw value is millidegrees).
fn read_hwmon_temp(driver: &str, temp_file: &str) -> Option<f32> {
    let dir = std::fs::read_dir("/sys/class/hwmon").ok()?;
    for entry in dir.flatten() {
        let path = entry.path();
        let name = std::fs::read_to_string(path.join("name"))
            .unwrap_or_default();
        if name.trim() != driver { continue; }
        let raw = std::fs::read_to_string(path.join(format!("{temp_file}_input")))
            .ok()?;
        let milli: f32 = raw.trim().parse().ok()?;
        return Some(milli / 1000.0);
    }
    None
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
