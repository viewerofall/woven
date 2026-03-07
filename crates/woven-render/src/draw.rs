//! Birds-eye overlay — 2×2 grid layout with pages.
//!
//! Screen is divided into a fixed 2×2 grid of workspace cells.
//! Page 0: workspaces 1-4, Page 1: workspaces 5-8, etc.
//! Empty slots show a placeholder with just the workspace number.
//! Scroll left/right to change pages.
//! Each cell always has the same large size — no thin columns, no degenerate paths.
//!
//! NO anti-aliased fill_path on small shapes. All rounded rects fall back to
//! plain fill_rect when too small, eliminating the tiny-skia hairline AA panic.

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};
use woven_common::types::{AnimationConfig, Theme, Workspace, WorkspaceMetrics};
use crate::text::TextRenderer;
use tracing::warn;

const TOP_H:    f32 = 48.0;
const GRID_PAD: f32 = 20.0;  // padding around the whole grid
const CELL_GAP: f32 = 14.0;  // gap between cells
const WIN_H:    f32 = 68.0;
const WIN_GAP:  f32 = 7.0;
const HDR_H:    f32 = 42.0;
const MET_H:    f32 = 26.0;
const PAGE_IND: f32 = 24.0;  // page indicator strip at bottom

// ── SysInfo ───────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct SysInfo {
    pub hostname:     String,
    pub distro:       String,
    pub kernel:       String,
    pub uptime_s:     u64,
    pub cpu_pct:      f32,
    pub mem_used_kb:  u64,
    pub mem_total_kb: u64,
    pub top_procs:    Vec<(String, f32)>,
}

impl SysInfo {
    pub fn collect() -> Self {
        SysInfo {
            hostname:     read_file("/etc/hostname").trim().to_string(),
            distro:       read_os_key("PRETTY_NAME").unwrap_or("Linux".into()),
            kernel:       read_file("/proc/sys/kernel/osrelease").trim().to_string(),
            uptime_s:     read_uptime(),
            cpu_pct:      read_cpu_pct(),
            mem_used_kb:  { let (u,_) = read_mem(); u },
            mem_total_kb: { let (_,t) = read_mem(); t },
            top_procs:    read_top_procs(4),
        }
    }
    pub fn uptime_str(&self) -> String {
        let h = self.uptime_s / 3600;
        let m = (self.uptime_s % 3600) / 60;
        if h > 0 { format!("{}h {}m", h, m) } else { format!("{}m", m) }
    }
}

fn read_file(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}
fn read_os_key(key: &str) -> Option<String> {
    for line in read_file("/etc/os-release").lines() {
        if line.starts_with(key) {
            return Some(line.splitn(2,'=').nth(1)?.trim_matches('"').to_string());
        }
    }
    None
}
fn read_uptime() -> u64 {
    read_file("/proc/uptime").split_whitespace().next()
    .and_then(|v| v.parse::<f64>().ok()).map(|v| v as u64).unwrap_or(0)
}
fn read_cpu_pct() -> f32 {
    let snap = || -> Option<(u64,u64)> {
        let s = std::fs::read_to_string("/proc/stat").ok()?;
        let nums: Vec<u64> = s.lines().next()?.split_whitespace().skip(1)
        .filter_map(|v| v.parse().ok()).collect();
        if nums.len() < 4 { return None; }
        Some((nums[3], nums.iter().sum()))
    };
    if let (Some((i1,t1)), _, Some((i2,t2))) = (
        snap(), std::thread::sleep(std::time::Duration::from_millis(25)), snap()
    ) {
        let dt = t2.saturating_sub(t1).max(1) as f32;
        let di = i2.saturating_sub(i1) as f32;
        return ((1.0 - di/dt) * 100.0).clamp(0.0, 100.0);
    }
    0.0
}
fn read_mem() -> (u64, u64) {
    let mut tot = 0u64; let mut avail = 0u64;
    for line in read_file("/proc/meminfo").lines() {
        if      line.starts_with("MemTotal:")     { tot   = parse_kb(line); }
        else if line.starts_with("MemAvailable:") { avail = parse_kb(line); }
    }
    (tot.saturating_sub(avail), tot)
}
fn parse_kb(line: &str) -> u64 {
    line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0)
}
fn read_top_procs(n: usize) -> Vec<(String, f32)> {
    let mut v: Vec<(String,u64)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for e in dir.flatten() {
            let nm = e.file_name(); let s = nm.to_string_lossy();
            if !s.chars().all(|c| c.is_ascii_digit()) { continue; }
            if let Ok(st) = std::fs::read_to_string(format!("/proc/{}/stat", s)) {
                let p: Vec<&str> = st.split_whitespace().collect();
                if p.len() > 14 {
                    let comm = p[1].trim_matches(|c| c=='('||c==')').to_string();
                    let t = p[13].parse::<u64>().unwrap_or(0) + p[14].parse::<u64>().unwrap_or(0);
                    v.push((comm, t));
                }
            }
        }
    }
    v.sort_by(|a,b| b.1.cmp(&a.1));
    let tot = v.iter().map(|p| p.1).sum::<u64>().max(1);
    v.iter().take(n).map(|(n,t)| (n.clone(), *t as f32/tot as f32*100.0)).collect()
}

