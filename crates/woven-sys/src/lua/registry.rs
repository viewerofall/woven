//! Registers all Rust-backed functions into the `woven` Lua global.
//! This is the complete list of what Lua is allowed to call.
//! Nothing outside this table can reach Rust or the system.

use mlua::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tracing::info;
use woven_common::types::{AnimationConfig, BarConfig, DrawCmd, LayoutConfig, Theme, WidgetDef, WidgetSlot};
use crate::compositor::backend::WmEvent;

use crate::compositor::backend::CompositorBackend;
use crate::sys::proc_metrics::MetricsCollector;

use woven_render::{RenderThread, RenderCmd};

/// A registered bar widget + its Lua render function + last render time.
pub struct WidgetEntry {
    pub def:          WidgetDef,
    pub render_key:   LuaRegistryKey,           // registered Lua function
    pub cmds:         Arc<Mutex<Vec<DrawCmd>>>,  // shared with ctx closures
    pub last_render:  std::time::Instant,
    /// Last error string — tracked to avoid spamming notifications on every tick.
    pub last_error:   Option<String>,
}

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
    pub widgets:     Arc<Mutex<Vec<WidgetEntry>>>,
    /// Pending compositor events to fire on next woven.sleep() tick.
    pub event_queue: Arc<Mutex<VecDeque<WmEvent>>>,
    /// Registered Lua event hooks: event-name → list of callback keys.
    pub hooks:       Arc<Mutex<HashMap<String, Vec<LuaRegistryKey>>>>,
    /// Optional user-registered error callback (from `woven.on_error(fn)`).
    pub error_handler: Arc<Mutex<Option<LuaRegistryKey>>>,
    /// Cava audio reader — started on first call to `woven.audio.start()`.
    pub cava: Arc<Mutex<Option<crate::sys::audio::CavaReader>>>,
    /// Persistent key-value store — survives hot-reloads and restarts.
    pub store: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    /// Workspace auto-namer — replaces numeric IDs with smart names in the overlay.
    pub namer: Arc<Mutex<super::ws_namer::WorkspaceNamer>>,
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
    bind_bar_api(lua, &woven, state.clone())?;
    bind_layout_api(lua, &woven, state.clone())?;
    bind_plugin_api(lua, &woven, state.clone())?;
    bind_events_api(lua, &woven, state.clone())?;
    bind_rules_api(lua, &woven, state.clone())?;
    bind_error_api(lua, &woven, state.clone())?;
    bind_sys_api(lua, &woven)?;
    bind_io_api(lua, &woven)?;
    bind_audio_api(lua, &woven, state.clone())?;
    bind_store_api(lua, &woven, state.clone())?;
    bind_http_api(lua, &woven)?;
    bind_namer_api(lua, &woven, state.clone())?;

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
        let p = std::path::Path::new(&path)
            .canonicalize()
            .map_err(|e| LuaError::external(format!("fs.read: {}", e)))?;
        if !p.starts_with(&cfg_dir_r) {
            return Err(LuaError::external("fs.read: path outside config dir"));
        }
        std::fs::read_to_string(p).map_err(LuaError::external)
    })?;

    let cfg_dir_w = std::path::Path::new(&config_path)
    .parent().map(|p| p.to_path_buf())
    .unwrap_or_default();
    let write = lua.create_function(move |_, (path, content): (String, String)| {
        // For writes, canonicalize the parent dir (file may not exist yet).
        let p = std::path::Path::new(&path);
        let parent = p.parent()
            .ok_or_else(|| LuaError::external("fs.write: no parent directory"))?
            .canonicalize()
            .map_err(|e| LuaError::external(format!("fs.write: {}", e)))?;
        if !parent.starts_with(&cfg_dir_w) {
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

    // woven.sleep(ms) — blocks the Lua run loop for N milliseconds.
    // Also ticks any registered bar widgets that are due for a re-render.
    let widgets_sleep = state.widgets.clone();
    let events_sleep  = state.event_queue.clone();
    let hooks_sleep   = state.hooks.clone();
    let render_sleep  = state.render.clone();
    let sleep_fn = lua.create_function(move |lua, ms: u64| {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        tick_widgets(lua, &widgets_sleep, &render_sleep);
        fire_events(lua, &events_sleep, &hooks_sleep);
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

    // woven.process.exec("playerctl", {"metadata", "--format", "..."}) → stdout string
    // Runs synchronously (blocks Lua VM); stdout is returned as a string.
    // stderr is discarded. Returns "" on error or non-zero exit.
    let exec = lua.create_function(|_, (prog, args): (String, Vec<String>)| {
        let out = std::process::Command::new(&prog)
        .args(&args)
        .output();
        match out {
            Ok(o) if o.status.success() => {
                Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
            }
            _ => Ok(String::new()),
        }
    })?;

    proc.set("spawn", spawn)?;
    proc.set("sleep", sleep)?;
    proc.set("exec",  exec)?;
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

fn bind_bar_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let render = state.render.clone();
    // woven.bar({ enabled = true, position = "right" })
    let set_bar = lua.create_function(move |_, t: LuaTable| {
        let enabled  = t.get::<bool>("enabled").unwrap_or(true);
        let position = t.get::<String>("position").unwrap_or_else(|_| "right".into());
        let json = serde_json::json!({ "enabled": enabled, "position": position });
        let cfg: BarConfig = serde_json::from_value(json).map_err(LuaError::external)?;
        render.send(RenderCmd::UpdateBarConfig(cfg));
        Ok(())
    })?;
    woven.set("bar", set_bar)?;
    Ok(())
}

fn bind_layout_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let render = state.render.clone();
    // woven.layout({ card_gap = 12, ws_thumb_width = 200, ... })
    // Any field omitted keeps the current default.
    let set_layout = lua.create_function(move |_, t: LuaTable| {
        let mut l = LayoutConfig::default();
        if let Ok(v) = t.get::<f32>("top_bar_height")    { l.top_bar_height    = v; }
        if let Ok(v) = t.get::<f32>("ws_strip_height")   { l.ws_strip_height   = v; }
        if let Ok(v) = t.get::<f32>("widget_bar_height") { l.widget_bar_height = v; }
        if let Ok(v) = t.get::<f32>("outer_padding")     { l.outer_padding     = v; }
        if let Ok(v) = t.get::<f32>("strip_gap")         { l.strip_gap         = v; }
        if let Ok(v) = t.get::<f32>("ws_thumb_width")    { l.ws_thumb_width    = v; }
        if let Ok(v) = t.get::<f32>("ws_thumb_height")   { l.ws_thumb_height   = v; }
        if let Ok(v) = t.get::<f32>("ws_btn_height")     { l.ws_btn_height     = v; }
        if let Ok(v) = t.get::<f32>("card_padding")      { l.card_padding      = v; }
        if let Ok(v) = t.get::<f32>("card_gap")          { l.card_gap          = v; }
        if let Ok(v) = t.get::<f32>("card_thumb_ratio")  { l.card_thumb_ratio  = v; }
        render.send(RenderCmd::UpdateLayout(l));
        Ok(())
    })?;
    woven.set("layout", set_layout)?;
    Ok(())
}

fn bind_plugin_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let plugin = lua.create_table()?;

    let render     = state.render.clone();
    let widgets    = state.widgets.clone();
    let config_dir = std::path::Path::new(&state.config_path)
    .parent()
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_default();

    let register = lua.create_function(move |lua, t: LuaTable| {
        let name: String = t.get("name").unwrap_or_else(|_| "unnamed".into());
        let kind: String = t.get("type").unwrap_or_else(|_| "unknown".into());
        tracing::info!("[plugin] registered: {} ({})", name, kind);

        let handle = lua.create_table()?;
        handle.set("name", name.clone())?;
        handle.set("type", kind.clone())?;

        // ── icon pack ────────────────────────────────────────────────────
        if kind == "icon_pack" {
            let render_ic   = render.clone();
            let cfg_dir_ic  = config_dir.clone();
            let render_def  = render.clone();
            let cfg_dir_def = config_dir.clone();

            let icon_fn = lua.create_function(move |_, (class, path): (String, String)| {
                let abs = resolve_plugin_path(&cfg_dir_ic, &path);
                let mut map = std::collections::HashMap::new();
                map.insert(class.to_lowercase(), abs);
                render_ic.send(RenderCmd::UpdateIconOverrides { map, default_icon: None });
                Ok(())
            })?;
            let icon_default_fn = lua.create_function(move |_, path: String| {
                let abs = resolve_plugin_path(&cfg_dir_def, &path);
                render_def.send(RenderCmd::UpdateIconOverrides {
                    map: std::collections::HashMap::new(),
                                default_icon: Some(abs),
                });
                Ok(())
            })?;
            handle.set("icon",         icon_fn)?;
            handle.set("icon_default", icon_default_fn)?;
        }

        // ── bar widget ───────────────────────────────────────────────────
        if kind == "bar_widget" {
            let render_w  = render.clone();
            let widgets_w = widgets.clone();
            let plugin_id = name.clone();

            // handle.widget({ slot="bottom", height=40, interval=5, render=fn })
            let widget_fn = lua.create_function(move |lua, cfg: LuaTable| {
                let slot_str: String = cfg.get("slot").unwrap_or_else(|_| "bottom".into());
                let slot = match slot_str.as_str() {
                    "top"     => WidgetSlot::Top,
                    "panel"   => WidgetSlot::Panel,
                    "overlay" => WidgetSlot::Overlay,
                    _         => WidgetSlot::Bottom,
                };
                let height:    u32 = cfg.get("height").unwrap_or(40);
                let interval:  u32 = cfg.get("interval").unwrap_or(5);
                let onclick: Option<String> = cfg.get("onclick").ok();
                let render_fn: LuaFunction = cfg.get("render")
                .map_err(|_| LuaError::external("bar_widget: render function required"))?;

                // Canvas dimensions per slot:
                //   Top/Bottom → BAR_THICK(52) - 2*pad(6) = 40
                //   Panel      → PANEL_THICK(300) - 2*pad(14) = 272
                //   Overlay    → max slot_w(260) - gap(8) = 252
                let canvas_w: f32 = match slot {
                    WidgetSlot::Top | WidgetSlot::Bottom => 40.0,
                    WidgetSlot::Panel   => 272.0,
                    WidgetSlot::Overlay => 252.0,
                };

                let def = WidgetDef {
                    id:           plugin_id.clone(),
                    slot,
                    height,
                    interval_secs: interval,
                    onclick_cmd:   onclick,
                };

                // Shared draw-command buffer — written by ctx closures, read by tick_widgets.
                let cmds: Arc<Mutex<Vec<DrawCmd>>> = Arc::new(Mutex::new(Vec::new()));
                let ctx = build_ctx(lua, cmds.clone(), height as f32, canvas_w)?;

                let render_key = lua.create_registry_value(render_fn)?;

                // Register with the render thread.
                render_w.send(RenderCmd::RegisterWidget(def.clone()));

                // Store entry so tick_widgets can call it.
                if let Ok(mut list) = widgets_w.lock() {
                    list.retain(|e| e.def.id != plugin_id); // replace if re-registered
                    list.push(WidgetEntry {
                        def,
                        render_key,
                        cmds,
                        last_render: std::time::Instant::now()
                        .checked_sub(std::time::Duration::from_secs(999))
                        .unwrap_or(std::time::Instant::now()),
                        last_error: None,
                    });
                }

                // Store ctx on the handle so Lua can inspect it if needed.
                lua.globals().set(format!("__woven_ctx_{}", plugin_id), ctx)?;
                Ok(())
            })?;
            handle.set("widget", widget_fn)?;
        }

        Ok(handle)
    })?;

    plugin.set("register", register)?;
    woven.set("plugin", plugin)?;
    Ok(())
}

/// Build a Lua ctx table whose methods append to `cmds`.
/// `canvas_h` / `canvas_w` are the logical pixel dimensions of the widget canvas.
fn build_ctx(lua: &Lua, cmds: Arc<Mutex<Vec<DrawCmd>>>, canvas_h: f32, canvas_w: f32) -> LuaResult<LuaTable> {
    let ctx = lua.create_table()?;

    let c = cmds.clone();
    ctx.set("text", lua.create_function(move |_, (text, opts): (String, LuaTable)| {
        let x     = opts.get::<f32>("x").unwrap_or(0.0);
        let y     = opts.get::<f32>("y").unwrap_or(0.0);
        let size  = opts.get::<f32>("size").unwrap_or(12.0);
        let color = opts.get::<String>("color").unwrap_or_else(|_| "#cdd6f4".into());
        let alpha = opts.get::<f32>("alpha").unwrap_or(1.0);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::Text { content: text, x, y, size, color, alpha }); }
        Ok(())
    })?)?;

    let c = cmds.clone();
    ctx.set("rect", lua.create_function(move |_, (x, y, w, h, opts): (f32, f32, f32, f32, Option<LuaTable>)| {
        let color  = opts.as_ref().and_then(|t| t.get::<String>("color").ok()).unwrap_or_else(|| "#313244".into());
        let alpha  = opts.as_ref().and_then(|t| t.get::<f32>("alpha").ok()).unwrap_or(1.0);
        let radius = opts.as_ref().and_then(|t| t.get::<f32>("radius").ok()).unwrap_or(4.0);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::FillRect { x, y, w, h, color, alpha, radius }); }
        Ok(())
    })?)?;

    let c = cmds.clone();
    ctx.set("circle", lua.create_function(move |_, (cx, cy, r, opts): (f32, f32, f32, Option<LuaTable>)| {
        let color = opts.as_ref().and_then(|t| t.get::<String>("color").ok()).unwrap_or_else(|| "#89b4fa".into());
        let alpha = opts.as_ref().and_then(|t| t.get::<f32>("alpha").ok()).unwrap_or(1.0);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::Circle { cx, cy, r, color, alpha }); }
        Ok(())
    })?)?;

    let c = cmds.clone();
    ctx.set("clear", lua.create_function(move |_, opts: Option<LuaTable>| {
        let color = opts.as_ref().and_then(|t| t.get::<String>("color").ok())
        .unwrap_or_else(|| "#1e1e2e".into());
        let alpha = opts.as_ref().and_then(|t| t.get::<f32>("alpha").ok()).unwrap_or(0.85);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::Clear { color, alpha }); }
        Ok(())
    })?)?;

    let c = cmds.clone();
    ctx.set("text_centered", lua.create_function(move |_, (text, opts): (String, LuaTable)| {
        let y     = opts.get::<f32>("y").unwrap_or(0.0);
        let size  = opts.get::<f32>("size").unwrap_or(12.0);
        let color = opts.get::<String>("color").unwrap_or_else(|_| "#cdd6f4".into());
        let alpha = opts.get::<f32>("alpha").unwrap_or(1.0);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::TextCentered { content: text, y, size, color, alpha }); }
        Ok(())
    })?)?;

    let c = cmds.clone();
    ctx.set("app_icon", lua.create_function(move |_, (class, opts): (String, LuaTable)| {
        let x    = opts.get::<f32>("x").unwrap_or(-1.0); // -1 = auto-center
        let y    = opts.get::<f32>("y").unwrap_or(5.0);
        let size = opts.get::<f32>("size").unwrap_or(40.0);
        if let Ok(mut g) = c.lock() { g.push(DrawCmd::AppIcon { class, x, y, size }); }
        Ok(())
    })?)?;

    ctx.set("height", canvas_h)?;
    ctx.set("w",      canvas_w)?;
    ctx.set("h",      canvas_h)?; // alias so both ctx.h and ctx.height work
    Ok(ctx)
}

