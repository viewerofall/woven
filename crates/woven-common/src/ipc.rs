//! IPC message types shared between woven daemon and woven-ctrl.

use serde::{Deserialize, Serialize};
use crate::types::{Theme, Workspace, WorkspaceMetrics};

pub const SOCKET_PATH_ENV: &str = "WOVEN_SOCKET";

pub fn default_socket_path() -> String {
    let user = std::env::var("USER").unwrap_or("user".into());
    format!("/tmp/woven-{}.sock", user)
}

pub fn socket_path() -> String {
    std::env::var(SOCKET_PATH_ENV).unwrap_or_else(|_| default_socket_path())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcCommand {
    Show,
    Hide,
    Toggle,
    ReloadConfig,
    GetStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok,
    Status(Box<DaemonStatus>),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version:    String,
    pub visible:    bool,
    pub compositor: String,
    pub workspaces: Vec<Workspace>,
    pub metrics:    Vec<WorkspaceMetrics>,
    pub theme:      Theme,
}
