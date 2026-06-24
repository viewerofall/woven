//! woven-bar — standalone Wayland layer-shell status bar (also embedded in woven)

use anyhow::Result;
use std::sync::{Arc, atomic::AtomicBool};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("woven_bar=info".parse().unwrap()))
        .init();

    woven_bar::run(woven_bar::BarConfig::default(), Arc::new(AtomicBool::new(false)))
}
