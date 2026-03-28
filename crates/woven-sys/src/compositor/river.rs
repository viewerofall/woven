//! River compositor backend.
//!
//! River uses a tag-based model instead of numbered workspaces.
//! Tags 1–32 are bitmasks on each output; woven maps tags 1–9 to
//! workspaces 1–9, which matches the conventional River keybind setup.
//!
//! ## Limitations
//! River's `riverctl` CLI is command-only — it cannot enumerate windows or
//! query which tags currently have views. This backend:
//!   - Returns tags 1–9 as workspaces (no live window list)
//!   - Sends commands via `riverctl` subprocess
//!   - Falls back to the 2-second poll (no event stream)
//!
//! Full window enumeration via `wlr-foreign-toplevel-management-v1` is planned
//! for a future release.

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use woven_common::types::{Window, Workspace};

use super::backend::{CompositorBackend, WmCommand};

pub struct RiverBackend;

impl RiverBackend {
    pub fn new() -> Result<Self> { Ok(Self) }

    /// River is running if XDG_CURRENT_DESKTOP=river, or if the `river`
    /// process is present in /proc (fallback for compositors that don't
    /// set that variable).
    pub fn detect() -> bool {
        if std::env::var("XDG_CURRENT_DESKTOP")
            .map(|d| d.to_lowercase() == "river")
            .unwrap_or(false)
        {
            return true;
        }
        // Fallback: scan /proc/*/comm for a running "river" process.
        std::fs::read_dir("/proc")
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
                    .any(|comm| comm.trim() == "river")
            })
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
        // River has no workspace concept; tags are the unit of organisation.
        // Return tags 1–9 as workspaces. Tag 1 is shown as active by default
        // since we cannot query the current focused-tags bitmask without a
        // full wlr-status Wayland client.
        let workspaces = (1u32..=9)
            .map(|i| Workspace {
                id:      i,
                name:    i.to_string(),
                active:  i == 1,
                windows: Vec::new(),
            })
            .collect();
        Ok(workspaces)
    }

    async fn windows(&self) -> Result<Vec<Window>> {
        // riverctl has no list-views command.
        // wlr-foreign-toplevel-management-v1 support is planned.
        Ok(Vec::new())
    }

    async fn dispatch(&self, cmd: WmCommand) -> Result<()> {
        match cmd {
            WmCommand::FocusWorkspace(id) => {
                // River tags are a bitmask: workspace N → bit (N-1).
                let mask = 1u32 << id.saturating_sub(1);
                Self::riverctl(&["set-focused-tags", &mask.to_string()]).await?;
            }
            WmCommand::FocusWindow(_) => {
                // River has no focus-by-id; focus is seat/position-based.
            }
            WmCommand::CloseWindow(_) => {
                // Closes the currently focused view — best we can do.
                Self::riverctl(&["close"]).await?;
            }
            WmCommand::FullscreenWindow(_) => {
                Self::riverctl(&["toggle-fullscreen"]).await?;
            }
            WmCommand::ToggleFloat(_) => {
                Self::riverctl(&["toggle-float"]).await?;
            }
            WmCommand::TogglePin(_) => {
                // River has no sticky/pin concept.
            }
            WmCommand::MoveWindow { workspace: ws, .. } |
            WmCommand::MoveToWorkspace { ws, .. } => {
                // Move the focused view to a tag.
                let mask = 1u32 << ws.saturating_sub(1);
                Self::riverctl(&["set-view-tags", &mask.to_string()]).await?;
            }
        }
        Ok(())
    }
}
