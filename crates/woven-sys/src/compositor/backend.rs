#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use woven_common::types::{Window, Workspace};

/// Every compositor backend implements this trait.
/// Lua never touches this — it calls woven.compositor.* which dispatches here.
#[async_trait]
pub trait CompositorBackend: Send + Sync {
    /// Human readable name e.g. "hyprland"
    fn name(&self) -> &'static str;

    /// Fetch current workspace list with windows
    async fn workspaces(&self) -> Result<Vec<Workspace>>;

    /// Fetch flat window list
    async fn windows(&self) -> Result<Vec<Window>>;

    /// Send a command to the compositor
    async fn dispatch(&self, cmd: WmCommand) -> Result<()>;

    /// Detect if this backend is available on the current system
    fn detect() -> bool where Self: Sized;
}

/// All commands Lua can trigger via woven.window.*
#[derive(Debug, Clone)]
pub enum WmCommand {
    FocusWindow(String),
    CloseWindow(String),
    FullscreenWindow(String),
    ToggleFloat(String),
    TogglePin(String),
    MoveWindow { id: String, workspace: u32 },
    MoveToWorkspace { id: String, ws: u32 },
}

/// Events the compositor emits that Lua hooks can react to
#[derive(Debug, Clone)]
pub enum WmEvent {
    WorkspaceFocused   { id: u32 },
    WindowOpened       { window: Window },
    WindowClosed       { id: String },
    WindowFocused      { id: String },
    WindowMoved        { id: String, workspace: u32 },
    WindowFullscreen   { id: String, state: bool },
}
