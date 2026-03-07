//! Hyprland IPC backend
//! Communicates via unix sockets:
//!   $HYPRLAND_INSTANCE_SIGNATURE -> .socket.sock  (commands)
//!   $HYPRLAND_INSTANCE_SIGNATURE -> .socket2.sock (events)

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::{debug, warn};
use woven_common::types::{Rect, Window, Workspace};

use super::backend::{CompositorBackend, WmCommand};

pub struct HyprlandBackend {
    socket_path: String,
}

impl HyprlandBackend {
    pub fn new() -> Result<Self> {
        let sig = env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .context("HYPRLAND_INSTANCE_SIGNATURE not set — is Hyprland running?")?;
        let base = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let socket_path = format!("{}/hypr/{}/.socket.sock", base, sig);
        Ok(Self { socket_path })
    }

    /// Send a raw command to Hyprland socket, get response string
    async fn send(&self, cmd: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path)
        .await
        .context("Failed to connect to Hyprland socket")?;
        stream.write_all(cmd.as_bytes()).await?;
        let mut buf = String::new();
        stream.read_to_string(&mut buf).await?;
        debug!("hyprland <- {} | {} bytes", cmd, buf.len());
        Ok(buf)
    }

    /// j/workspaces returns JSON array of workspace objects
    async fn fetch_workspaces_raw(&self) -> Result<serde_json::Value> {
        let raw = self.send("j/workspaces").await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// j/clients returns JSON array of window objects
    async fn fetch_clients_raw(&self) -> Result<serde_json::Value> {
        let raw = self.send("j/clients").await?;
        Ok(serde_json::from_str(&raw)?)
    }
}

#[async_trait]
impl CompositorBackend for HyprlandBackend {
    fn name(&self) -> &'static str { "hyprland" }

    fn detect() -> bool {
        env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>> {
        let clients = self.fetch_clients_raw().await?;
        let ws_raw  = self.fetch_workspaces_raw().await?;

        let workspaces = ws_raw.as_array()
        .context("Expected array from j/workspaces")?
        .iter()
        .map(|w| {
            let id   = w["id"].as_u64().unwrap_or(0) as u32;
            let name = w["name"].as_str().unwrap_or("").to_string();

            // collect windows belonging to this workspace
            let windows = clients.as_array()
            .map(|arr| {
                arr.iter()
                .filter(|c| c["workspace"]["id"].as_u64().unwrap_or(0) as u32 == id)
                .map(parse_window)
                .collect()
            })
            .unwrap_or_default();

            Workspace { id, name, active: false, windows }
        })
        .collect();

        Ok(workspaces)
    }

    async fn windows(&self) -> Result<Vec<Window>> {
        let raw = self.fetch_clients_raw().await?;
        Ok(raw.as_array()
        .context("Expected array from j/clients")?
        .iter()
        .map(parse_window)
        .collect())
    }

    async fn dispatch(&self, cmd: WmCommand) -> Result<()> {
        let dispatch_str = match cmd {
            WmCommand::FocusWindow(id) =>
            format!("dispatch focuswindow address:{}", id),
                WmCommand::CloseWindow(id) =>
                format!("dispatch closewindow address:{}", id),
                    WmCommand::FullscreenWindow(id) =>
                    format!("dispatch fullscreen address:{}", id),
                        WmCommand::ToggleFloat(id) =>
                        format!("dispatch togglefloating address:{}", id),
                            WmCommand::TogglePin(id) =>
                            format!("dispatch pin address:{}", id),
                                WmCommand::MoveWindow { id, workspace } =>
                                format!("dispatch movetoworkspace {}, address:{}", workspace, id),
                                    WmCommand::MoveToWorkspace { id, ws } =>
                                    format!("dispatch movetoworkspace {}, address:{}", ws, id),
        };

        let resp = self.send(&dispatch_str).await?;
        if resp.trim() != "ok" {
            warn!("Hyprland dispatch returned: {}", resp.trim());
        }
        Ok(())
    }
}

/// Parse a Hyprland client JSON object into our Window type
fn parse_window(c: &serde_json::Value) -> Window {
    let at   = &c["at"];
    let size = &c["size"];
    Window {
        id:         c["address"].as_str().unwrap_or("").to_string(),
        pid:        c["pid"].as_u64().map(|p| p as u32),
        class:      c["class"].as_str().unwrap_or("").to_string(),
        title:      c["title"].as_str().unwrap_or("").to_string(),
        workspace:  c["workspace"]["id"].as_u64().unwrap_or(0) as u32,
        fullscreen: c["fullscreen"].as_bool().unwrap_or(false),
        floating:   c["floating"].as_bool().unwrap_or(false),
        xwayland:   c["xwayland"].as_bool().unwrap_or(false),
        geometry: Rect {
            x: at[0].as_i64().unwrap_or(0) as i32,
            y: at[1].as_i64().unwrap_or(0) as i32,
            w: size[0].as_u64().unwrap_or(0) as u32,
            h: size[1].as_u64().unwrap_or(0) as u32,
        },
    }
}
