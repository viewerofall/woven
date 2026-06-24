//! Sway IPC backend for woven-bar.
//! Lifted from woven/crates/woven-sys/src/compositor/sway.rs and trimmed
//! to only what the bar needs: workspace list + active window title.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const MAGIC: &[u8]         = b"i3-ipc";
const MSG_SUBSCRIBE: u32   = 2;
const MSG_GET_WORKSPACES: u32 = 1;
const MSG_GET_TREE: u32    = 4;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BarState {
    pub workspaces:    Vec<WsInfo>,
    pub active_title:  String,
    pub active_class:  String,
}

#[derive(Debug, Clone)]
pub struct WsInfo {
    pub name:   String,
    pub num:    i32,
    pub active: bool,
    pub urgent: bool,
}

// ── Client ────────────────────────────────────────────────────────────────────

pub struct SwayClient {
    socket_path: String,
}

impl SwayClient {
    pub fn new() -> Result<Self> {
        let path = std::env::var("SWAYSOCK")
            .context("SWAYSOCK not set — is Sway running?")?;
        Ok(Self { socket_path: path })
    }

    pub fn detect() -> bool {
        std::env::var("SWAYSOCK").is_ok()
    }

    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .context("sway: connect failed")
    }

    async fn ipc(&self, msg_type: u32, payload: &str) -> Result<serde_json::Value> {
        let mut stream = self.connect().await?;
        let body = payload.as_bytes();
        let mut msg = Vec::with_capacity(14 + body.len());
        msg.extend_from_slice(MAGIC);
        msg.extend_from_slice(&(body.len() as u32).to_le_bytes());
        msg.extend_from_slice(&msg_type.to_le_bytes());
        msg.extend_from_slice(body);
        stream.write_all(&msg).await?;

        let mut hdr = [0u8; 14];
        stream.read_exact(&mut hdr).await?;
        if &hdr[..6] != MAGIC { bail!("invalid sway IPC magic"); }
        let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        Ok(serde_json::from_slice(&buf)?)
    }

    /// Fetch workspace list.
    pub async fn workspaces(&self) -> Result<Vec<WsInfo>> {
        let v = self.ipc(MSG_GET_WORKSPACES, "").await?;
        let mut ws = Vec::new();
        for w in v.as_array().unwrap_or(&vec![]) {
            ws.push(WsInfo {
                name:   w["name"].as_str().unwrap_or("?").to_string(),
                num:    w["num"].as_i64().unwrap_or(0) as i32,
                active: w["focused"].as_bool().unwrap_or(false),
                urgent: w["urgent"].as_bool().unwrap_or(false),
            });
        }
        ws.sort_by_key(|w| w.num);
        Ok(ws)
    }

    /// Fetch the focused window title and app_id/class.
    pub async fn focused_window(&self) -> Result<(String, String)> {
        let tree = self.ipc(MSG_GET_TREE, "").await?;
        Ok(find_focused(&tree).unwrap_or_default())
    }

    /// Subscribe to workspace + window events. Returns a channel that fires
    /// whenever the compositor state changes.
    pub fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<()> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let path = self.socket_path.clone();

        tokio::spawn(async move {
            loop {
                match UnixStream::connect(&path).await {
                    Err(e) => {
                        tracing::warn!("sway event socket: {e} — retry in 2s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    Ok(mut stream) => {
                        let sub = r#"["window","workspace"]"#;
                        let body = sub.as_bytes();
                        let mut msg = Vec::with_capacity(14 + body.len());
                        msg.extend_from_slice(MAGIC);
                        msg.extend_from_slice(&(body.len() as u32).to_le_bytes());
                        msg.extend_from_slice(&MSG_SUBSCRIBE.to_le_bytes());
                        msg.extend_from_slice(body);
                        if stream.write_all(&msg).await.is_err() { continue; }

                        // drain ack
                        let mut hdr = [0u8; 14];
                        if stream.read_exact(&mut hdr).await.is_err() { continue; }
                        let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
                        let mut ack = vec![0u8; len];
                        let _ = stream.read_exact(&mut ack).await;

                        loop {
                            let mut hdr = [0u8; 14];
                            if stream.read_exact(&mut hdr).await.is_err() { break; }
                            let len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;
                            let mut buf = vec![0u8; len];
                            if stream.read_exact(&mut buf).await.is_err() { break; }
                            let _ = tx.send(());
                        }
                        tracing::warn!("sway event stream ended — reconnecting");
                    }
                }
            }
        });

        rx
    }
}

// ── Tree walking ──────────────────────────────────────────────────────────────

fn find_focused(node: &serde_json::Value) -> Option<(String, String)> {
    if node["focused"].as_bool().unwrap_or(false) {
        let title = node["name"].as_str().unwrap_or("").to_string();
        let class = node["app_id"].as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                node["window_properties"]["class"].as_str().unwrap_or("").to_string()
            });
        if !title.is_empty() {
            return Some((title, class));
        }
    }

    for child in node["nodes"].as_array().unwrap_or(&vec![]).iter()
        .chain(node["floating_nodes"].as_array().unwrap_or(&vec![]).iter())
    {
        if let Some(r) = find_focused(child) { return Some(r); }
    }

    None
}
