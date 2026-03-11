//! Niri IPC backend
//!
//! Niri exposes a JSON socket at $NIRI_SOCKET.
//! Protocol: newline-delimited JSON. Send a request object, get a response object.
//! Requests: {"Workspaces": null}, {"Windows": null}, {"Action": {...}}
//! Events: connect with {"EventStream": null} then read newline-delimited events.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::env;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, warn};
use woven_common::types::{Rect, Window, Workspace};

use super::backend::{CompositorBackend, WmCommand};

pub struct NiriBackend {
    socket_path: String,
}

impl NiriBackend {
    pub fn new() -> Result<Self> {
        let path = env::var("NIRI_SOCKET")
        .context("NIRI_SOCKET not set — is Niri running?")?;
        Ok(Self { socket_path: path })
    }

    pub fn detect() -> bool {
        env::var("NIRI_SOCKET").is_ok()
    }

    /// Send a request and read one response line.
    async fn ipc(&self, request: serde_json::Value) -> Result<serde_json::Value> {
        let mut stream = UnixStream::connect(&self.socket_path)
        .await
        .context("Failed to connect to Niri socket")?;
        let mut req = request.to_string();
        req.push('\n');
        stream.write_all(req.as_bytes()).await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        debug!("niri ipc response: {} bytes", line.len());
        Ok(serde_json::from_str(line.trim())?)
    }
}

#[async_trait]
impl CompositorBackend for NiriBackend {
    fn name(&self) -> &'static str { "niri" }

    fn event_stream(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<()>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let path = self.socket_path.clone();

        tokio::spawn(async move {
            loop {
                match UnixStream::connect(&path).await {
                    Err(e) => {
                        warn!("niri event socket connect failed: {e} — retrying in 2s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    Ok(mut stream) => {
                        let sub = "{\"EventStream\":null}\n";
                        if stream.write_all(sub.as_bytes()).await.is_err() { continue; }

                        let mut reader = BufReader::new(stream);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match reader.read_line(&mut line).await {
                                Ok(0) | Err(_) => break,
                     Ok(_) => {
                         // Fire on events that change workspace/window state
                         let relevant = line.contains("WorkspacesChanged")
                         || line.contains("WindowsChanged")
                         || line.contains("WindowOpenedOrChanged")
                         || line.contains("WindowClosed")
                         || line.contains("WorkspaceActiveWindowChanged")
                         || line.contains("WorkspaceFocused");
                         if relevant {
                             let _ = tx.send(());
                         }
                     }
                            }
                        }
                        warn!("niri event stream ended — reconnecting");
                    }
                }
            }
        });

        Some(rx)
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>> {
        // Fetch workspaces and windows in parallel
        let (ws_resp, win_resp) = tokio::try_join!(
            self.ipc(serde_json::json!({"Workspaces": null})),
                                                   self.ipc(serde_json::json!({"Windows": null})),
        )?;

        let ws_list = ws_resp["Ok"]["Workspaces"]
        .as_array()
        .context("Expected Workspaces array")?;

        let win_list = win_resp["Ok"]["Windows"]
        .as_array()
        .cloned()
        .unwrap_or_default();

        let workspaces = ws_list.iter().map(|ws| {
            let id     = ws["id"].as_u64().unwrap_or(0) as u32;
            let name   = ws["name"].as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.to_string());
            let active = ws["is_focused"].as_bool().unwrap_or(false);

            let windows = win_list.iter()
            .filter(|w| w["workspace_id"].as_u64().unwrap_or(0) as u32 == id)
            .map(|w| parse_window(w, id))
            .collect();

            Workspace { id, name, active, windows }
        }).collect();

        Ok(workspaces)
    }

    async fn windows(&self) -> Result<Vec<Window>> {
        let resp = self.ipc(serde_json::json!({"Windows": null})).await?;
        let win_list = resp["Ok"]["Windows"]
        .as_array()
        .context("Expected Windows array")?;
        Ok(win_list.iter().map(|w| parse_window(w, 0)).collect())
    }

    async fn dispatch(&self, cmd: WmCommand) -> Result<()> {
        let action = match cmd {
            WmCommand::FocusWindow(id) =>
            serde_json::json!({"Action": {"FocusWindow": {"id": id.parse::<u64>().unwrap_or(0)}}}),
            WmCommand::CloseWindow(id) =>
            serde_json::json!({"Action": {"CloseWindow": {"id": id.parse::<u64>().unwrap_or(0)}}}),
            WmCommand::FullscreenWindow(id) =>
            serde_json::json!({"Action": {"FullscreenWindow": {"id": id.parse::<u64>().unwrap_or(0)}}}),
            WmCommand::ToggleFloat(id) =>
            serde_json::json!({"Action": {"ToggleWindowFloating": {"id": id.parse::<u64>().unwrap_or(0)}}}),
            WmCommand::MoveWindow { id, workspace } |
            WmCommand::MoveToWorkspace { id, ws: workspace } =>
            serde_json::json!({"Action": {"MoveWindowToWorkspace": {
                "window_id": id.parse::<u64>().unwrap_or(0),
                              "reference": {"Id": workspace}
            }}}),
            // Niri doesn't have a pin concept, closest is sticky
            WmCommand::TogglePin(_) => return Ok(()),
        };

        let resp = self.ipc(action).await?;
        if resp["Ok"].is_null() && !resp["Err"].is_null() {
            warn!("niri action failed: {:?}", resp["Err"]);
        }
        Ok(())
    }
}

fn parse_window(w: &serde_json::Value, ws_id: u32) -> Window {
    let id = w["id"].as_u64().unwrap_or(0).to_string();
    let pid = w["pid"].as_u64().map(|p| p as u32);
    let class = w["app_id"].as_str().unwrap_or("").to_string();
    let title = w["title"].as_str().unwrap_or("").to_string();
    let ws = w["workspace_id"].as_u64().unwrap_or(ws_id as u64) as u32;

    // Niri geometry is in the "geometry" field
    let rect = &w["geometry"];
    Window {
        id, pid, class, title,
        workspace: ws,
        fullscreen: w["is_fullscreen"].as_bool().unwrap_or(false),
        floating:   w["is_floating"].as_bool().unwrap_or(false),
        xwayland:   false, // Niri is Wayland-only
        geometry: Rect {
            x: rect["x"].as_i64().unwrap_or(0) as i32,
            y: rect["y"].as_i64().unwrap_or(0) as i32,
            w: rect["width"].as_u64().unwrap_or(0) as u32,
            h: rect["height"].as_u64().unwrap_or(0) as u32,
        },
    }
}
