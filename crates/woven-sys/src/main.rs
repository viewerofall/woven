//! woven-sys: bootstrap, IPC server, hand control to Lua.

mod compositor;
mod lua;
mod sys;

use anyhow::{Context, Result};
use mlua::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use woven_common::ipc::{DaemonStatus, IpcCommand, IpcResponse};
use woven_common::types::{AnimationConfig, Theme};
use woven_render::{RenderCmd, RenderThread};

use compositor::detect_backend;
use lua::registry::AppState;
use sys::ipc_server::IpcServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::from_env("WOVEN_LOG")
        .add_directive("woven=info".parse()?)
    )
    .init();

    info!("woven starting");

    let project_root = std::env::var("WOVEN_ROOT")
    .unwrap_or_else(|_| ".".to_string());
    let config_path = format!("{}/config/woven.lua", project_root);
    let runtime_dir = format!("{}/runtime", project_root);

    // ── auto-detect compositor backend (plug-and-play) ────────────────────────
    let backend = detect_backend()
    .context("Compositor detection failed")?;

    let theme = Theme::default();
    let anims = AnimationConfig::default();

    // ── spawn render thread ───────────────────────────────────────────────────
    let render = Arc::new(
        RenderThread::spawn(theme.clone(), anims.clone())
        .context("Failed to spawn render thread")?
    );

    // ── shared app state ──────────────────────────────────────────────────────
    let state = Arc::new(AppState {
        backend,
        render:      render.clone(),
                         metrics:     Arc::new(RwLock::new(sys::proc_metrics::MetricsCollector::new())),
                         theme:       Arc::new(RwLock::new(theme.clone())),
                         anims:       Arc::new(RwLock::new(anims)),
                         runtime_dir: runtime_dir.clone(),
                         config_path: config_path.clone(),
    });

    // ── workspace state poller (pure Rust, bypasses Lua) ─────────────────────
    // Polls compositor every 2s and pushes state directly to render thread.
    // This replaces the Lua _push_state loop that was crashing on lua_to_json.
    {
        let backend_poll = state.backend.clone();
        let render_poll  = render.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(workspaces) = backend_poll.workspaces().await {
                    render_poll.send(RenderCmd::UpdateState {
                        workspaces,
                        metrics: vec![],
                    });
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
            }
        });
    }


    // Listens on unix socket — woven-ctrl connects here.
    // Runs concurrently with the Lua runtime via tokio::spawn.
    {
        let render_ipc  = render.clone();
        let state_ipc   = state.clone();
        let socket_path = woven_common::ipc::socket_path();

        tokio::spawn(async move {
            let server = IpcServer { socket_path };
            let handler = Arc::new(move |cmd: IpcCommand| {
                let render = render_ipc.clone();
                let state  = state_ipc.clone();
                async move {
                    match cmd {
                        IpcCommand::Show   => {
                            render.send(RenderCmd::Show);
                            IpcResponse::Ok
                        }
                        IpcCommand::Hide   => {
                            render.send(RenderCmd::Hide);
                            IpcResponse::Ok
                        }
                        IpcCommand::Toggle => {
                            render.send(RenderCmd::Toggle);
                            IpcResponse::Ok
                        }
                        IpcCommand::ReloadConfig => {
                            // re-read config file and push new theme
                            // for now just acknowledge — full reload needs Lua restart
                            IpcResponse::Ok
                        }
                        IpcCommand::GetStatus => {
                            let theme  = state.theme.read().await.clone();
                            let backend_name = state.backend.name().to_string();
                            let workspaces = state.backend.workspaces().await
                            .unwrap_or_default();
                            IpcResponse::Status(DaemonStatus {
                                version:    env!("CARGO_PKG_VERSION").to_string(),
                                                visible:    false, // TODO: track in shared state
                                                compositor: backend_name,
                                                workspaces,
                                                metrics:    vec![],
                                                theme,
                            })
                        }
                    }
                }
            });

            if let Err(e) = server.serve(handler).await {
                tracing::error!("IPC server error: {:#}", e);
            }
        });
    }

    // ── build Lua VM ──────────────────────────────────────────────────────────
    let lua = Lua::new();

    lua::sandbox::apply(&lua)
    .context("Failed to apply Lua sandbox")?;

    lua::registry::bind(&lua, state.clone())
    .context("Failed to bind Lua API")?;

    let boot_path = format!("{}/boot.lua", runtime_dir);
    let boot_code = std::fs::read_to_string(&boot_path)
    .with_context(|| format!("Could not read {}", boot_path))?;

    info!("handing control to Lua runtime");

    // ── window action pump ────────────────────────────────────────────────────
    // Poll the render thread's action channel and dispatch to compositor.
    // Runs alongside the Lua runtime.
    {
        use compositor::backend::WmCommand;
        use woven_render::WindowAction;

        let backend_actions = state.backend.clone();
        let action_rx       = render.action_rx.clone();
        let render_hide     = render.clone();

        tokio::spawn(async move {
            loop {
                while let Ok(action) = action_rx.try_recv() {
                    let cmd = match action {
                        WindowAction::Focus(id)            => WmCommand::FocusWindow(id),
                     WindowAction::Close(id)            => WmCommand::CloseWindow(id),
                     WindowAction::ToggleFloat(id)      => WmCommand::ToggleFloat(id),
                     WindowAction::TogglePin(id)        => WmCommand::TogglePin(id),
                     WindowAction::ToggleFullscreen(id) => WmCommand::FullscreenWindow(id),
                    };
                    if let Err(e) = backend_actions.dispatch(cmd).await {
                        tracing::warn!("window action failed: {:#}", e);
                    }
                    // hide overlay after any window action so toggle state stays in sync
                    render_hide.send(RenderCmd::Hide);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(16)).await;
            }
        });
    }

    lua.load(&boot_code)
    .set_name("boot.lua")
    .exec()
    .map_err(|e| anyhow::anyhow!("Lua runtime error: {}", e))?;

    Ok(())
}