/// Called by `woven.sleep()` — re-renders any widget whose interval has elapsed.
fn tick_widgets(lua: &Lua, widgets: &Arc<Mutex<Vec<WidgetEntry>>>, render: &Arc<RenderThread>) {
    // Collect due widgets under lock, then drop lock before calling Lua.
    // This prevents deadlocks if a render function calls back into the widget API.
    struct DueWidget {
        idx:       usize,
        id:        String,
        cmds:      Arc<Mutex<Vec<DrawCmd>>>,
    }

    let due_list: Vec<DueWidget> = {
        let Ok(list) = widgets.lock() else { return };
        list.iter().enumerate().filter_map(|(i, entry)| {
            let due = entry.def.interval_secs == 0
                || entry.last_render.elapsed().as_secs() >= entry.def.interval_secs as u64;
            if !due { return None; }
            Some(DueWidget {
                idx: i,
                id:  entry.def.id.clone(),
                cmds: entry.cmds.clone(),
            })
        }).collect()
    };
    // Lock is dropped here.

    for dw in &due_list {
        // Retrieve the render function from the Lua registry.
        let render_key = {
            let Ok(list) = widgets.lock() else { continue };
            let Some(entry) = list.get(dw.idx) else { continue };
            if entry.def.id != dw.id { continue; } // guard against index shift
            lua.registry_value::<LuaFunction>(&entry.render_key).ok()
        };
        let Some(func) = render_key else { continue };

        let ctx_key = format!("__woven_ctx_{}", dw.id);
        let Ok(ctx) = lua.globals().get::<LuaTable>(ctx_key) else { continue };

        // Clear previous draw commands.
        if let Ok(mut g) = dw.cmds.lock() { g.clear(); }

        // Call render(ctx) — lock is NOT held, so callbacks are safe.
        if let Err(e) = func.call::<()>(ctx) {
            let msg = e.to_string();
            tracing::warn!("[plugin {}] render error: {}", dw.id, msg);
            // Update last_error under lock, only notify on new/changed errors.
            let should_notify = {
                let Ok(mut list) = widgets.lock() else { continue };
                if let Some(entry) = list.get_mut(dw.idx).filter(|e| e.def.id == dw.id) {
                    let changed = entry.last_error.as_deref() != Some(&msg);
                    entry.last_error = Some(msg.clone());
                    changed
                } else { false }
            };
            if should_notify {
                let title = format!("woven plugin: {}", dw.id);
                let _ = std::process::Command::new("notify-send")
                    .args(["-u", "normal", "-i", "dialog-warning", &title, &msg])
                    .spawn();
                render.send(RenderCmd::ShowToast {
                    message:     format!("[{}] {}", dw.id, msg),
                    duration_ms: 6000,
                });
            }
        } else {
            // Clear error on success.
            if let Ok(mut list) = widgets.lock() {
                if let Some(entry) = list.get_mut(dw.idx).filter(|e| e.def.id == dw.id) {
                    entry.last_error = None;
                }
            }
        }

        // Collect commands and send to render thread.
        if let Ok(g) = dw.cmds.lock() {
            render.send(RenderCmd::UpdateWidgetContent {
                id:   dw.id.clone(),
                cmds: g.clone(),
            });
        }

        // Update last_render timestamp.
        if let Ok(mut list) = widgets.lock() {
            if let Some(entry) = list.get_mut(dw.idx).filter(|e| e.def.id == dw.id) {
                entry.last_render = std::time::Instant::now();
            }
        }
    }
}