// ── Icon glyphs ───────────────────────────────────────────────────────────────

fn app_icon(class: &str) -> &'static str {
    match class.to_lowercase().as_str() {
        c if c.contains("firefox")     => "\u{f269}",
        c if c.contains("chrome")
        || c.contains("chromium")    => "\u{f268}",
        c if c.contains("brave")       => "\u{e726}",
        c if c.contains("kitty")
        || c.contains("alacritty")
        || c.contains("foot")
        || c.contains("wezterm")     => "\u{e691}",
        c if c.contains("nvim")
        || c.contains("neovim")
        || c.contains("vim")         => "\u{e62b}",
        c if c.contains("code")
        || c.contains("vscode")
        || c.contains("zed")         => "\u{e70c}",
        c if c.contains("discord")     => "\u{f392}",
        c if c.contains("telegram")    => "\u{f2c6}",
        c if c.contains("slack")       => "\u{f198}",
        c if c.contains("spotify")     => "\u{f1bc}",
        c if c.contains("mpv")
        || c.contains("vlc")         => "\u{f04b}",
        c if c.contains("steam")       => "\u{f1b6}",
        c if c.contains("thunar")
        || c.contains("nautilus")
        || c.contains("dolphin")     => "\u{f07b}",
        c if c.contains("obs")         => "\u{f03d}",
        c if c.contains("gimp")
        || c.contains("inkscape")    => "\u{f1fc}",
        _                              => "\u{f2d0}",
    }
}

use crossbeam_channel::Sender;
use crate::thread::WindowAction;

// ── Button hit rect ───────────────────────────────────────────────────────────
// Stored each frame so on_press can hit-test without re-computing layout.

#[derive(Clone)]
struct ButtonRect {
    x: f32, y: f32, w: f32, h: f32,
    action: WindowAction,
}

impl ButtonRect {
    fn hit(&self, mx: f32, my: f32) -> bool {
        mx >= self.x && mx <= self.x + self.w &&
        my >= self.y && my <= self.y + self.h
    }
}

// ── Painter ───────────────────────────────────────────────────────────────────

pub struct Painter {
    theme:       Theme,
    #[allow(dead_code)]
    anims:       AnimationConfig,
    workspaces:  Vec<Workspace>,
    metrics:     Vec<WorkspaceMetrics>,
    sys:         SysInfo,
    sys_tick:    u32,
    text:        TextRenderer,
    pub page:    usize,
    // hover & click state
    mouse_x:     f32,
    mouse_y:     f32,
    _hover_win:  Option<String>,   // window id under cursor (reserved for future use)
    buttons:     Vec<ButtonRect>,  // rebuilt each frame
    action_tx:   Sender<WindowAction>,
}

impl Painter {
    pub fn new(theme: Theme, anims: AnimationConfig, action_tx: Sender<WindowAction>) -> Self {
        Self {
            theme, anims,
            workspaces: vec![],
            metrics:    vec![],
            sys:        SysInfo::default(),
            sys_tick:   999,
            text:       TextRenderer::new(),
            page:       0,
            mouse_x:    0.0,
            mouse_y:    0.0,
            _hover_win:  None,
            buttons:    vec![],
            action_tx,
        }
    }

