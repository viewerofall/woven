//! Disk usage widget — shows used/total for a mount point (default: /).

use super::{RenderCtx, Widget};
use crate::draw::hex_color;
use crate::widgets::cpu::usage_color;

pub struct DiskWidget {
    mount:   String,
    used_gb: f32,
    total_gb: f32,
    last_ms: u64,
}

impl DiskWidget {
    pub fn new(mount: &str) -> Self {
        let mut w = Self {
            mount:    mount.to_string(),
            used_gb:  0.0,
            total_gb: 0.0,
            last_ms:  0,
        };
        w.refresh_inner();
        w
    }

    fn refresh_inner(&mut self) {
        if let Some((used, total)) = statvfs(&self.mount) {
            self.used_gb  = used;
            self.total_gb = total;
        }
    }

    fn refresh(&mut self) {
        let now = now_ms();
        if now - self.last_ms < 30_000 { return; } // disk changes slowly
        self.last_ms = now;
        self.refresh_inner();
    }
}

impl Widget for DiskWidget {
    fn width(&self, theme: &crate::config::Theme, text: &mut crate::text::TextRenderer) -> u32 {
        (text.measure("\u{f0a0} 999/999G", theme.font_size) + 16.0) as u32 // nf-fa-hdd_o
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        self.refresh();
        let h    = ctx.height as f32;
        let ty   = (h - ctx.theme.font_size) / 2.0;
        let pct  = self.used_gb / self.total_gb.max(0.001) * 100.0;
        let color = usage_color(pct, ctx.theme);
        let label = format!("\u{f0a0} {:.0}/{:.0}G", self.used_gb, self.total_gb);
        ctx.text.draw(ctx.pixmap, &label, x + 8.0, ty, ctx.theme.font_size, color);
    }
}

/// Returns (used_gb, total_gb) for the given mount point using statvfs syscall.
fn statvfs(path: &str) -> Option<(f32, f32)> {
    use std::ffi::CString;
    let cpath = CString::new(path).ok()?;
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(cpath.as_ptr(), &mut st) != 0 { return None; }
        let block = st.f_frsize as f64;
        let total = st.f_blocks as f64 * block / 1e9;
        let avail = st.f_bavail as f64 * block / 1e9;
        let used  = total - avail;
        Some((used as f32, total as f32))
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
