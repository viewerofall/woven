//! Network status widget — reads WiFi SSID via iw or /proc/net/wireless.

use super::{RenderCtx, Widget};
use crate::draw::hex_color;

pub struct NetworkWidget {
    cache:        Option<NetInfo>,
    last_read_ms: u64,
}

#[derive(Clone)]
struct NetInfo {
    ssid:    Option<String>,
    wired:   bool,
}

impl NetworkWidget {
    pub fn new() -> Self {
        Self { cache: None, last_read_ms: 0 }
    }

    fn read() -> NetInfo {
        // Try to get WiFi SSID via iw
        if let Some(ssid) = read_ssid_iw() {
            return NetInfo { ssid: Some(ssid), wired: false };
        }
        // Check for active wired interface
        if has_wired() {
            return NetInfo { ssid: None, wired: true };
        }
        NetInfo { ssid: None, wired: false }
    }

    fn info(&mut self) -> &NetInfo {
        let now_ms = now_ms();
        if self.cache.is_none() || now_ms - self.last_read_ms > 5_000 {
            self.cache        = Some(Self::read());
            self.last_read_ms = now_ms;
        }
        self.cache.as_ref().unwrap()
    }
}

impl Widget for NetworkWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        let w = text.measure("\u{f05a9} MyNetwork", theme.font_size);
        (w + 16.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let h  = ctx.height as f32;
        let ty = (h - ctx.theme.font_size) / 2.0;

        let info = self.info().clone();

        let (icon, label, color) = if let Some(ssid) = &info.ssid {
            ("\u{f05a9}", ssid.clone(), hex_color(&ctx.theme.foreground)) // nf-md-wifi
        } else if info.wired {
            ("\u{f0200}", "eth".into(), hex_color(&ctx.theme.foreground))  // nf-md-ethernet
        } else {
            ("\u{f05aa}", "disconnected".into(), hex_color(&ctx.theme.dim)) // nf-md-wifi_off
        };

        let text = format!("{icon} {label}");
        ctx.text.draw(ctx.pixmap, &text, x + 8.0, ty, ctx.theme.font_size, color);
    }
}

fn read_ssid_iw() -> Option<String> {
    // `iw dev` lists interfaces; find one with an SSID
    let out = std::process::Command::new("iw")
        .args(["dev"])
        .output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);

    // Find "Interface <name>" lines then run `iw <name> link`
    for line in s.lines() {
        let line = line.trim();
        if let Some(iface) = line.strip_prefix("Interface ") {
            let link = std::process::Command::new("iw")
                .args(["dev", iface, "link"])
                .output().ok()?;
            let ls = String::from_utf8_lossy(&link.stdout);
            for ll in ls.lines() {
                let ll = ll.trim();
                if let Some(ssid) = ll.strip_prefix("SSID: ") {
                    return Some(ssid.to_string());
                }
            }
        }
    }
    None
}

fn has_wired() -> bool {
    // Check /sys/class/net for eth* / en* interfaces that are up
    let Ok(dir) = std::fs::read_dir("/sys/class/net") else { return false; };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if n.starts_with("eth") || n.starts_with("en") {
            let carrier = std::fs::read_to_string(entry.path().join("carrier"))
                .unwrap_or_default();
            if carrier.trim() == "1" { return true; }
        }
    }
    false
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