    pub fn update_theme(&mut self, t: Theme)  { self.theme = t; }
    pub fn update_state(&mut self, ws: Vec<Workspace>, met: Vec<WorkspaceMetrics>) {
        self.workspaces = ws;
        self.metrics    = met;
    }

    pub fn next_page(&mut self) {
        let total_pages = ((self.workspaces.len().max(1) + 3) / 4).max(1);
        if self.page + 1 < total_pages { self.page += 1; }
    }
    pub fn prev_page(&mut self) {
        if self.page > 0 { self.page -= 1; }
    }

    pub fn on_scroll(&mut self, _sx: f64, dy: f64) { let _ = dy; }

    pub fn on_motion(&mut self, x: f64, y: f64) {
        self.mouse_x = x as f32;
        self.mouse_y = y as f32;
        // update hover_win by checking which window card the cursor is inside
        // buttons are rebuilt each frame so we just store position
    }

    pub fn on_press(&mut self, x: f64, y: f64) -> bool {
        let mx = x as f32;
        let my = y as f32;
        for btn in &self.buttons.clone() {
            if btn.hit(mx, my) {
                let _ = self.action_tx.try_send(btn.action.clone());
                return true;
            }
        }
        false
    }

    pub fn on_release(&mut self, _x: f64, _y: f64) {}

    // ── paint ─────────────────────────────────────────────────────────────────

    pub fn paint(&mut self, width: u32, height: u32) -> Vec<u8> {
        let mut pm = match Pixmap::new(width, height) {
            Some(p) => p,
            None    => {
                warn!("can't alloc {}x{}", width, height);
                return vec![0u8; (width * height * 4) as usize];
            }
        };

        self.sys_tick += 1;
        if self.sys_tick >= 240 {
            self.sys      = SysInfo::collect();
            self.sys_tick = 0;
        }

        let sw = width  as f32;
        let sh = height as f32;
        let theme = self.theme.clone();

        pm.fill(parse_color(&theme.background, theme.opacity));

        // clear hit rects — rebuilt during draw_grid below
        self.buttons.clear();

        self.draw_top_bar(&mut pm, sw, sh);
        self.draw_grid(&mut pm, sw, sh, &theme);
        self.draw_page_indicator(&mut pm, sw, sh, &theme);

        rgba_to_argb(pm.data())
    }

    // ── top bar ───────────────────────────────────────────────────────────────