/// `woven.on("workspace_focus", function(data) ... end)`
/// Registers a Lua callback for a compositor event.
///
/// Supported event names:
///   workspace_focus  — data: { id }
///   window_open      — data: { id, class, title, workspace, pid }
///   window_close     — data: { id }
///   window_focus     — data: { id }
///   window_move      — data: { id, workspace }
///   window_fullscreen — data: { id, fullscreen }
fn bind_events_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let hooks = state.hooks.clone();

    let on_fn = lua.create_function(move |lua, (event, cb): (String, LuaFunction)| {
        let key = lua.create_registry_value(cb)?;
        if let Ok(mut map) = hooks.lock() {
            map.entry(event).or_default().push(key);
        }
        Ok(())
    })?;

    woven.set("on", on_fn)?;
    Ok(())
}

/// `woven.io.read(path)` — read a file at any path (not restricted to config dir).
/// `woven.io.read_bytes(path, n)` — non-blocking read of up to n raw bytes.
///   Returns a string of bytes (may be shorter than n), or nil if no data available.
/// `woven.audio.start(n_bars)` — spawn cava and start reading.
/// `woven.audio.bars()` — returns latest bar levels as a Lua array of floats (0.0–1.0).
/// No-ops if cava is not installed. `start` is idempotent.
fn bind_audio_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let audio = lua.create_table()?;

    let cava_start = state.cava.clone();
    let start_fn = lua.create_function(move |_, n_bars: Option<usize>| {
        let n = n_bars.unwrap_or(16);
        let Ok(mut guard) = cava_start.lock() else { return Ok(()); };
        if guard.is_none() {
            *guard = crate::sys::audio::CavaReader::start(n);
        }
        Ok(())
    })?;
    audio.set("start", start_fn)?;

    let cava_bars = state.cava.clone();
    let bars_fn = lua.create_function(move |lua, ()| {
        let vals = match cava_bars.lock() {
            Ok(guard) => guard.as_ref().map(|r| r.bars()).unwrap_or_default(),
            Err(_)    => vec![],
        };
        let t = lua.create_table()?;
        for (i, v) in vals.iter().enumerate() {
            t.set(i + 1, *v)?;
        }
        Ok(t)
    })?;
    audio.set("bars", bars_fn)?;

    woven.set("audio", audio)?;
    Ok(())
}

