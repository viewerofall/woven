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
use woven_common::types::{AnimationConfig, Workspace};
use compositor::backend::WmEvent;
use woven_render::{RenderCmd, RenderThread};

use compositor::detect_backend;
use lua::registry::AppState;
use sys::ipc_server::IpcServer;

/// Load persistent store from disk. Returns empty map on missing/corrupt file.
fn load_store() -> std::collections::HashMap<String, serde_json::Value> {
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    }
}

/// Save store to disk atomically (write tmp, then rename).
fn save_store(store: &std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>) {
    let path = store_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let guard = store.lock().unwrap();
    if let Ok(json) = serde_json::to_string_pretty(&*guard) {
        let tmp = format!("{}.tmp", path);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn store_path() -> String {
    let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{}/.local/share", home)
    });
    format!("{}/woven/store.json", data_home)
}

/// Read the Lua config and extract theme values without spinning a full Lua VM.
/// This gives the render thread correct colors from frame 1, not after Lua boots.
fn parse_initial_theme(config_path: &str) -> woven_common::types::Theme {
    let mut t = woven_common::types::Theme::default();
    let src = match std::fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(_) => return t,
    };

    // Scan line by line, tolerating any amount of alignment whitespace around `=`.
    fn lua_str(src: &str, key: &str) -> Option<String> {
        for line in src.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(key) { continue; }
            let after = &trimmed[key.len()..];
            if !after.starts_with([' ', '\t', '=']) { continue; }
            let after = after.trim_start_matches([' ', '\t']);
            let after = after.strip_prefix('=')?;
            let after = after.trim_start_matches([' ', '\t']);
            let after = after.strip_prefix('"')?;
            let end   = after.find('"')?;
            return Some(after[..end].to_string());
        }
        None
    }
    fn lua_num(src: &str, key: &str) -> Option<String> {
        for line in src.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(key) { continue; }
            let after = &trimmed[key.len()..];
            if !after.starts_with([' ', '\t', '=']) { continue; }
            let after = after.trim_start_matches([' ', '\t']);
            let after = after.strip_prefix('=')?;
            let after = after.trim_start_matches([' ', '\t']);
            let end   = after.find([',', '\n', '}', ' ', '\t'])
                             .unwrap_or(after.len());
            let v = after[..end].trim();
            if v.is_empty() { return None; }
            return Some(v.to_string());
        }
        None
    }

    if let Some(v) = lua_str(&src, "background")   { t.background    = v; }
    if let Some(v) = lua_str(&src, "border")        { t.border        = v; }
    if let Some(v) = lua_str(&src, "text")          { t.text          = v; }
    if let Some(v) = lua_str(&src, "accent")        { t.accent        = v; }
    if let Some(v) = lua_num(&src, "border_radius") {
        if let Ok(n) = v.parse::<u32>() { t.border_radius = n; }
    }
    if let Some(v) = lua_num(&src, "opacity") {
        if let Ok(n) = v.parse::<f32>() { t.opacity = n; }
    }
    t
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::from_env("WOVEN_LOG")
        .add_directive("woven=info".parse()?)
    )
    .init();

    info!("woven starting");

    // ── paths ─────────────────────────────────────────────────────────────────
    // Runtime (Lua stdlib): shipped with the binary, read-only.
    //   WOVEN_ROOT env override for development; otherwise next to the binary.
    // Config (user file):   ~/.config/woven/woven.lua
    //   XDG_CONFIG_HOME respected; WOVEN_CONFIG override for testing.
    let project_root = std::env::var("WOVEN_ROOT")
    .unwrap_or_else(|_| {
        std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_else(|| ".".into())
    });
    let runtime_dir = format!("{}/runtime", project_root);

    let config_path = std::env::var("WOVEN_CONFIG").unwrap_or_else(|_| {
        let xdg = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{}/.config", home)
        });
        format!("{}/woven/woven.lua", xdg)
    });

    // ── auto-detect compositor backend (plug-and-play) ────────────────────────
    let backend = detect_backend()
    .context("Compositor detection failed")?;

    let theme = parse_initial_theme(&config_path);
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
        widgets:       Arc::new(std::sync::Mutex::new(Vec::new())),
        event_queue:   Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        hooks:         Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        error_handler: Arc::new(std::sync::Mutex::new(None)),
        cava:          Arc::new(std::sync::Mutex::new(None)),
        store:         Arc::new(std::sync::Mutex::new(load_store())),
        namer:         Arc::new(std::sync::Mutex::new(lua::ws_namer::WorkspaceNamer::default())),
    });

    // ── periodic store flush (every 30s) ─────────────────────────────────────
    {
        let store_flush = state.store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                save_store(&store_flush);
            }
        });
    }

    // ── workspace state poller ────────────────────────────────────────────────
    // Tries to use the compositor's event stream (Hyprland: socket2) for instant
    // updates. Falls back to a 2s poll for backends that don't support events.
    {
        let backend_poll = state.backend.clone();
        let render_poll  = render.clone();

        let event_queue_poll = state.event_queue.clone();
        let namer_poll       = state.namer.clone();
        let store_poll       = state.store.clone();

        /// Diff two workspace snapshots and push WmEvents for any changes.
        fn diff_and_push(prev: &[Workspace], next: &[Workspace], queue: &std::sync::Mutex<std::collections::VecDeque<WmEvent>>) {
            // Workspace focus change
            let prev_active = prev.iter().find(|w| w.active).map(|w| w.id);
            let next_active = next.iter().find(|w| w.active).map(|w| w.id);
            if next_active != prev_active {
                if let Some(id) = next_active {
                    if let Ok(mut q) = queue.lock() { q.push_back(WmEvent::WorkspaceFocused { id }); }
                }
            }

            // Build flat window maps for diffing
            let prev_wins: std::collections::HashMap<&str, (u32, &woven_common::types::Window)> =
                prev.iter().flat_map(|ws| ws.windows.iter().map(move |w| (w.id.as_str(), (ws.id, w)))).collect();
            let next_wins: std::collections::HashMap<&str, (u32, &woven_common::types::Window)> =
                next.iter().flat_map(|ws| ws.windows.iter().map(move |w| (w.id.as_str(), (ws.id, w)))).collect();

            let Ok(mut q) = queue.lock() else { return };

            // Closed windows
            for id in prev_wins.keys() {
                if !next_wins.contains_key(id) {
                    q.push_back(WmEvent::WindowClosed { id: id.to_string() });
                }
            }
            // Opened windows
            for (id, (_, win)) in &next_wins {
                if !prev_wins.contains_key(id) {
                    q.push_back(WmEvent::WindowOpened { window: (*win).clone() });
                }
            }
            // Moved windows (same id, different workspace)
            for (id, (next_ws, _)) in &next_wins {
                if let Some((prev_ws, _)) = prev_wins.get(id) {
                    if prev_ws != next_ws {
                        q.push_back(WmEvent::WindowMoved { id: id.to_string(), workspace: *next_ws });
                    }
                }
            }
        }

        if let Some(mut events) = state.backend.event_stream() {
            let namer_ev = namer_poll.clone();
            let store_ev = store_poll.clone();
            tokio::spawn(async move {
                let poll_interval = tokio::time::Duration::from_millis(2000);
                let mut interval  = tokio::time::interval(poll_interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut prev_active_ws:  Option<u32>      = None;
                let mut prev_workspaces: Vec<Workspace>   = vec![];

                loop {
                    tokio::select! {
                        _ = events.recv() => {}
                        _ = interval.tick() => {}
                    }
                    if let Ok(mut workspaces) = backend_poll.workspaces().await {
                        let active = workspaces.iter().find(|w| w.active).map(|w| w.id);
                        if active != prev_active_ws {
                            if let Some(ws_id) = active {
                                render_poll.send(RenderCmd::CaptureForWorkspace(ws_id));
                            }
                            prev_active_ws = active;
                        }
                        diff_and_push(&prev_workspaces, &workspaces, &event_queue_poll);
                        prev_workspaces = workspaces.clone();
                        // Apply workspace auto-namer before sending to render
                        if let Ok(namer) = namer_ev.lock() {
                            namer.apply_names(&mut workspaces, &store_ev);
                        }
                        render_poll.send(RenderCmd::UpdateState { workspaces, metrics: vec![] });
                    }
                }
            });
        } else {
            tokio::spawn(async move {
                let mut prev_active_ws:  Option<u32>    = None;
                let mut prev_workspaces: Vec<Workspace> = vec![];
                loop {
                    if let Ok(mut workspaces) = backend_poll.workspaces().await {
                        let active = workspaces.iter().find(|w| w.active).map(|w| w.id);
                        if active != prev_active_ws {
                            if let Some(ws_id) = active {
                                render_poll.send(RenderCmd::CaptureForWorkspace(ws_id));
                            }
                            prev_active_ws = active;
                        }
                        diff_and_push(&prev_workspaces, &workspaces, &event_queue_poll);
                        prev_workspaces = workspaces.clone();
                        // Apply workspace auto-namer before sending to render
                        if let Ok(namer) = namer_poll.lock() {
                            namer.apply_names(&mut workspaces, &store_poll);
                        }
                        render_poll.send(RenderCmd::UpdateState { workspaces, metrics: vec![] });
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                }
            });
        }
    }


    // Listens on unix socket — woven-ctrl connects here.
    // Runs concurrently with the Lua runtime via tokio::spawn.
    {
        let render_ipc  = render.clone();
        let state_ipc   = state.clone();
        let socket_path = woven_common::ipc::socket_path();

        tokio::spawn(async move {
            let server = IpcServer { socket_path };
            let visible_flag = render_ipc.visible_flag.clone();
            let handler = Arc::new(move |cmd: IpcCommand| {
                let render = render_ipc.clone();
                let state  = state_ipc.clone();
                let vf     = visible_flag.clone();
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
                            // Full reload — restart the daemon process so the
                            // entire Lua config (plugins, widgets, namer, bar,
                            // theme, animations) is re-evaluated from scratch.
                            // Save store before restarting.
                            save_store(&state.store);
                            tracing::info!("reload requested — restarting daemon");
                            tokio::spawn(async {
                                // Small delay so the IPC response reaches the client.
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                let _ = std::process::Command::new("systemctl")
                                    .args(["--user", "restart", "woven"])
                                    .status();
                            });
                            IpcResponse::Ok
                        }
                        IpcCommand::GetStatus => {
                            let theme  = state.theme.read().await.clone();
                            let backend_name = state.backend.name().to_string();
                            let workspaces = state.backend.workspaces().await
                            .unwrap_or_default();
                            IpcResponse::Status(Box::new(DaemonStatus {
                                version:    env!("CARGO_PKG_VERSION").to_string(),
                                                visible:    vf.load(std::sync::atomic::Ordering::Relaxed),
                                                compositor: backend_name,
                                                workspaces,
                                                metrics:    vec![],
                                                theme,
                            }))
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
                    // CloseOverlay: spawn woven-ctrl --hide since direct
                    // RenderCmd::Hide through this channel is unreliable.
                    if matches!(action, WindowAction::CloseOverlay) {
                        let _ = std::process::Command::new("woven-ctrl")
                        .arg("--hide")
                        .spawn();
                        continue;
                    }
                    let cmd = match action {
                        WindowAction::Focus(id)            => WmCommand::FocusWindow(id),
                        WindowAction::Close(id)            => WmCommand::CloseWindow(id),
                        WindowAction::ToggleFloat(id)      => WmCommand::ToggleFloat(id),
                        WindowAction::TogglePin(id)        => WmCommand::TogglePin(id),
                        WindowAction::ToggleFullscreen(id) => WmCommand::FullscreenWindow(id),
                        WindowAction::FocusWorkspace(id)   => WmCommand::FocusWorkspace(id),
                        // All of the following are consumed in the render thread:
                        WindowAction::CloseOverlay         => unreachable!(),
                        WindowAction::PreviewWorkspace(_)  => unreachable!(),
                        WindowAction::ClosePanel           => unreachable!(),
                        WindowAction::ToggleOverlay        => unreachable!(),
                        WindowAction::HideBar              => unreachable!(),
                        WindowAction::ExpandPanel          => unreachable!(),
                        WindowAction::CollapsePanel        => unreachable!(),
                        WindowAction::PowerSuspend         => unreachable!(),
                        WindowAction::PowerReboot          => unreachable!(),
                        WindowAction::PowerShutdown        => unreachable!(),
                        WindowAction::PowerLock            => unreachable!(),
                        WindowAction::PowerLogout          => unreachable!(),
                        WindowAction::MediaPlayPause       => unreachable!(),
                        WindowAction::MediaNext            => unreachable!(),
                        WindowAction::MediaPrev            => unreachable!(),
                        WindowAction::WifiToggle           => unreachable!(),
                        WindowAction::BtToggle             => unreachable!(),
                    };
                    if let Err(e) = backend_actions.dispatch(cmd).await {
                        tracing::warn!("window action failed: {:#}", e);
                    }
                    // hide overlay after any window action
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

    // flush store before exit
    save_store(&state.store);

    Ok(())
}