    fn draw_top_bar(&mut self, pm: &mut Pixmap, sw: f32, _sh: f32) {
        let theme  = self.theme.clone();
        let sys    = self.sys.clone();
        let bar_h  = TOP_H;

        fill_rect(pm, 0.0, 0.0, sw, bar_h,
                  parse_color(&theme.background, 0.93));
        fill_rect(pm, 0.0, bar_h - 1.0, sw, 1.0,
                  parse_color(&theme.border, 0.22));

        let fs     = 13.0f32;
        let sm     = 11.0f32;
        let cy     = bar_h / 2.0;
        let pad    = 16.0f32;
        let accent = parse_color(&theme.accent, 1.0);
        let text_c = parse_color(&theme.text,   1.0);
        let dim_c  = parse_color(&theme.text,   0.45);
        let sep    = "  ·  ";

        let mut cx = pad;
        let hw = self.text.draw(pm, &sys.hostname, cx, cy - fs/2.0, fs, accent);
        cx += hw;
        let sw2 = self.text.draw(pm, sep, cx, cy - sm/2.0, sm, dim_c); cx += sw2;
        let dw  = self.text.draw(pm, &sys.distro,  cx, cy - sm/2.0, sm, text_c); cx += dw;
        let sw3 = self.text.draw(pm, sep, cx, cy - sm/2.0, sm, dim_c); cx += sw3;
        let ks  = sys.kernel.split('-').next().unwrap_or(&sys.kernel).to_string();
        let kw  = self.text.draw(pm, &ks, cx, cy - sm/2.0, sm, dim_c); cx += kw;
        let sw4 = self.text.draw(pm, sep, cx, cy - sm/2.0, sm, dim_c); cx += sw4;
        self.text.draw(pm, &format!("up {}", sys.uptime_str()), cx, cy - sm/2.0, sm, dim_c);

        let mut rx = sw - pad;
        for (name, _) in sys.top_procs.iter().take(3).rev() {
            let pw = self.text.measure(name, sm);
            rx -= pw;
            self.text.draw(pm, name, rx, cy - sm/2.0, sm, parse_color(&theme.border, 0.6));
            let sepw = self.text.measure(sep, sm);
            rx -= sepw;
            self.text.draw(pm, sep, rx, cy - sm/2.0, sm, dim_c);
        }
        let mem_s = format!("{:.1}/{:.0}G",
                            sys.mem_used_kb  as f32 / (1024.0*1024.0),
                            sys.mem_total_kb as f32 / (1024.0*1024.0));
        let mw = self.text.measure(&mem_s, sm);
        rx -= mw + pad;
        self.text.draw(pm, &mem_s, rx, cy - sm/2.0, sm, dim_c);

        let cpu_s = format!("cpu {:.0}%", sys.cpu_pct);
        let cpuw  = self.text.measure(&cpu_s, sm);
        rx -= cpuw + pad;
        let cpu_c = if sys.cpu_pct > 80.0 { Color::from_rgba8(243,139,168,255) }
        else if sys.cpu_pct > 50.0 { Color::from_rgba8(250,179,135,255) }
        else                        { accent };
        self.text.draw(pm, &cpu_s, rx, cy - sm/2.0, sm, cpu_c);
    }

    // ── 2×2 grid ──────────────────────────────────────────────────────────────

    fn draw_grid(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        // grid area sits below top bar and above page indicator
        let grid_top = TOP_H + GRID_PAD;
        let grid_bot = sh - PAGE_IND - GRID_PAD;
        let grid_h   = (grid_bot - grid_top).max(10.0);
        let grid_w   = sw - GRID_PAD * 2.0;

        // 2 columns, 2 rows → 4 cells
        let cell_w = (grid_w - CELL_GAP) / 2.0;
        let cell_h = (grid_h - CELL_GAP) / 2.0;

        let workspaces = self.workspaces.clone();
        let metrics    = self.metrics.clone();
        let page       = self.page;

        // clamp page to valid range
        let total_pages = ((workspaces.len().max(1) + 3) / 4).max(1);
        let page = page.min(total_pages - 1);

        for slot in 0..4usize {
            let col = slot % 2;
            let row = slot / 2;
            let x   = GRID_PAD + col as f32 * (cell_w + CELL_GAP);
            let y   = grid_top + row as f32 * (cell_h + CELL_GAP);

            let ws_idx = page * 4 + slot;
            let ws     = workspaces.get(ws_idx);
            let met    = ws.and_then(|w| metrics.iter().find(|m| m.workspace_id == w.id));
            let ws_num = (ws_idx + 1) as u32;

            self.draw_cell(pm, x, y, cell_w, cell_h, ws, met, ws_num, theme);
        }
    }

