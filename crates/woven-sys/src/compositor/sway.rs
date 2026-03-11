//! Sway IPC backend
//!
//! Sway uses i3-compatible IPC over a Unix socket at $SWAYSOCK.
//! Protocol: 4-byte magic "i3-ipc", 4-byte payload length (LE), 4-byte type (LE), payload.
//! Type 1 = run_command, Type 4 = get_workspaces, Type 3 = get_outputs,
//! Type 1 with "subscribe" for events.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::{debug, warn};
use woven_common::types::{Rect, Window, Workspace};

use super::backend::{CompositorBackend, WmCommand};

const MAGIC: &[u8] = b"i3-ipc";
const MSG_RUN_COMMAND:   u32 = 0;
const MSG_GET_WORKSPACES: u32 = 1;
const MSG_SUBSCRIBE:     u32 = 2;
const MSG_GET_TREE:      u32 = 4;

pub struct SwayBackend {
    socket_path: String,
}

impl SwayBackend {
    pub fn new() -> Result<Self> {
        let path = env::var("SWAYSOCK")
        .context("SWAYSOCK not set — is Sway running?")?;
        Ok(Self { socket_path: path })
    }

    pub fn detect() -> bool {
        env::var("SWAYSOCK").is_ok()
        && env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() // don't steal from Hyprland
    }

    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
        .await
        .context("Failed to connect to Sway IPC socket")
    }

    /// Send a message and receive a response.
    async fn ipc(&self, msg_type: u32, payload: &str) -> Result<serde_json::Value> {
        let mut stream = self.connect().await?;
        let body = payload.as_bytes();
        let mut msg = Vec::with_capacity(14 + body.len());
        msg.extend_from_slice(MAGIC);
        msg.extend_from_slice(&(body.len() as u32).to_le_bytes());
        msg.extend_from_slice(&msg_type.to_le_bytes());
        msg.extend_from_slice(body);
        stream.write_all(&msg).await?;

        // Read response header
        let mut hdr = [0u8; 14];
        stream.read_exact(&mut hdr).await?;
        if &hdr[..6] != MAGIC {
            bail!("Invalid Sway IPC magic");
        }
        let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        debug!("sway ipc type={} len={}", msg_type, len);
        Ok(serde_json::from_slice(&buf)?)
    }

    /// Get the full window tree and flatten to workspace+window lists.
    async fn fetch_tree(&self) -> Result<serde_json::Value> {
        self.ipc(MSG_GET_TREE, "").await
    }

    /// Get workspaces list (lighter than full tree, used for active state).
    async fn fetch_workspaces_raw(&self) -> Result<serde_json::Value> {
        self.ipc(MSG_GET_WORKSPACES, "").await
    }
}

#[async_trait]
impl CompositorBackend for SwayBackend {
    fn name(&self) -> &'static str { "sway" }

