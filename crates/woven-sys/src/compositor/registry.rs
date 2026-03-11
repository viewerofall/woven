//! Plug-and-play backend registry.
//!
//! Drop a new backend file in crates/woven-sys/src/compositor/ that implements
//! CompositorBackend, add it to BACKENDS below, and it auto-detects at startup.
//! No other files need changing.
//!
//! Detection order matters — put more specific backends first.

use anyhow::{bail, Result};
use std::sync::Arc;
use tracing::info;

use super::backend::CompositorBackend;
use super::hyprland::HyprlandBackend;
use super::niri::NiriBackend;
use super::sway::SwayBackend;

/// Try each registered backend in order, return the first that detects.
pub fn detect_backend() -> Result<Arc<dyn CompositorBackend>> {
    // ── Register backends here ────────────────────────────────────────────────
    // Each entry: (name, detect_fn, constructor_fn)
    // To add a backend: impl CompositorBackend, add a line below. That's it.

    type Constructor = Box<dyn Fn() -> Result<Arc<dyn CompositorBackend>>>;

    let backends: Vec<(&str, bool, Constructor)> = vec![
        (
            "hyprland",
         HyprlandBackend::detect(),
         Box::new(|| Ok(Arc::new(HyprlandBackend::new()?) as Arc<dyn CompositorBackend>)),
        ),
        (
            "niri",
         NiriBackend::detect(),
         Box::new(|| Ok(Arc::new(NiriBackend::new()?) as Arc<dyn CompositorBackend>)),
        ),
        (
            "sway",
         SwayBackend::detect(),
         Box::new(|| Ok(Arc::new(SwayBackend::new()?) as Arc<dyn CompositorBackend>)),
        ),
    ];

    for (name, detected, constructor) in backends {
        if detected {
            info!("compositor: detected {}", name);
            return constructor();
        }
    }

    bail!(
        "No supported compositor detected.\n\
Currently supported: Hyprland, Niri, Sway\n\
To add support for your compositor, implement CompositorBackend\n\
in crates/woven-sys/src/compositor/<n>.rs and register it in registry.rs"
    )
}