    fn draw_cell(
        &mut self,
        pm:     &mut Pixmap,
        x:      f32, y: f32, w: f32, h: f32,
        ws:     Option<&Workspace>,
        met:    Option<&WorkspaceMetrics>,
        ws_num: u32,
        theme:  &Theme,
    ) {
        let accent = parse_color(&theme.accent, 1.0);
        let dim_c  = parse_color(&theme.text,   0.4);
        let active = ws.map(|w| w.active).unwrap_or(false);
        let r      = (theme.border_radius as f32).min(w/2.0).min(h/2.0);

        // ── cell background ───────────────────────────────────────────────────
        // border ring: outer fill + inner fill (no stroke_path)
        let bdr_color = if active {
            parse_color(&theme.accent, 0.65)
        } else {
            parse_color(&theme.border, 0.18)
        };
        let bdr_w = if active { 2.0f32 } else { 1.0f32 };
        // outer (border color)
        fill_rrect(pm, x, y, w, h, r, bdr_color);
        // inner (cell bg)
        fill_rrect(pm,
                   x + bdr_w, y + bdr_w,
                   (w - bdr_w*2.0).max(0.0), (h - bdr_w*2.0).max(0.0),
                   (r - bdr_w).max(0.0),
                   parse_color(&theme.background, if active { 0.88 } else { 0.75 }));

        // active glow: a slightly larger outer ring
        if active {
            let gl = 4.0f32;
            fill_rrect(pm, x-gl, y-gl, w+gl*2.0, h+gl*2.0, r+gl,
                       parse_color(&theme.accent, 0.08));
            // re-draw cell bg to cover the glow overlap
            fill_rrect(pm, x, y, w, h, r, bdr_color);
            fill_rrect(pm,
                       x + bdr_w, y + bdr_w,
                       (w - bdr_w*2.0).max(0.0), (h - bdr_w*2.0).max(0.0),
                       (r - bdr_w).max(0.0),
                       parse_color(&theme.background, 0.88));
        }

        // ── header ────────────────────────────────────────────────────────────
        let hdr_h = HDR_H.min(h * 0.18);
        fill_rrect(pm, x, y, w, hdr_h, r,
                   if active { parse_color(&theme.accent, 0.18) }
                   else { parse_color(&theme.border, 0.08) });
        // square off bottom of header
        fill_rect(pm, x, y + hdr_h/2.0, w, hdr_h/2.0,
                  if active { parse_color(&theme.accent, 0.18) }
                  else { parse_color(&theme.border, 0.08) });
        // header bottom line
        fill_rect(pm, x, y + hdr_h - 1.0, w, 1.0,
                  parse_color(&theme.border, 0.18));

        let fs  = 13.0f32;
        let pad = 12.0f32;

        // workspace label
        let label = if let Some(ws) = ws {
            if ws.name.is_empty() || ws.name == ws.id.to_string() {
                format!("workspace {}", ws.id)
            } else {
                format!("{}  {}", ws.id, ws.name)
            }
        } else {
            format!("workspace {}", ws_num) // placeholder
        };
        self.text.draw(pm, &label,
                       x + pad, y + hdr_h/2.0 - fs/2.0, fs,
                       if active { accent } else { parse_color(&theme.text, 0.55) });

        // window count pill
        if let Some(ws) = ws {
            if !ws.windows.is_empty() {
                let cs    = 10.0f32;
                let cnt   = format!("{}", ws.windows.len());
                let cw    = self.text.measure(&cnt, cs) + 10.0;
                let ch    = cs + 6.0;
                let pill_x = x + w - cw - pad/2.0;
                let pill_y = y + hdr_h/2.0 - ch/2.0;
                fill_rrect(pm, pill_x, pill_y, cw, ch, ch/2.0,
                           if active { parse_color(&theme.accent, 0.30) }
                           else { parse_color(&theme.border, 0.20) });
                self.text.draw(pm, &cnt,
                               pill_x + 5.0, pill_y + ch/2.0 - cs/2.0, cs,
                               if active { accent } else { dim_c });
            }
        }

        // ── content ───────────────────────────────────────────────────────────
        if let Some(ws) = ws {
            if ws.windows.is_empty() {
                // empty workspace — show subtle "empty" label
                let em  = "empty";
                let efs = 11.0f32;
                let emw = self.text.measure(em, efs);
                self.text.draw(pm, em,
                               x + w/2.0 - emw/2.0, y + h/2.0 - efs/2.0,
                               efs, parse_color(&theme.border, 0.28));
            } else {
                self.draw_window_list(pm, x, y, w, h, hdr_h, ws, theme);
            }
        } else {
            // placeholder — large faded workspace number
            let nfs = (h * 0.35).min(80.0);
            let ns  = format!("{}", ws_num);
            let nw  = self.text.measure(&ns, nfs);
            self.text.draw(pm, &ns,
                           x + w/2.0 - nw/2.0, y + h/2.0 - nfs/2.0,
                           nfs, parse_color(&theme.border, 0.12));
        }

        // ── metrics bar ───────────────────────────────────────────────────────
        if let Some(m) = met {
            let mh      = MET_H.min(h * 0.08);
            let bar_y   = y + h - mh + mh * 0.2;
            let bar_h   = (mh * 0.22).max(2.0);
            let half    = (w - pad*2.0 - 8.0) / 2.0;
            let bfs     = 9.0f32;

            fill_rect(pm, x, y + h - mh, w, 1.0,
                      parse_color(&theme.border, 0.12));

            let cpu_pct = (m.cpu_total/100.0).clamp(0.0,1.0);
            let cpu_col = if cpu_pct > 0.8 { Color::from_rgba8(243,139,168,200) }
            else if cpu_pct > 0.5 { Color::from_rgba8(250,179,135,200) }
            else                   { accent };
            fill_rrect(pm, x+pad, bar_y, half, bar_h, bar_h/2.0,
                       parse_color(&theme.border, 0.15));
            if cpu_pct > 0.0 {
                fill_rrect(pm, x+pad, bar_y, half*cpu_pct, bar_h, bar_h/2.0, cpu_col);
            }
            self.text.draw(pm, "cpu", x+pad, bar_y+bar_h+1.0, bfs, dim_c);

            let mem_pct = (m.mem_total_kb as f32 / (32.0*1024.0*1024.0)).clamp(0.0,1.0);
            let mx = x + pad + half + 8.0;
            fill_rrect(pm, mx, bar_y, half, bar_h, bar_h/2.0,
                       parse_color(&theme.border, 0.15));
            if mem_pct > 0.0 {
                fill_rrect(pm, mx, bar_y, half*mem_pct, bar_h, bar_h/2.0,
                           parse_color(&theme.border, 0.6));
            }
            self.text.draw(pm, "mem", mx, bar_y+bar_h+1.0, bfs, dim_c);
        }
    }