/// `woven.store` — persistent key-value store that survives reloads and restarts.
/// Values are stored as JSON internally, supporting strings, numbers, bools, and tables.
fn bind_store_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let store_tbl = lua.create_table()?;

    let st = state.store.clone();
    let get_fn = lua.create_function(move |lua, key: String| {
        let guard = st.lock().map_err(|_| LuaError::external("store: lock poisoned"))?;
        match guard.get(&key) {
            Some(v) => json_to_lua(lua, v.clone()),
            None => Ok(LuaValue::Nil),
        }
    })?;
    store_tbl.set("get", get_fn)?;

    let st = state.store.clone();
    let set_fn = lua.create_function(move |_, (key, val): (String, LuaValue)| {
        let json_val = lua_to_json(val).map_err(LuaError::external)?;
        st.lock().map_err(|_| LuaError::external("store: lock poisoned"))?.insert(key, json_val);
        Ok(())
    })?;
    store_tbl.set("set", set_fn)?;

    let st = state.store.clone();
    let del_fn = lua.create_function(move |_, key: String| {
        st.lock().map_err(|_| LuaError::external("store: lock poisoned"))?.remove(&key);
        Ok(())
    })?;
    store_tbl.set("delete", del_fn)?;

    let st = state.store.clone();
    let keys_fn = lua.create_function(move |lua, ()| {
        let guard = st.lock().map_err(|_| LuaError::external("store: lock poisoned"))?;
        let t = lua.create_table()?;
        for (i, key) in guard.keys().enumerate() {
            t.set(i + 1, key.as_str())?;
        }
        Ok(t)
    })?;
    store_tbl.set("keys", keys_fn)?;

    woven.set("store", store_tbl)?;
    Ok(())
}