    fn event_stream(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<()>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let path = self.socket_path.clone();

        tokio::spawn(async move {
            loop {
                match UnixStream::connect(&path).await {
                    Err(e) => {
                        warn!("sway event socket connect failed: {e} — retrying in 2s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    Ok(mut stream) => {
                        // Subscribe to window + workspace events
                        let sub = r#"["window","workspace"]"#;
                        let body = sub.as_bytes();
                        let mut msg = Vec::with_capacity(14 + body.len());
                        msg.extend_from_slice(MAGIC);
                        msg.extend_from_slice(&(body.len() as u32).to_le_bytes());
                        msg.extend_from_slice(&MSG_SUBSCRIBE.to_le_bytes());
                        msg.extend_from_slice(body);
                        if stream.write_all(&msg).await.is_err() { continue; }

                        // Drain subscription ack
                        let mut hdr = [0u8; 14];
                        if stream.read_exact(&mut hdr).await.is_err() { continue; }
                        let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
                        let mut ack = vec![0u8; len];
                        let _ = stream.read_exact(&mut ack).await;

                        // Read event stream
                        loop {
                            let mut hdr = [0u8; 14];
                            if stream.read_exact(&mut hdr).await.is_err() { break; }
                            let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
                            let mut buf = vec![0u8; len];
                            if stream.read_exact(&mut buf).await.is_err() { break; }
                            let _ = tx.send(());
                        }
                        warn!("sway event stream ended — reconnecting");
                    }
                }
            }
        });

        Some(rx)
    }

    async fn workspaces(&self) -> Result<Vec<Workspace>> {
        let tree = self.fetch_tree().await?;
        let ws_list = self.fetch_workspaces_raw().await?;

        // Build active workspace set from workspaces endpoint
        let focused_ws: std::collections::HashSet<String> = ws_list
        .as_array().unwrap_or(&vec![])
        .iter()
        .filter(|w| w["focused"].as_bool().unwrap_or(false))
        .filter_map(|w| w["name"].as_str().map(|s| s.to_string()))
        .collect();

        // Walk the tree: root > outputs > workspaces > containers > windows
        let mut workspaces = Vec::new();
        let outputs = tree["nodes"].as_array().unwrap_or(&vec![]).to_vec();
        for output in &outputs {
            let ws_nodes = output["nodes"].as_array().unwrap_or(&vec![]).to_vec();
            for ws_node in &ws_nodes {
                let ws_type = ws_node["type"].as_str().unwrap_or("");
                if ws_type != "workspace" { continue; }
                // Skip __i3_scratch
                let ws_name = ws_node["name"].as_str().unwrap_or("");
                if ws_name == "__i3_scratch" { continue; }

                let ws_id = ws_node["id"].as_u64().unwrap_or(0) as u32;
                let active = focused_ws.contains(ws_name);

                let mut windows = Vec::new();
                collect_windows(ws_node, ws_id, &mut windows);

                workspaces.push(Workspace {
                    id: ws_id,
                    name: ws_name.to_string(),
                                active,
                                windows,
                });
            }
        }

        // Sort by workspace id for consistent ordering
        workspaces.sort_by_key(|w| w.id);
        Ok(workspaces)
    }

    async fn windows(&self) -> Result<Vec<Window>> {
        let tree = self.fetch_tree().await?;
        let mut windows = Vec::new();
        collect_windows(&tree, 0, &mut windows);
        Ok(windows)
    }

    async fn dispatch(&self, cmd: WmCommand) -> Result<()> {
        let sway_cmd = match cmd {
            WmCommand::FocusWindow(id)       => format!("[con_id={}] focus", id),
            WmCommand::CloseWindow(id)        => format!("[con_id={}] kill", id),
            WmCommand::FullscreenWindow(id)   => format!("[con_id={}] fullscreen toggle", id),
            WmCommand::ToggleFloat(id)        => format!("[con_id={}] floating toggle", id),
            WmCommand::TogglePin(id)          => format!("[con_id={}] sticky toggle", id),
            WmCommand::MoveWindow { id, workspace } |
            WmCommand::MoveToWorkspace { id, ws: workspace } =>
            format!("[con_id={}] move to workspace number {}", id, workspace),
        };

        let resp = self.ipc(MSG_RUN_COMMAND, &sway_cmd).await?;
        if let Some(arr) = resp.as_array() {
            for r in arr {
                if !r["success"].as_bool().unwrap_or(false) {
                    warn!("sway command failed: {:?}", r["error"]);
                }
            }
        }
        Ok(())
    }
}

/// Recursively collect all leaf windows from a Sway tree node.
fn collect_windows(node: &serde_json::Value, ws_id: u32, out: &mut Vec<Window>) {
    // A node is a window if it has a non-empty "app_id" or "window_properties"
    let is_window = node["app_id"].as_str().map(|s| !s.is_empty()).unwrap_or(false)
    || node["window_properties"].is_object();

    if is_window {
        let rect = &node["rect"];
        let app_id = node["app_id"].as_str().unwrap_or("");
        let class = if app_id.is_empty() {
            node["window_properties"]["class"].as_str().unwrap_or("").to_string()
        } else {
            app_id.to_string()
        };
        let title = node["name"].as_str().unwrap_or("").to_string();
        let pid = node["pid"].as_u64().map(|p| p as u32);
        let id = node["id"].as_u64().unwrap_or(0).to_string();

        out.push(Window {
            id,
            pid,
            class,
            title,
            workspace: ws_id,
            fullscreen: node["fullscreen_mode"].as_u64().unwrap_or(0) > 0,
                 floating: node["type"].as_str() == Some("floating_con"),
                 xwayland: node["window_properties"].is_object(), // X11 windows have window_properties
                 geometry: Rect {
                     x: rect["x"].as_i64().unwrap_or(0) as i32,
                 y: rect["y"].as_i64().unwrap_or(0) as i32,
                 w: rect["width"].as_u64().unwrap_or(0) as u32,
                 h: rect["height"].as_u64().unwrap_or(0) as u32,
                 },
        });
        return;
    }

    // Recurse into child nodes and floating nodes
    for child in node["nodes"].as_array().unwrap_or(&vec![]).iter()
        .chain(node["floating_nodes"].as_array().unwrap_or(&vec![]).iter())
        {
            collect_windows(child, ws_id, out);
        }
}