    fn draw_window_list(
        &mut self,
        pm:    &mut Pixmap,
        cx:    f32, cy: f32, cw: f32, ch: f32,
        hdr_h: f32,
        ws:    &Workspace,
        theme: &Theme,
    ) {
        let pad     = 10.0f32;
        let win_h   = WIN_H.min((ch - hdr_h - MET_H - WIN_GAP*2.0) / 4.0).max(20.0);
        let win_gap = WIN_GAP;
        let text_c  = parse_color(&theme.text, 1.0);
        let dim_c   = parse_color(&theme.text, 0.4);
        let accent  = parse_color(&theme.accent, 1.0);
        let mut wy  = cy + hdr_h + win_gap;
        let max_y   = cy + ch - MET_H - win_gap;
        let mx      = self.mouse_x;
        let my      = self.mouse_y;

        let mut new_buttons: Vec<ButtonRect> = Vec::new();

        for win in &ws.windows.clone() {
            if wy + win_h > max_y { break; }

            let wx   = cx + pad;
            let ww   = cw - pad * 2.0;
            let cls  = class_color(&win.class);
            let id   = win.id.clone();

            // is mouse hovering this card?
            let hovered = mx >= wx && mx <= wx + ww && my >= wy && my <= wy + win_h;

            // card bg — brighter when hovered
            let cr = 5.0f32.min(win_h / 2.0).min(ww / 2.0);
            fill_rrect(pm, wx, wy, ww, win_h, cr,
                       parse_color(&theme.border, if hovered { 0.35 } else { 0.18 }));
            fill_rrect(pm, wx+1.0, wy+1.0, (ww-2.0).max(0.0), (win_h-2.0).max(0.0),
                       (cr-1.0).max(0.0),
                       parse_color(&theme.background, if hovered { 0.80 } else { 0.55 }));

            // left color strip
            fill_rrect(pm, wx, wy, 3.0, win_h, 1.5, cls);

            // icon circle
            let icon_r  = (win_h * 0.28).max(6.0);
            let icon_cx = wx + pad/2.0 + icon_r + 2.0;
            let icon_cy = wy + win_h / 2.0;
            fill_circle(pm, icon_cx, icon_cy, icon_r, with_alpha(cls, 0.20));

            let icon_str = app_icon(&win.class);
            let icon_fs  = (icon_r * 1.4).max(8.0);
            let icon_mw  = self.text.measure(icon_str, icon_fs);
            let (draw_str, draw_fs) = if icon_mw > 2.0 {
                (icon_str.to_string(), icon_fs)
            } else {
                let l = win.class.chars().find(|c| c.is_alphabetic())
                .map(|c| c.to_uppercase().to_string()).unwrap_or("?".into());
                (l, icon_fs * 0.85)
            };
            let dw = self.text.measure(&draw_str, draw_fs);
            self.text.draw(pm, &draw_str,
                           icon_cx - dw / 2.0,
                           icon_cy - draw_fs / 2.0 - draw_fs * 0.12,
                           draw_fs, cls);

            // app name + title (hide when hovering to make room for buttons)
            if !hovered {
                let name_x   = icon_cx + icon_r + 6.0;
                let name_fs  = (win_h * 0.22).clamp(8.0, 13.0);
                let title_fs = (name_fs * 0.82).max(7.0);
                let class_s  = if win.class.is_empty() { "unknown" } else { &win.class };
                self.text.draw(pm, class_s, name_x, wy + win_h * 0.18, name_fs, text_c);
                let title = truncate(&win.title, 36);
                self.text.draw(pm, &title, name_x,
                               wy + win_h * 0.18 + name_fs + 2.0, title_fs, dim_c);
            } else {
                // ── action buttons (shown on hover) ───────────────────────────
                // [ Focus ] [ Float ] [ Pin ] [ FS ] [ ✕ ]
                let btn_h   = (win_h * 0.52).max(16.0).min(28.0);
                let btn_fs  = (btn_h * 0.42).max(7.0).min(11.0);
                let btn_pad = 8.0f32;
                let btn_gap = 4.0f32;
                let btn_y   = wy + win_h / 2.0 - btn_h / 2.0;

                // measure labels first to compute widths
                let btns: &[(&str, WindowAction, [u8;4])] = &[
                    ("focus",  WindowAction::Focus(id.clone()),            [166,227,161,255]),
                    ("float",  WindowAction::ToggleFloat(id.clone()),      [137,180,250,255]),
                    ("pin",    WindowAction::TogglePin(id.clone()),        [203,166,247,255]),
                    ("fs",     WindowAction::ToggleFullscreen(id.clone()), [250,179,135,255]),
                    ("✕",     WindowAction::Close(id.clone()),             [243,139,168,255]),
                ];

                let name_x  = icon_cx + icon_r + 6.0;
                let mut bx  = name_x;

                for (label, action, rgba) in btns {
                    let lw    = self.text.measure(label, btn_fs);
                    let bw    = lw + btn_pad * 2.0;
                    let bcol  = Color::from_rgba8(rgba[0], rgba[1], rgba[2], 40);
                    let tcol  = Color::from_rgba8(rgba[0], rgba[1], rgba[2], 230);

                    // clamp so we don't spill past card right edge
                    if bx + bw > wx + ww - 4.0 { break; }

                    fill_rrect(pm, bx, btn_y, bw, btn_h, btn_h / 2.0, bcol);
                    self.text.draw(pm, label,
                                   bx + btn_pad, btn_y + btn_h / 2.0 - btn_fs / 2.0,
                                   btn_fs, tcol);

                    new_buttons.push(ButtonRect {
                        x: bx, y: btn_y, w: bw, h: btn_h,
                        action: action.clone(),
                    });

                    bx += bw + btn_gap;
                }

                let _ = accent; // used by window count pill above
            }

            wy += win_h + win_gap;
        }

        // push buttons collected this frame
        self.buttons.extend(new_buttons);

        // overflow label
        let shown = ((max_y - (cy + hdr_h + win_gap)) / (win_h + win_gap)) as usize;
        let total = ws.windows.len();
        if total > shown && shown > 0 {
            let more = total - shown;
            let msg  = format!("+{} more", more);
            let mfs  = 10.0f32;
            let mw   = self.text.measure(&msg, mfs);
            self.text.draw(pm, &msg,
                           cx + cw / 2.0 - mw / 2.0, max_y - mfs - 2.0,
                           mfs, parse_color(&theme.border, 0.5));
        }
    }

