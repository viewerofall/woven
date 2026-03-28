//! Window and output thumbnail cache — backed by `woven_protocols::ScreencopyManager`.
//!
//! Three capture kinds, each with its own ID namespace:
//!   - Window region (0x0000…–0xFFFE_FFFF_FFFF_FFFF): per-window crop
//!   - Full output   (OUTPUT_BASE | output_idx):        full output screenshot → output_cache
//!   - Workspace     (WS_BASE | ws_id):                 workspace-tagged screenshot → workspace_cache

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tracing::{info, warn};
use woven_protocols::{CaptureRequest, ScreencopyManager};
use woven_common::types::{Rect, Window};

/// (width, height, XRGB8888 pixels)
pub type Thumbnail = (u32, u32, Vec<u8>);

/// window_id_str → scaled thumbnail pixels
pub type ThumbnailCache = HashMap<String, Thumbnail>;

/// output_idx → full output thumbnail
pub type OutputCache = HashMap<usize, Thumbnail>;

/// workspace_id → workspace screenshot
pub type WorkspaceCache = HashMap<u32, Thumbnail>;

const THUMB_W: u32 = 320;
const THUMB_H: u32 = 180;

/// ID prefix for full-output captures (output_idx in low bits).
const OUTPUT_BASE: u64 = 0xFFFF_FF00_0000_0000;
/// ID prefix for workspace-tagged output captures (ws_id in low bits).
const WS_BASE:     u64 = 0xFFFF_FFFE_0000_0000;

pub struct ThumbnailCapturer {
    manager: ScreencopyManager,
    cache:          ThumbnailCache,
    output_cache:   OutputCache,
    workspace_cache: WorkspaceCache,
    in_flight:           HashMap<u64, String>,   // capture_id → window id str
    in_flight_outputs:   HashMap<u64, usize>,    // capture_id → output_idx
    in_flight_workspaces: HashMap<u64, u32>,     // capture_id → ws_id
}

impl ThumbnailCapturer {
    pub fn new() -> Option<Self> {
        match ScreencopyManager::spawn() {
            Ok(mgr) => Some(Self {
                manager:              mgr,
                cache:                HashMap::new(),
                output_cache:         HashMap::new(),
                workspace_cache:      HashMap::new(),
                in_flight:            HashMap::new(),
                in_flight_outputs:    HashMap::new(),
                in_flight_workspaces: HashMap::new(),
            }),
            Err(e) => {
                warn!("screencopy unavailable: {e:#} — overview will use placeholder cards");
                None
            }
        }
    }

    // ── per-window captures ────────────────────────────────────────────────────

    pub fn request_all(&mut self, windows: &[(&str, &Rect)]) {
        let requests: Vec<CaptureRequest> = windows
            .iter()
            .filter(|(_, rect)| rect.w > 0 && rect.h > 0)
            .map(|(id, rect)| {
                let wid = hash_id(id);
                self.in_flight.insert(wid, id.to_string());
                CaptureRequest {
                    window_id:   wid,
                    output_idx:  0,
                    full_output: false,
                    x: rect.x, y: rect.y, w: rect.w, h: rect.h,
                }
            })
            .collect();
        info!("screencopy: requesting {} window thumbnails", requests.len());
        self.manager.request_batch(requests);
    }

    pub fn request_windows(&mut self, windows: &[Window]) {
        let pairs: Vec<(&str, &Rect)> = windows
            .iter()
            .map(|w| (w.id.as_str(), &w.geometry))
            .collect();
        self.request_all(&pairs);
    }

    // ── full-output captures ───────────────────────────────────────────────────

    /// Capture the full output, store in `output_cache[output_idx]`.
    /// Used as the overlay backdrop.
    pub fn request_output(&mut self, output_idx: usize) {
        let id = OUTPUT_BASE | output_idx as u64;
        self.in_flight_outputs.insert(id, output_idx);
        self.manager.request(CaptureRequest {
            window_id: id, output_idx, full_output: true,
            x: 0, y: 0, w: 0, h: 0,
        });
    }

    /// Capture the full output, store in `workspace_cache[ws_id]`.
    /// Call this when workspace `ws_id` becomes active so we build a per-workspace
    /// screenshot library over time.
    pub fn request_output_for_ws(&mut self, ws_id: u32, output_idx: usize) {
        let id = WS_BASE | ws_id as u64;
        self.in_flight_workspaces.insert(id, ws_id);
        self.manager.request(CaptureRequest {
            window_id: id, output_idx, full_output: true,
            x: 0, y: 0, w: 0, h: 0,
        });
    }

    // ── drain results ──────────────────────────────────────────────────────────

    pub fn pump(&mut self) {
        for frame in self.manager.drain() {
            let id = frame.window_id;

            if let Some(ws_id) = self.in_flight_workspaces.remove(&id) {
                let pixels = frame.scale_nearest(frame.width, frame.height);
                info!("screencopy: workspace {} captured {}×{}", ws_id, frame.width, frame.height);
                self.workspace_cache.insert(ws_id, (frame.width, frame.height, pixels));

            } else if let Some(output_idx) = self.in_flight_outputs.remove(&id) {
                let pixels = frame.scale_nearest(frame.width, frame.height);
                info!("screencopy: output[{}] captured {}×{}", output_idx, frame.width, frame.height);
                self.output_cache.insert(output_idx, (frame.width, frame.height, pixels));

            } else if let Some(id_str) = self.in_flight.remove(&id) {
                let pixels = frame.scale_nearest(THUMB_W, THUMB_H);
                self.cache.insert(id_str, (THUMB_W, THUMB_H, pixels));
            }
        }
    }

    // ── accessors ──────────────────────────────────────────────────────────────

    pub fn get(&self, window_id: &str) -> Option<&Thumbnail> { self.cache.get(window_id) }
    pub fn cache(&self) -> &ThumbnailCache { &self.cache }
    pub fn output_cache(&self) -> &OutputCache { &self.output_cache }
    pub fn workspace_cache(&self) -> &WorkspaceCache { &self.workspace_cache }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.in_flight.clear();
        // output_cache and workspace_cache persist across overlay opens.
    }
}

fn hash_id(id: &str) -> u64 {
    // Niri: numeric ids. Hyprland: hex "0x1a2b3c".
    if let Some(hex) = id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")) {
        if let Ok(v) = u64::from_str_radix(hex, 16) { return v; }
    }
    if let Ok(v) = id.parse::<u64>() { return v; }
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}
