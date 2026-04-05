//! River compositor backend — limited mode.
//!
//! River uses a tag-based model instead of numbered workspaces.
//! Tags 1–9 map to workspaces 1–9 (the conventional River keybind setup).
//!
//! ## Limitations
//! River exposes no IPC for window enumeration or active-tag queries.
//! This backend provides:
//!   - Tags 1–9 as labeled workspaces (no active-tag detection, no window list)
//!   - Workspace-switch via `riverctl set-focused-tags`
//!
//! Per-window commands (focus, close, float, pin, move) are not supported
//! because River has no window-by-id IPC. These are silently ignored.

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use woven_common::types::{Window, Workspace};

use super::backend::{CompositorBackend, WmCommand};

pub struct RiverBackend;

impl RiverBackend {
    pub fn new() -> Result<Self> { Ok(Self) }

    pub fn detect() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP")
            .map(|d| d.to_lowercase() == "river")
            .unwrap_or(false)
    }

    async fn riverctl(args: &[&str]) -> Result<String> {
        let out = Command::new("riverctl")
            .args(args)
            .output()
            .await
            .context("riverctl not found — is River installed and in PATH?")?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

#[async_trait]
impl CompositorBackend for RiverBackend {
    fn name(&self) -> &'static str { "river" }

    // No event stream — rely on the 2-second poll in main.rs.

    async fn workspaces(&self) -> Result<Vec<Workspace>> {
        // Return tags 1–9 as static workspaces.
        // River provides no IPC to query which tag is currently focused,
        // so active is always false (the bar still shows all 9 labels).
        Ok((1u32..=9)
            .map(|i| Workspace {
                id:      i,
                name:    i.to_string(),
                active:  false,
                windows: Vec::new(),
            })
            .collect())
    }

    async fn windows(&self) -> Result<Vec<Window>> {
        // River has no window-enumeration IPC.
        Ok(Vec::new())
    }

    async fn dispatch(&self, cmd: WmCommand) -> Result<()> {
        if let WmCommand::FocusWorkspace(id) = cmd {
            // Tags are a bitmask: workspace N → bit (N-1).
            let mask = 1u32 << id.saturating_sub(1);
            Self::riverctl(&["set-focused-tags", &mask.to_string()]).await?;
        }
        // All per-window commands are unsupported on River.
        Ok(())
    }
}