    // ── page indicator ────────────────────────────────────────────────────────

    fn draw_page_indicator(&mut self, pm: &mut Pixmap, sw: f32, sh: f32, theme: &Theme) {
        let total_pages = ((self.workspaces.len().max(1) + 3) / 4).max(1);
        if total_pages <= 1 { return; }

        let y   = sh - PAGE_IND + PAGE_IND/2.0 - 4.0;
        let dot = 6.0f32;
        let gap = 10.0f32;
        let total_w = total_pages as f32 * dot + (total_pages-1) as f32 * gap;
        let mut x = sw/2.0 - total_w/2.0;

        for i in 0..total_pages {
            let c = if i == self.page {
                parse_color(&theme.accent, 0.9)
            } else {
                parse_color(&theme.border, 0.35)
            };
            fill_circle(pm, x + dot/2.0, y, dot/2.0, c);
            x += dot + gap;
        }

        // scroll hint
        let hint = "scroll to change page";
        let hfs  = 9.0f32;
        let hw   = self.text.measure(hint, hfs);
        self.text.draw(pm, hint, sw/2.0 - hw/2.0, sh - hfs - 2.0,
                       hfs, parse_color(&theme.text, 0.18));
    }
}

// ── primitives ────────────────────────────────────────────────────────────────

fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if w < 0.5 || h < 0.5 || !x.is_finite() || !y.is_finite() { return; }
    let mut p = Paint::default();
    p.set_color(c);
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
}