/// `woven.http.get(url, opts?)` — synchronous HTTP GET.
/// Returns `{ status, body, ok }`. Blocks the Lua thread (use sparingly).
fn bind_http_api(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    let http = lua.create_table()?;

    let get_fn = lua.create_function(|lua, (url, opts): (String, Option<LuaTable>)| {
        let timeout_secs = opts.as_ref()
            .and_then(|o| o.get::<u64>("timeout").ok())
            .unwrap_or(10);

        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(ref o) = opts {
            if let Ok(h) = o.get::<LuaTable>("headers") {
                for (k, v) in h.pairs::<String, String>().flatten() {
                    headers.push((k, v));
                }
            }
        }

        let resp = crate::sys::http::get(&url, timeout_secs, &headers);
        let t = lua.create_table()?;
        t.set("status", resp.status)?;
        t.set("body", resp.body)?;
        t.set("ok", resp.ok)?;
        Ok(t)
    })?;
    http.set("get", get_fn)?;

    woven.set("http", http)?;
    Ok(())
}

/// `woven.namer({...})` — configure workspace auto-naming.
/// `woven.namer.set(ws_id, name)` — pin a manual name.
/// `woven.namer.unset(ws_id)` — remove manual pin.
fn bind_namer_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    use super::ws_namer::NamingRule;

    let namer_tbl = lua.create_table()?;

    // woven.namer.set(ws_id, name) — pin a workspace name
    let store_pin = state.store.clone();
    let set_fn = lua.create_function(move |_, (ws_id, name): (u32, String)| {
        let key = format!("ws_namer.pin.{}", ws_id);
        store_pin.lock().map_err(|_| LuaError::external("store: lock poisoned"))?
            .insert(key, serde_json::Value::String(name));
        Ok(())
    })?;
    namer_tbl.set("set", set_fn)?;

    // woven.namer.unset(ws_id) — remove pin
    let store_unpin = state.store.clone();
    let unset_fn = lua.create_function(move |_, ws_id: u32| {
        let key = format!("ws_namer.pin.{}", ws_id);
        store_unpin.lock().map_err(|_| LuaError::external("store: lock poisoned"))?.remove(&key);
        Ok(())
    })?;
    namer_tbl.set("unset", unset_fn)?;

    woven.set("namer", namer_tbl)?;

    // woven.namer({...}) — configure the namer (callable as function)
    let namer_state = state.namer.clone();
    let namer_fn = lua.create_function(move |_, opts: LuaTable| {
        let Ok(mut namer) = namer_state.lock() else { return Ok(()); };
        if let Ok(enabled) = opts.get::<bool>("enabled") {
            namer.enabled = enabled;
        }
        if let Ok(rules_tbl) = opts.get::<LuaTable>("rules") {
            namer.rules.clear();
            for rule in rules_tbl.sequence_values::<LuaTable>().flatten() {
                let classes: Vec<String> = rule.get::<LuaTable>("classes")
                    .map(|t| t.sequence_values::<String>()
                        .filter_map(|v| v.ok())
                        .map(|s| s.to_lowercase())
                        .collect())
                    .unwrap_or_default();
                let name: String = rule.get("name").unwrap_or_default();
                if !classes.is_empty() && !name.is_empty() {
                    namer.rules.push(NamingRule { classes, name });
                }
            }
        }
        Ok(())
    })?;

    // Set the namer table as callable via __call metamethod
    let namer_ref: LuaTable = woven.get("namer")?;
    let meta = lua.create_table()?;
    meta.set("__call", lua.create_function(move |_, (_self, opts): (LuaValue, LuaTable)| {
        namer_fn.call::<()>(opts)
    })?)?;
    namer_ref.set_metatable(Some(meta));

    Ok(())
}

