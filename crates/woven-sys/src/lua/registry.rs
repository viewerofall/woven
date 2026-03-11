//! Registers all Rust-backed functions into the `woven` Lua global.
//! This is the complete list of what Lua is allowed to call.
//! Nothing outside this table can reach Rust or the system.

use mlua::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use woven_common::types::{AnimationConfig, Theme};

use crate::compositor::backend::CompositorBackend;
use crate::sys::proc_metrics::MetricsCollector;

use woven_render::{RenderThread, RenderCmd};

/// Shared app state passed into every Lua API function
#[allow(dead_code)]
pub struct AppState {
    pub backend:     Arc<dyn CompositorBackend>,
    pub render:      Arc<RenderThread>,
    pub metrics:     Arc<RwLock<MetricsCollector>>,
    pub theme:       Arc<RwLock<Theme>>,
    pub anims:       Arc<RwLock<AnimationConfig>>,
    pub runtime_dir: String,
    pub config_path: String,
}

/// Serialize a serde-able value into a LuaValue via JSON round-trip.
/// mlua 0.10 removed lua.to_value() — this is the correct replacement.
fn to_lua<T: serde::Serialize>(lua: &Lua, val: &T) -> LuaResult<LuaValue> {
    let json = serde_json::to_value(val).map_err(LuaError::external)?;
    json_to_lua(lua, json)
}

fn json_to_lua(lua: &Lua, val: serde_json::Value) -> LuaResult<LuaValue> {
    match val {
        serde_json::Value::Null        => Ok(LuaValue::Nil),
        serde_json::Value::Bool(b)     => Ok(LuaValue::Boolean(b)),
        serde_json::Value::Number(n)   => {
            if let Some(i) = n.as_i64() { Ok(LuaValue::Integer(i)) }
            else { Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0))) }
        }
        serde_json::Value::String(s)   => Ok(LuaValue::String(lua.create_string(&s)?)),
        serde_json::Value::Array(arr)  => {
            let t = lua.create_table()?;
            for (i, v) in arr.into_iter().enumerate() {
                t.set(i + 1, json_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(t))
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.set(k, json_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(t))
        }
    }
}

/// Convert a LuaValue back to serde_json::Value for deserialization
fn lua_to_json(val: LuaValue) -> anyhow::Result<serde_json::Value> {
    match val {
        LuaValue::Nil            => Ok(serde_json::Value::Null),
        LuaValue::Boolean(b)     => Ok(serde_json::Value::Bool(b)),
        LuaValue::Integer(i)     => Ok(serde_json::Value::Number(i.into())),
        LuaValue::Number(n)      => Ok(serde_json::Value::Number(
            serde_json::Number::from_f64(n)
            .unwrap_or(serde_json::Number::from(0))
        )),
        LuaValue::String(s)      => Ok(serde_json::Value::String(
            s.to_str().map_err(|e| anyhow::anyhow!("{}", e))?.to_string()
        )),
        LuaValue::Table(t)       => {
            let len = t.raw_len();
            if len > 0 {
                let mut arr = Vec::new();
                for i in 1..=len {
                    let v: LuaValue = t.get(i)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                    arr.push(lua_to_json(v)?);
                }
                Ok(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<LuaValue, LuaValue>() {
                    let (k, v) = pair.map_err(|e| anyhow::anyhow!("{}", e))?;
                    let key = match k {
                        LuaValue::String(s) => s.to_str()
                        .map_err(|e| anyhow::anyhow!("{}", e))?.to_string(),
                        LuaValue::Integer(i) => i.to_string(),
                        _ => continue,
                    };
                    map.insert(key, lua_to_json(v)?);
                }
                Ok(serde_json::Value::Object(map))
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}

pub fn bind(lua: &Lua, state: Arc<AppState>) -> anyhow::Result<()> {
    let woven = lua.create_table()?;

    bind_fs(lua, &woven, state.clone())?;
    bind_compositor(lua, &woven, state.clone())?;
    bind_window(lua, &woven, state.clone())?;
    bind_metrics(lua, &woven, state.clone())?;
    bind_overlay(lua, &woven, state.clone())?;
    bind_workspaces_api(lua, &woven, state.clone())?;
    bind_guide(lua, &woven)?;
    bind_log(lua, &woven)?;
    bind_process(lua, &woven)?;
    bind_config_api(lua, &woven, state.clone())?;

    // safe require — only resolves from runtime/ and config dir
    let runtime_dir = state.runtime_dir.clone();
    let config_dir  = std::path::Path::new(&state.config_path)
    .parent()
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_default();

    let safe_require = lua.create_function(move |lua, module: String| {
        let candidates = [
            format!("{}/{}.lua", runtime_dir, module.replace('.', "/")),
                format!("{}/{}.lua", config_dir,  module.replace('.', "/")),
        ];
        for path in &candidates {
            if std::path::Path::new(path).exists() {
                let code = std::fs::read_to_string(path)
                .map_err(LuaError::external)?;
                return lua.load(&code).set_name(&module).eval::<LuaMultiValue>();
            }
        }
        Err(LuaError::external(format!("module '{}' not found", module)))
    })?;

    lua.globals().set("require", safe_require)?;
    lua.globals().set("__woven_config_path", state.config_path.clone())?;
    lua.globals().set("woven", woven)?;

    info!("Lua API registered");
    Ok(())
}

fn bind_fs(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let fs          = lua.create_table()?;
    let config_path = state.config_path.clone();

    let exists = lua.create_function(|_, path: String| {
        Ok(std::path::Path::new(&path).exists())
    })?;

    let cfg_dir_r = std::path::Path::new(&config_path)
    .parent().map(|p| p.to_path_buf())
    .unwrap_or_default();
    let read = lua.create_function(move |_, path: String| {
        let p = std::path::Path::new(&path);
        if !p.starts_with(&cfg_dir_r) {
            return Err(LuaError::external("fs.read: path outside config dir"));
        }
        std::fs::read_to_string(p).map_err(LuaError::external)
    })?;

    let cfg_dir_w = std::path::Path::new(&config_path)
    .parent().map(|p| p.to_path_buf())
    .unwrap_or_default();
    let write = lua.create_function(move |_, (path, content): (String, String)| {
        let p = std::path::Path::new(&path);
        if !p.starts_with(&cfg_dir_w) {
            return Err(LuaError::external("fs.write: path outside config dir"));
        }
        std::fs::write(p, content).map_err(LuaError::external)
    })?;

    let cp = config_path.clone();
    let config_path_fn = lua.create_function(move |_, ()| Ok(cp.clone()))?;

    fs.set("exists",      exists)?;
    fs.set("read",        read)?;
    fs.set("write",       write)?;
    fs.set("config_path", config_path_fn)?;
    woven.set("fs", fs)?;
    Ok(())
}

fn bind_compositor(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let comp = lua.create_table()?;

    let detect = lua.create_function(|_, ()| {
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return Ok("hyprland".to_string());
        }
        if std::env::var("SWAYSOCK").is_ok() {
            return Ok("sway".to_string());
        }
        if std::env::var("NIRI_SOCKET").is_ok() {
            return Ok("niri".to_string());
        }
        Ok("unknown".to_string())
    })?;

    let backend = state.backend.clone();
    let workspaces = lua.create_function(move |lua, ()| {
        let backend = backend.clone();
        let ws = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
            .block_on(backend.workspaces())
        }).map_err(LuaError::external)?;
        to_lua(lua, &ws)
    })?;

    let backend2 = state.backend.clone();
    let windows = lua.create_function(move |lua, ()| {
        let backend = backend2.clone();
        let wins = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
            .block_on(backend.windows())
        }).map_err(LuaError::external)?;
        to_lua(lua, &wins)
    })?;

    comp.set("detect",     detect)?;
    comp.set("workspaces", workspaces)?;
    comp.set("windows",    windows)?;
    woven.set("compositor", comp)?;
    Ok(())
}

