//! Screencopy backend compatibility shim for woven-lite.
//!
//! Currently hardcodes zwlr (Niri only).
//! Once woven-protocols gains `detect_backend()` + `ext_image.rs`, replace
//! the body of `active_backend()` with a call to that and remove the build.rs warning.

/// Which screencopy backend is active at runtime.
/// Placeholder until woven-protocols dual-backend lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// zwlr-screencopy-unstable-v1 (Niri). Currently the only wired backend.
    Zwlr,
    /// ext-image-copy-capture-v1 (Sway / Hyprland). NOT YET WIRED.
    ExtImage,
}

/// Detect which backend to use based on compositor env vars.
/// Returns `Backend::Zwlr` for everything until ext_image.rs is implemented.
///
/// SWAP THIS OUT when woven-protocols::detect_backend() exists:
/// ```rust
/// pub fn active_backend() -> Backend {
///     woven_protocols::detect_backend().into()
/// }
/// ```
pub fn active_backend() -> Backend {
    // TODO: replace with woven-protocols runtime detection once dual-backend lands.
    if std::env::var("SWAYSOCK").is_ok()
        || std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
    {
        // We know it's Sway/Hyprland but ext_image isn't wired yet.
        // ScreencopyManager::spawn() will bail at runtime — that's intentional,
        // window cards will show placeholders until the backend is wired.
        tracing::warn!(
            "woven-lite: Sway/Hyprland detected but ext-image-copy-capture-v1 backend \
             is not yet implemented. Window thumbnails will use placeholders."
        );
        Backend::ExtImage
    } else {
        Backend::Zwlr
    }
}