///   Safe for named pipes (fifos) — won't block if no writer.
fn bind_io_api(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    let io = lua.create_table()?;

    // woven.io.read(path) — blocking read of entire file
    let read_fn = lua.create_function(|_, path: String| {
        std::fs::read_to_string(&path).map_err(LuaError::external)
    })?;
    io.set("read", read_fn)?;

    // woven.io.read_bytes(path, n) — O_NONBLOCK read of n bytes
    // Returns a string (possibly shorter than n) or nil if nothing available.
    let read_bytes_fn = lua.create_function(|lua, (path, n): (String, usize)| {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Read;
        // O_NONBLOCK = 0o4000 on Linux
        let result = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&path);
        match result {
            Ok(mut f) => {
                let mut buf = vec![0u8; n];
                match f.read(&mut buf) {
                    Ok(read) if read > 0 => {
                        buf.truncate(read);
                        Ok(LuaValue::String(lua.create_string(&buf)?))
                    }
                    _ => Ok(LuaValue::Nil),
                }
            }
            Err(_) => Ok(LuaValue::Nil),
        }
    })?;
    io.set("read_bytes", read_bytes_fn)?;

    woven.set("io", io)?;
    Ok(())
}

/// `woven.now()` — current local time (chrono-backed, sandbox-safe).
/// Returns `{ hour, min, sec, day, month, year, wday, day_abbr, month_abbr }`.
///
/// `woven.sys_info()` — live system metrics from /proc.
/// Returns `{ cpu_pct, mem_pct, mem_used_gb, mem_total_gb }`.
fn bind_sys_api(lua: &Lua, woven: &LuaTable) -> LuaResult<()> {
    // woven.now()
    let now_fn = lua.create_function(|lua, ()| {
        let now = chrono::Local::now();
        let t   = lua.create_table()?;
        t.set("hour",       now.format("%H").to_string().parse::<u32>().unwrap_or(0))?;
        t.set("min",        now.format("%M").to_string().parse::<u32>().unwrap_or(0))?;
        t.set("sec",        now.format("%S").to_string().parse::<u32>().unwrap_or(0))?;
        t.set("day",        now.format("%d").to_string().parse::<u32>().unwrap_or(0))?;
        t.set("month",      now.format("%m").to_string().parse::<u32>().unwrap_or(0))?;
        t.set("year",       now.format("%Y").to_string().parse::<u32>().unwrap_or(0))?;
        // 1=Mon … 7=Sun (ISO)
        t.set("wday",       now.format("%u").to_string().parse::<u32>().unwrap_or(1))?;
        t.set("day_abbr",   now.format("%a").to_string())?;  // "Mon"
        t.set("month_abbr", now.format("%b").to_string())?;  // "Jan"
        t.set("unix_ts",    now.timestamp())?;               // seconds since epoch
        Ok(t)
    })?;
    woven.set("now", now_fn)?;

    // woven.sys_info()
    let sys_fn = lua.create_function(|lua, ()| {
        let cpu  = read_proc_cpu();
        let (used_kb, total_kb) = read_proc_mem();
        let mem_pct  = if total_kb > 0 { used_kb as f32 / total_kb as f32 * 100.0 } else { 0.0 };
        let used_gb  = used_kb  as f32 / (1024.0 * 1024.0);
        let total_gb = total_kb as f32 / (1024.0 * 1024.0);
        let t = lua.create_table()?;
        t.set("cpu_pct",      cpu)?;
        t.set("mem_pct",      mem_pct)?;
        t.set("mem_used_gb",  used_gb)?;
        t.set("mem_total_gb", total_gb)?;
        Ok(t)
    })?;
    woven.set("sys_info", sys_fn)?;

    Ok(())
}