fn bind_window(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    use crate::compositor::backend::WmCommand;
    let win = lua.create_table()?;

    macro_rules! dispatch_fn {
        ($cmd:expr) => {{
            let backend = state.backend.clone();
            lua.create_function(move |_, id: String| {
                let cmd = $cmd(id);
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                    .block_on(backend.dispatch(cmd))
                }).map_err(LuaError::external)
            })?
        }};
    }

    let focus      = dispatch_fn!(WmCommand::FocusWindow);
    let close      = dispatch_fn!(WmCommand::CloseWindow);
    let fullscreen = dispatch_fn!(WmCommand::FullscreenWindow);

    let backend_mv = state.backend.clone();
    let move_fn = lua.create_function(move |_, (id, ws): (String, u32)| {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
            .block_on(backend_mv.dispatch(WmCommand::MoveWindow { id, workspace: ws }))
        }).map_err(LuaError::external)
    })?;

    win.set("focus",      focus)?;
    win.set("close",      close)?;
    win.set("fullscreen", fullscreen)?;
    win.set("move",       move_fn)?;
    woven.set("window", win)?;
    Ok(())
}

fn bind_metrics(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let met           = lua.create_table()?;
    let metrics_state = state.clone();

    let ws_metrics = lua.create_function(move |lua, ws_id: u32| {
        let backend = metrics_state.backend.clone();
        let metrics = metrics_state.metrics.clone();

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let workspaces = backend.workspaces().await?;
                let ws         = workspaces.into_iter().find(|w| w.id == ws_id);
                if let Some(ws) = ws {
                    let mut m = metrics.write().await;
                    let agg   = m.aggregate(&[ws]);
                    Ok(agg.into_iter().next())
                } else {
                    Ok(None)
                }
            })
        }).map_err(|e: anyhow::Error| LuaError::external(e))?;

        match result {
            Some(m) => to_lua(lua, &m),
                                         None    => Ok(LuaValue::Nil),
        }
    })?;

    met.set("workspace", ws_metrics)?;
    woven.set("metrics", met)?;
    Ok(())
}