/// Rounded rect fill. Falls back to plain rect when too small to round safely.
/// anti_alias is DISABLED — prevents tiny-skia's hairline AA panic entirely.
fn fill_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    if w < 1.0 || h < 1.0 || !x.is_finite() || !y.is_finite() { return; }
    let r = r.min(w/2.0).min(h/2.0).max(0.0);
    // Too small to round, or radius negligible → plain rect (safe code path)
    if r < 1.5 || w < 6.0 || h < 6.0 {
        fill_rect(pm, x, y, w, h, c);
        return;
    }
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = false; // ← KEY: hairline AA only fires when anti_alias=true
    if let Some(path) = rrect_path(x, y, w, h, r) {
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, c: Color) {
    if r < 0.5 || !cx.is_finite() || !cy.is_finite() { return; }
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = false;
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut b = PathBuilder::new();
    b.move_to(x+r,   y);       b.line_to(x+w-r, y);
    b.quad_to(x+w,y,   x+w,y+r);
    b.line_to(x+w, y+h-r);
    b.quad_to(x+w,y+h, x+w-r,y+h);
    b.line_to(x+r, y+h);
    b.quad_to(x,y+h,   x,y+h-r);
    b.line_to(x,   y+r);
    b.quad_to(x,y,     x+r,y);
    b.close();
    b.finish()
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_color(hex: &str, alpha: f32) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 { return Color::from_rgba8(30,30,46,(alpha*255.0) as u8); }
    let r = u8::from_str_radix(&hex[0..2],16).unwrap_or(30);
    let g = u8::from_str_radix(&hex[2..4],16).unwrap_or(30);
    let b = u8::from_str_radix(&hex[4..6],16).unwrap_or(46);
    Color::from_rgba8(r, g, b, (alpha*255.0) as u8)
}

fn class_color(class: &str) -> Color {
    let h = class.bytes().fold(5381u32, |a,b| a.wrapping_mul(33).wrapping_add(b as u32));
    Color::from_rgba8(
        100u8.saturating_add(((h>>16)&0x9F) as u8),
                      100u8.saturating_add(((h>> 8)&0x9F) as u8),
                      100u8.saturating_add(((h    )&0x9F) as u8),
                      255,
    )
}

fn with_alpha(mut c: Color, a: f32) -> Color { c.set_alpha(a); c }

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { format!("{}…", s.chars().take(n-1).collect::<String>()) }
}

fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        out.push(px[2]); out.push(px[1]); out.push(px[0]); out.push(px[3]);
    }
    out
}