/// Read instantaneous CPU usage from /proc/stat (two-sample delta).
/// Returns 0.0 if unavailable.
fn read_proc_cpu() -> f32 {
    fn parse_stat() -> Option<(u64, u64)> {
        let s = std::fs::read_to_string("/proc/stat").ok()?;
        let line = s.lines().next()?;
        let mut it = line.split_whitespace().skip(1);
        let user:    u64 = it.next()?.parse().ok()?;
        let nice:    u64 = it.next()?.parse().ok()?;
        let system:  u64 = it.next()?.parse().ok()?;
        let idle:    u64 = it.next()?.parse().ok()?;
        let iowait:  u64 = it.next()?.parse().ok()?;
        let irq:     u64 = it.next()?.parse().ok()?;
        let softirq: u64 = it.next()?.parse().ok()?;
        let total = user + nice + system + idle + iowait + irq + softirq;
        Some((idle + iowait, total))
    }
    let Some((idle1, total1)) = parse_stat() else { return 0.0 };
    std::thread::sleep(std::time::Duration::from_millis(80));
    let Some((idle2, total2)) = parse_stat() else { return 0.0 };
    let dt = (total2 - total1) as f32;
    let di = (idle2  - idle1)  as f32;
    if dt <= 0.0 { return 0.0; }
    ((dt - di) / dt * 100.0).clamp(0.0, 100.0)
}

/// Read memory usage from /proc/meminfo.
/// Returns (used_kb, total_kb).
fn read_proc_mem() -> (u64, u64) {
    let Ok(s) = std::fs::read_to_string("/proc/meminfo") else { return (0, 0) };
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in s.lines() {
        if line.starts_with("MemTotal:")     { total = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0); }
        if line.starts_with("MemAvailable:") { avail = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0); }
    }
    (total.saturating_sub(avail), total)
}