fn bind_overlay(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let ov = lua.create_table()?;

    let r = state.render.clone();
    let show = lua.create_function(move |_, ()| {
        r.send(RenderCmd::Show); Ok(())
    })?;

    let r = state.render.clone();
    let hide = lua.create_function(move |_, ()| {
        r.send(RenderCmd::Hide); Ok(())
    })?;

    let r = state.render.clone();
    let toggle = lua.create_function(move |_, ()| {
        r.send(RenderCmd::Toggle); Ok(())
    })?;

    // woven.overlay.update_state(workspaces, metrics)
    // called from Lua run loop to push fresh compositor state to the painter
    let r = state.render.clone();
    let update_state = lua.create_function(move |_, (ws, met): (LuaValue, LuaValue)| {
        // convert LuaValue tables back to JSON then deserialize
        let ws_json  = lua_to_json(ws).map_err(LuaError::external)?;
        let met_json = lua_to_json(met).map_err(LuaError::external)?;

        let workspaces = serde_json::from_value(ws_json)
        .map_err(LuaError::external)?;
        let metrics    = serde_json::from_value(met_json)
        .map_err(LuaError::external)?;

        r.send(RenderCmd::UpdateState { workspaces, metrics });
        Ok(())
    })?;

    ov.set("show",         show)?;
    ov.set("hide",         hide)?;
    ov.set("toggle",       toggle)?;
    ov.set("update_state", update_state)?;
    woven.set("overlay", ov)?;

    // woven.sleep(ms) — blocks the Lua run loop for N milliseconds
    // this is what keeps the process alive without burning CPU
    let sleep_fn = lua.create_function(|_, ms: u64| {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        Ok(())
    })?;
    woven.set("sleep", sleep_fn)?;

    Ok(())
}

fn bind_workspaces_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let render = state.render.clone();
    // woven.workspaces({ show_empty = bool }) — called from user config
    let set_workspaces = lua.create_function(move |_, t: LuaTable| {
        let show_empty = t.get::<bool>("show_empty").unwrap_or(false);
        render.send(RenderCmd::UpdateSettings { show_empty });
        Ok(())
    })?;
    woven.set("workspaces", set_workspaces)?;
    Ok(())
}

fn bind_guide(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    let guide    = lua.create_table()?;
    let print_fn = lua.create_function(|_, msg: String| {
        println!("[woven guide] {}", msg);
        Ok(())
    })?;
    let input = lua.create_function(|_, ()| {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)
        .map_err(LuaError::external)?;
        Ok(buf.trim().to_string())
    })?;

    guide.set("print", print_fn)?;
    guide.set("input", input)?;
    woven.set("guide", guide)?;
    Ok(())
}

fn bind_log(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    let log   = lua.create_table()?;
    let info  = lua.create_function(|_, m: String| { tracing::info!("[lua] {}",  m); Ok(()) })?;
    let warn  = lua.create_function(|_, m: String| { tracing::warn!("[lua] {}",  m); Ok(()) })?;
    let error = lua.create_function(|_, m: String| { tracing::error!("[lua] {}", m); Ok(()) })?;

    log.set("info",  info)?;
    log.set("warn",  warn)?;
    log.set("error", error)?;
    woven.set("log", log)?;
    Ok(())
}

fn bind_process(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    let proc  = lua.create_table()?;

    // woven.process.spawn("woven-ctrl", {"--setup"})
    let spawn = lua.create_function(|_, (prog, args): (String, Vec<String>)| {
        std::process::Command::new(&prog)
            .args(&args)
            .spawn()
            .map_err(|e| mlua::Error::external(
                format!("process.spawn({}): {}", prog, e)
            ))?;
        Ok(())
    })?;

    // woven.process.sleep(ms) — blocks the Lua VM thread, not the render thread
    let sleep = lua.create_function(|_, ms: u64| {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        Ok(())
    })?;

    proc.set("spawn", spawn)?;
    proc.set("sleep", sleep)?;
    woven.set("process", proc)?;
    Ok(())
}

fn bind_config_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let theme_state = state.theme.clone();
    let render      = state.render.clone();
    let set_theme   = lua.create_function(move |_, t: LuaTable| {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut theme = theme_state.write().await;
                if let Ok(v) = t.get::<String>("background")  { theme.background    = v; }
                if let Ok(v) = t.get::<String>("border")      { theme.border        = v; }
                if let Ok(v) = t.get::<String>("text")        { theme.text          = v; }
                if let Ok(v) = t.get::<String>("accent")      { theme.accent        = v; }
                if let Ok(v) = t.get::<u32>("border_radius")  { theme.border_radius = v; }
                if let Ok(v) = t.get::<String>("font")        { theme.font          = v; }
                if let Ok(v) = t.get::<u32>("font_size")      { theme.font_size     = v; }
                if let Ok(v) = t.get::<f32>("opacity")        { theme.opacity       = v; }
                if let Ok(v) = t.get::<bool>("blur")          { theme.blur          = v; }
                // push updated theme to render thread
                render.send(RenderCmd::UpdateTheme(theme.clone()));
            })
        });
        Ok(())
    })?;

    woven.set("theme", set_theme)?;
    Ok(())
}
