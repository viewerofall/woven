//! Window thumbnail capture — stubbed pending a working protocol binding.
//!
//! TODO: implement via hyprland-toplevel-export-v1 once generate_client_code!
//! macro issues are resolved (it requires being in a dedicated protocol crate).

use std::collections::HashMap;

pub type Thumbnail      = (u32, u32, Vec<u8>);
pub type ThumbnailCache = HashMap<String, Thumbnail>;

pub struct ThumbnailCapturer {
    cache: ThumbnailCache,
}

impl ThumbnailCapturer {
    /// Always returns None until the protocol bindings are wired up.
    pub fn new() -> Option<Self> {
        None
    }

    pub fn request_all(&mut self, _windows: &[(&str, u32)]) {}

    pub fn pump_and_collect(&mut self) -> &ThumbnailCache {
        &self.cache
    }

    pub fn cache(&self) -> &ThumbnailCache { &self.cache }
    pub fn clear(&mut self) { self.cache.clear(); }
}