/// `woven.on_error(function(msg) ... end)`
/// Register a callback invoked when config loading encounters a Lua error.
/// The callback receives the error message as a string.
///
/// Also exposes `woven.__fire_error(msg)` used internally by boot.lua.
fn bind_error_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let handler = state.error_handler.clone();

    // woven.on_error(fn) — registers user callback
    let on_error = lua.create_function(move |lua, cb: LuaFunction| {
        let key = lua.create_registry_value(cb)?;
        if let Ok(mut h) = handler.lock() { *h = Some(key); }
        Ok(())
    })?;
    woven.set("on_error", on_error)?;

    // woven.__fire_error(msg) — called by boot.lua pcall catch block
    let handler2 = state.error_handler.clone();
    let render   = state.render.clone();
    let fire = lua.create_function(move |lua, msg: String| {
        tracing::error!("[lua config] {}", msg);
        // system notification — always fires regardless of user handler
        let _ = std::process::Command::new("notify-send")
            .args(["-u", "critical", "-i", "dialog-error", "woven: config error", &msg])
            .spawn();
        // overlay toast — always shown
        render.send(RenderCmd::ShowToast {
            message:     format!("Config error: {}", msg),
            duration_ms: 10000,
        });
        // user handler — called in addition to the above, not instead of
        if let Ok(h) = handler2.lock() {
            if let Some(ref key) = *h {
                if let Ok(func) = lua.registry_value::<LuaFunction>(key) {
                    let _ = func.call::<()>(msg.clone());
                }
            }
        }
        Ok(())
    })?;
    woven.set("__fire_error", fire)?;

    Ok(())
}

/// `woven.rules({ ["kitty"] = "#89b4fa", ["firefox"] = "#fab387" })`
/// Maps app class names to hex accent colors. Keys are lowercased.
fn bind_rules_api(lua: &Lua, woven: &LuaTable, state: Arc<AppState>) -> LuaResult<()> {
    let render = state.render.clone();

    let rules_fn = lua.create_function(move |_, tbl: LuaTable| {
        let mut map = std::collections::HashMap::new();
        for pair in tbl.pairs::<String, String>() {
            match pair {
                Ok((class, color)) => { map.insert(class, color); }
                Err(e) => tracing::warn!("woven.rules: bad entry: {}", e),
            }
        }
        render.send(RenderCmd::UpdateAppRules(map));
        Ok(())
    })?;

    woven.set("rules", rules_fn)?;
    Ok(())
}

/// Drains pending compositor events and fires registered Lua hooks.
fn fire_events(lua: &Lua, queue: &Arc<Mutex<VecDeque<WmEvent>>>, hooks: &Arc<Mutex<HashMap<String, Vec<LuaRegistryKey>>>>) {
    // Drain the queue without holding the lock during Lua calls.
    let events: Vec<WmEvent> = {
        let Ok(mut q) = queue.lock() else { return };
        q.drain(..).collect()
    };
    if events.is_empty() { return; }

    for event in &events {
        let (name, data) = match build_event_data(lua, event) {
            Some(v) => v,
            None    => continue,
        };
        // Resolve callback functions under lock, then drop lock before calling them.
        // This prevents deadlocks if a hook calls woven.on() to register more hooks.
        let funcs: Vec<LuaFunction> = {
            let Ok(map) = hooks.lock() else { continue };
            let Some(callbacks) = map.get(name) else { continue };
            callbacks.iter()
                .filter_map(|key| lua.registry_value::<LuaFunction>(key).ok())
                .collect()
        };
        for func in funcs {
            if let Err(e) = func.call::<()>(data.clone()) {
                tracing::warn!("[hook {}] error: {}", name, e);
            }
        }
    }
}

/// Map a `WmEvent` to (event-name-str, Lua table with event data).
fn build_event_data(lua: &Lua, event: &WmEvent) -> Option<(&'static str, LuaTable)> {
    let t = lua.create_table().ok()?;
    match event {
        WmEvent::WorkspaceFocused { id } => {
            t.set("id", *id).ok()?;
            Some(("workspace_focus", t))
        }
        WmEvent::WindowOpened { window } => {
            t.set("id",        window.id.clone()).ok()?;
            t.set("class",     window.class.clone()).ok()?;
            t.set("title",     window.title.clone()).ok()?;
            t.set("workspace", window.workspace).ok()?;
            if let Some(pid) = window.pid { t.set("pid", pid).ok()?; }
            Some(("window_open", t))
        }
        WmEvent::WindowClosed { id } => {
            t.set("id", id.clone()).ok()?;
            Some(("window_close", t))
        }
        WmEvent::WindowFocused { id } => {
            t.set("id", id.clone()).ok()?;
            Some(("window_focus", t))
        }
        WmEvent::WindowMoved { id, workspace } => {
            t.set("id",        id.clone()).ok()?;
            t.set("workspace", *workspace).ok()?;
            Some(("window_move", t))
        }
        WmEvent::WindowFullscreen { id, state } => {
            t.set("id",         id.clone()).ok()?;
            t.set("fullscreen", *state).ok()?;
            Some(("window_fullscreen", t))
        }
    }
}

/// Resolve a plugin-relative path to absolute.
/// "./icons/kitty.png" → "<config_dir>/icons/kitty.png"
/// Already-absolute paths are returned unchanged.
fn resolve_plugin_path(config_dir: &str, path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    let stripped = path.trim_start_matches("./").trim_start_matches('/');
    format!("{}/{}", config_dir, stripped)
}
