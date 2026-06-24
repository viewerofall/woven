//! Weather widget — fetches current temperature from Open-Meteo (free, no API key).
//! Configure location in bar.toml [weather] section, or defaults to env WOVEN_LAT / WOVEN_LON.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct WeatherWidget {
    temp_c:    Option<f32>,
    code:      Option<u32>,
    last_ms:   u64,
    lat:       f64,
    lon:       f64,
}

impl WeatherWidget {
    pub fn new(lat: f64, lon: f64) -> Self {
        let mut w = Self { temp_c: None, code: None, last_ms: 0, lat, lon };
        w.refresh_inner();
        w
    }

    fn refresh_inner(&mut self) {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code&temperature_unit=fahrenheit",
            self.lat, self.lon
        );
        match ureq::get(&url).call() {
            Ok(resp) => {
                if let Ok(body) = resp.into_body().read_to_string() {
                    self.parse_response(&body);
                }
            }
            Err(_) => {} // silently retry next poll
        }
    }

    fn parse_response(&mut self, body: &str) {
        // Minimal JSON parsing — avoid pulling in a full JSON parser just for two fields.
        // Format: {"current":{"temperature_2m":72.3,"weather_code":0,...}}
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(current) = v.get("current") {
                self.temp_c = current.get("temperature_2m").and_then(|v| v.as_f64()).map(|f| f as f32);
                self.code   = current.get("weather_code").and_then(|v| v.as_u64()).map(|u| u as u32);
            }
        }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 600_000 { return; } // 10 minutes
        self.last_ms = now;
        self.refresh_inner();
    }
}

impl Widget for WeatherWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let sample = match &self.temp_c {
            Some(t) => format!("{} {:.0}°F", wmo_icon(self.code.unwrap_or(0)), t),
            None    => format!("{} --°F", wmo_icon(0)),
        };
        (text.measure(&sample, theme.font_size) + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;
        let fg = hex_color(&ctx.theme.foreground);

        let icon = wmo_icon(self.code.unwrap_or(0));
        let label = match self.temp_c {
            Some(t) => format!("{icon} {t:.0}°F"),
            None    => format!("{icon} --°F"),
        };

        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, fg);
    }
}

/// Map WMO weather code to a Nerd Font icon.
fn wmo_icon(code: u32) -> &'static str {
    match code {
        0      => "\u{f0599}",  // nf-md-weather_sunny — Clear sky
        1..=3  => "\u{f0595}",  // nf-md-weather_partly_cloudy — Partly cloudy
        45 | 48 => "\u{f0591}", // nf-md-weather_fog
        51..=57 => "\u{f0597}", // nf-md-weather_rainy — Drizzle
        61..=67 => "\u{f0597}", // nf-md-weather_rainy — Rain
        71..=77 => "\u{f059b}", // nf-md-weather_snowy — Snow
        80..=82 => "\u{f0597}", // nf-md-weather_rainy — Showers
        85 | 86 => "\u{f059b}", // nf-md-weather_snowy — Snow showers
        95..=99 => "\u{f0593}", // nf-md-weather_lightning — Thunderstorm
        _       => "\u{f0590}", // nf-md-weather_cloudy — fallback
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
