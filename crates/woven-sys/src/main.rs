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
use woven_common::types::AnimationConfig;
use woven_render::{RenderCmd, RenderThread};

use compositor::detect_backend;
use lua::registry::AppState;
use sys::ipc_server::IpcServer;

/// Read the Lua config and extract theme values without spinning a full Lua VM.
/// This gives the render thread correct colors from frame 1, not after Lua boots.
fn parse_initial_theme(config_path: &str) -> woven_common::types::Theme {
    let mut t = woven_common::types::Theme::default();
    let src = match std::fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(_) => return t,
    };

    fn lua_str<'a>(src: &'a str, key: &str) -> Option<&'a str> {
        let needle = format!("{} = \"", key);
        let start  = src.find(&needle)? + needle.len();
        let end    = start + src[start..].find('"')?;
        Some(&src[start..end])
    }
    fn lua_num<'a>(src: &'a str, key: &str) -> Option<&'a str> {
        let needle = format!("{} = ", key);
        let start  = src.find(&needle)? + needle.len();
        let end    = start + src[start..]
        .find(|c: char| c == ',' || c == '\n' || c == '}')?;
        let v = src[start..end].trim();
        if v.is_empty() { None } else { Some(v) }
    }

    if let Some(v) = lua_str(&src, "background")    { t.background    = v.into(); }
    if let Some(v) = lua_str(&src, "border")         { t.border        = v.into(); }
    if let Some(v) = lua_str(&src, "text")           { t.text          = v.into(); }
    if let Some(v) = lua_str(&src, "accent")         { t.accent        = v.into(); }
    if let Some(v) = lua_num(&src, "border_radius")  {
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
    });

    // ── workspace state poller ────────────────────────────────────────────────
    // Tries to use the compositor's event stream (Hyprland: socket2) for instant
    // updates. Falls back to a 2s poll for backends that don't support events.
    {
        let backend_poll = state.backend.clone();
        let render_poll  = render.clone();

        if let Some(mut events) = state.backend.event_stream() {
            // event-driven path: fire immediately on relevant compositor events
            // + a 2s heartbeat so the display stays fresh even if we miss an event
            tokio::spawn(async move {
                let poll_interval = tokio::time::Duration::from_millis(2000);
                let mut interval  = tokio::time::interval(poll_interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = events.recv() => {}  // compositor event — refresh now
                        _ = interval.tick() => {} // heartbeat
                    }
                    if let Ok(workspaces) = backend_poll.workspaces().await {
                        render_poll.send(RenderCmd::UpdateState { workspaces, metrics: vec![] });
                    }
                }
            });
        } else {
            // fallback: plain 2s poll for backends without event streams
            tokio::spawn(async move {
                loop {
                    if let Ok(workspaces) = backend_poll.workspaces().await {
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
                            // Spin a throwaway Lua VM, wire only woven.theme(),
                            // execute the user config, grab whatever theme it set,
                            // then push UpdateTheme to the render thread.
                            let cfg_path  = state.config_path.clone();
                            let render_rc = state.render.clone();
                            let theme_rw  = state.theme.clone();

                            tokio::task::spawn_blocking(move || {
                                use mlua::prelude::*;
                                let Ok(code) = std::fs::read_to_string(&cfg_path) else { return; };
                                let lua = Lua::new();

                                // captured theme output
                                let captured: Arc<std::sync::Mutex<Option<woven_common::types::Theme>>>
                                = Arc::new(std::sync::Mutex::new(None));
                                let cap2 = captured.clone();

                                // wire a minimal woven.theme() that just captures the table
                                let woven = lua.create_table().unwrap();
                                let noop  = lua.create_function(|_, _: mlua::Value| Ok(())).unwrap();
                                woven.set("settings",   noop.clone()).unwrap();
                                woven.set("workspaces", noop.clone()).unwrap();
                                woven.set("animations", noop.clone()).unwrap();
                                woven.set("on",         noop.clone()).unwrap();

                                let theme_fn = lua.create_function(move |_, t: LuaTable| {
                                    let mut th = woven_common::types::Theme::default();
                                    if let Ok(v) = t.get::<String>("background")  { th.background    = v; }
                                    if let Ok(v) = t.get::<String>("border")      { th.border        = v; }
                                    if let Ok(v) = t.get::<String>("text")        { th.text          = v; }
                                    if let Ok(v) = t.get::<String>("accent")      { th.accent        = v; }
                                    if let Ok(v) = t.get::<u32>("border_radius")  { th.border_radius = v; }
                                    if let Ok(v) = t.get::<String>("font")        { th.font          = v; }
                                    if let Ok(v) = t.get::<u32>("font_size")      { th.font_size     = v; }
                                    if let Ok(v) = t.get::<f32>("opacity")        { th.opacity       = v; }
                                    if let Ok(v) = t.get::<bool>("blur")          { th.blur          = v; }
                                    *cap2.lock().unwrap() = Some(th);
                                    Ok(())
                                }).unwrap();
                                woven.set("theme", theme_fn).unwrap();

                                // stub woven.log so config can call it safely
                                let log_tbl = lua.create_table().unwrap();
                                let log_fn  = lua.create_function(|_, _: mlua::Value| Ok(())).unwrap();
                                log_tbl.set("info",  log_fn.clone()).unwrap();
                                log_tbl.set("warn",  log_fn.clone()).unwrap();
                                log_tbl.set("error", log_fn.clone()).unwrap();
                                woven.set("log", log_tbl).unwrap();

                                lua.globals().set("woven", woven).unwrap();

                                // run the config — ignore errors from missing globals etc.
                                let _ = lua.load(&code).exec();

                                let maybe_theme = captured.lock().unwrap().take();
                                if let Some(new_theme) = maybe_theme {
                                    tokio::task::block_in_place(|| {
                                        tokio::runtime::Handle::current().block_on(async {
                                            *theme_rw.write().await = new_theme.clone();
                                        });
                                    });
                                    render_rc.send(RenderCmd::UpdateTheme(new_theme));
                                    tracing::info!("config reloaded: theme updated");
                                }
                            });

                            IpcResponse::Ok
                        }
                        IpcCommand::GetStatus => {
                            let theme  = state.theme.read().await.clone();
                            let backend_name = state.backend.name().to_string();
                            let workspaces = state.backend.workspaces().await
                            .unwrap_or_default();
                            IpcResponse::Status(DaemonStatus {
                                version:    env!("CARGO_PKG_VERSION").to_string(),
                                                visible:    vf.load(std::sync::atomic::Ordering::Relaxed),
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
                     WindowAction::CloseOverlay         => unreachable!(),
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

    Ok(())
}
