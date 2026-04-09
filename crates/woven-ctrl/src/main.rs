//! woven-ctrl — control panel + CLI for the woven daemon.
//!
//! CLI (no GUI, exits immediately):
//!   woven-ctrl --show | --hide | --toggle | --reload | --setup

mod setup;
mod helpers;
mod compositor_config;
use helpers::*;
use compositor_config::{CompositorStatus, detect_all, inject_keybind, inject_autostart, reload_compositor};
use woven_common::ipc::{IpcCommand, IpcResponse};

use iced::{
    widget::{
        button, checkbox, column, container, pick_list, row, rule,
        scrollable, text, text_editor, text_input, Space,
    },
    Alignment, Color, Element, Font, Length, Task, Theme,
};

// ── Tabs ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
enum Tab { #[default] Status, Bar, Theme, Overview, Plugins, Config }

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum ColorField { Background, Accent, Text, Border }

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Msg {
    TabSelect(Tab),

    // Status tab
    DaemonPoll(String, String, bool),
    DaemonShow, DaemonHide, DaemonToggle, DaemonReload,
    InjectKeybind(String),
    InjectAutostart(String),

    // Bar tab
    BarEnabledToggle(bool),
    BarPositionPicked(String),
    BarApply,

    // Theme tab
    PresetPicked(String),
    ColorChanged { field: ColorField, value: String },
    OpacityChanged(String),
    RadiusChanged(String),
    ThemeApply,
    ThemeReset,

    // Overview tab
    ShowEmptyToggle(bool),
    ScrollDirPicked(String),
    AnimOpenCurveChanged(String),
    AnimOpenMsChanged(String),
    AnimCloseCurveChanged(String),
    AnimCloseMsChanged(String),
    AnimScrollCurveChanged(String),
    AnimScrollMsChanged(String),
    OverviewApply,

    // Plugins tab
    PluginsFetch,
    PluginsFetched(Vec<PluginRemote>),
    PluginInstall(String),
    PluginInstalled(String, Result<(), String>),
    PluginRemove(String),
    PluginEnable(String),
    PluginDisable(String),
    PluginSettings(String),
    PluginSettingsClose,
    PluginSettingUpdate { plugin: String, key: String, value: String },
    PluginSettingsSave,
    AppRuleAdd,
    AppRuleRemove(String),
    AppRuleNewClassChanged(String),
    AppRuleNewColorChanged(String),

    // Config tab
    ConfigAction(text_editor::Action),
    ConfigSave,
    ConfigReset,
    ConfigReload,
}

/// A plugin available on the remote repository.
#[derive(Debug, Clone)]
struct PluginRemote {
    name: String,          // e.g. "clock"
    filename: String,      // e.g. "clock.lua"
    download_url: String,  // raw GitHub URL
}

/// Local view of a plugin: combines remote info with local state.
#[derive(Debug, Clone)]
struct PluginView {
    name: String,
    download_url: Option<String>,
    installed: bool,
    enabled: bool,
}

// ── State ─────────────────────────────────────────────────────────────────────

struct App {
    tab:             Tab,
    // daemon state
    compositor:      String,
    daemon_ver:      String,
    daemon_on:       bool,
    daemon_vis:      bool,
    status:          String,
    // compositor keybind detection
    comp_statuses:   Vec<CompositorStatus>,
    // bar
    bar_enabled:     bool,
    bar_position:    String,
    // theme
    preset:          String,
    col_bg:          String,
    col_accent:      String,
    col_text:        String,
    col_border:      String,
    opacity:         String,
    radius:          String,
    // overview
    show_empty:      bool,
    scroll_dir:      String,
    anim_open_curve:   String,
    anim_open_ms:      String,
    anim_close_curve:  String,
    anim_close_ms:     String,
    anim_scroll_curve: String,
    anim_scroll_ms:    String,
    // plugins
    plugins_remote:  Vec<PluginRemote>,
    plugins_status:  String,
    plugins_loading: bool,
    plugin_settings_open: Option<String>,
    plugin_settings_data: std::collections::HashMap<String, String>,
    app_rules_new_class: String,
    app_rules_new_color: String,
    // config editor
    config_content:  text_editor::Content,
    config_dirty:    bool,
}

impl Default for App {
    fn default() -> Self {
        let theme    = parse_theme_from_config();
        let bar      = parse_bar_from_config();
        let overview = parse_overview_from_config();
        Self {
            tab:             Tab::Status,
            compositor:      "detecting\u{2026}".into(),
            daemon_ver:      "\u{2013}".into(),
            daemon_on:       false,
            daemon_vis:      false,
            status:          String::new(),
            comp_statuses:   detect_all(),
            bar_enabled:     bar.enabled,
            bar_position:    bar.position,
            preset:          theme.preset,
            col_bg:          theme.background,
            col_accent:      theme.accent,
            col_text:        theme.text,
            col_border:      theme.border,
            opacity:         theme.opacity,
            radius:          theme.border_radius,
            show_empty:      overview.show_empty,
            scroll_dir:      overview.scroll_dir,
            anim_open_curve:   overview.anim_open_curve,
            anim_open_ms:      overview.anim_open_ms,
            anim_close_curve:  overview.anim_close_curve,
            anim_close_ms:     overview.anim_close_ms,
            anim_scroll_curve: overview.anim_scroll_curve,
            anim_scroll_ms:    overview.anim_scroll_ms,
            plugins_remote:  Vec::new(),
            plugins_status:  String::new(),
            plugins_loading: false,
            plugin_settings_open: None,
            plugin_settings_data: std::collections::HashMap::new(),
            app_rules_new_class: String::new(),
            app_rules_new_color: String::new(),
            config_content:  text_editor::Content::with_text(&read_config()),
            config_dirty:    false,
        }
    }
}

impl App {
    fn init() -> (Self, Task<Msg>) {
        let app = Self::default();
        let task = Task::perform(
            async {
                tokio::task::spawn_blocking(|| {
                    match send_ipc(IpcCommand::GetStatus) {
                        Some(IpcResponse::Status(s)) => (s.compositor, s.version, s.visible),
                        _ => ("offline".into(), "-".into(), false),
                    }
                }).await.unwrap_or(("offline".to_string(), "-".to_string(), false))
            },
            |(c, v, vis)| Msg::DaemonPoll(c, v, vis),
        );
        (app, task)
    }
}

// ── Update ────────────────────────────────────────────────────────────────────

fn update(s: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::TabSelect(t) => {
            if t == Tab::Config && !s.config_dirty {
                s.config_content = text_editor::Content::with_text(&read_config());
            }
            let auto_fetch = t == Tab::Plugins && s.plugins_remote.is_empty() && !s.plugins_loading;
            s.tab = t;
            if auto_fetch {
                return update(s, Msg::PluginsFetch);
            }
        }
        // ── Status ────────────────────────────────────────────────────────────
        Msg::DaemonPoll(comp, ver, vis) => {
            s.daemon_on  = comp != "offline";
            s.compositor = comp;
            s.daemon_ver = ver;
            s.daemon_vis = vis;
        }
        Msg::DaemonShow   => { send_ipc(IpcCommand::Show);         s.daemon_vis = true;  s.status = "Overlay shown.".into(); }
        Msg::DaemonHide   => { send_ipc(IpcCommand::Hide);         s.daemon_vis = false; s.status = "Overlay hidden.".into(); }
        Msg::DaemonToggle => { send_ipc(IpcCommand::Toggle);       s.status = "Toggled.".into(); }
        Msg::DaemonReload => { send_ipc(IpcCommand::ReloadConfig); s.status = "Config reloaded.".into(); }
        Msg::InjectKeybind(name) => {
            if let Some(cs) = s.comp_statuses.iter().find(|c| c.name == name).cloned() {
                match inject_keybind(&cs) {
                    Ok(()) => {
                        reload_compositor(&cs);
                        s.status = format!("Keybind added to {} config.", name);
                    }
                    Err(e) => s.status = format!("Inject failed: {e}"),
                }
                s.comp_statuses = detect_all(); // refresh status
            }
        }
        Msg::InjectAutostart(name) => {
            if let Some(cs) = s.comp_statuses.iter().find(|c| c.name == name).cloned() {
                match inject_autostart(&cs) {
                    Ok(()) => {
                        reload_compositor(&cs);
                        s.status = format!("Autostart added to {} config.", name);
                    }
                    Err(e) => s.status = format!("Inject failed: {e}"),
                }
                s.comp_statuses = detect_all(); // refresh status
            }
        }

        // ── Bar ───────────────────────────────────────────────────────────────
        Msg::BarEnabledToggle(v) => s.bar_enabled  = v,
        Msg::BarPositionPicked(v) => s.bar_position = v,
        Msg::BarApply => {
            let block  = build_bar_block(s.bar_enabled, &s.bar_position);
            let config = splice_bar_into_config(&read_config(), &block);
            if let Err(e) = validate_lua_syntax(&config) {
                s.status = format!("Lua error — not saved: {e}");
            } else {
                match write_config(&config) {
                    Ok(()) => {
                        s.config_content = text_editor::Content::with_text(&config);
                        s.config_dirty   = false;
                        send_ipc(IpcCommand::ReloadConfig);
                        s.status = "Bar settings saved and applied.".into();
                    }
                    Err(e) => s.status = format!("Write failed: {e}"),
                }
            }
        }

        // ── Theme ─────────────────────────────────────────────────────────────
        Msg::PresetPicked(p) => {
            if p != "Custom" {
                let (bg, ac, txt, bd) = preset_colors(&p);
                s.col_bg = bg.into(); s.col_accent = ac.into();
                s.col_text = txt.into(); s.col_border = bd.into();
            }
            s.preset = p;
        }
        Msg::ColorChanged { field, value } => match field {
            ColorField::Background => s.col_bg     = value,
            ColorField::Accent     => s.col_accent = value,
            ColorField::Text       => s.col_text   = value,
            ColorField::Border     => s.col_border = value,
        },
        Msg::OpacityChanged(v) => s.opacity = v,
        Msg::RadiusChanged(v)  => s.radius  = v,
        Msg::ThemeApply => {
            let opacity: f32 = s.opacity.parse::<f32>().unwrap_or(0.92).clamp(0.0, 1.0);
            let radius:  u32 = s.radius.parse().unwrap_or(12);
            let block  = build_theme_block(&s.col_bg, &s.col_accent, &s.col_text, &s.col_border, radius, opacity);
            let config = splice_theme_into_config(&read_config(), &block);
            if let Err(e) = validate_lua_syntax(&config) {
                s.status = format!("Lua error — not saved: {e}");
            } else {
                match write_config(&config) {
                    Ok(()) => {
                        s.config_content = text_editor::Content::with_text(&config);
                        s.config_dirty   = false;
                        send_ipc(IpcCommand::ReloadConfig);
                        s.status = "Theme saved and applied.".into();
                    }
                    Err(e) => s.status = format!("Write failed: {e}"),
                }
            }
        }
        Msg::ThemeReset => {
            let d = woven_common::types::Theme::default();
            s.col_bg = d.background; s.col_accent = d.accent;
            s.col_text = d.text;     s.col_border = d.border;
            s.opacity = format!("{:.2}", d.opacity);
            s.radius  = d.border_radius.to_string();
            s.preset  = "Catppuccin Mocha".into();
            s.status  = "Reset to defaults (not saved yet).".into();
        }

        // ── Overview ──────────────────────────────────────────────────────────
        Msg::ShowEmptyToggle(v)          => s.show_empty       = v,
        Msg::ScrollDirPicked(v)          => s.scroll_dir       = v,
        Msg::AnimOpenCurveChanged(v)     => s.anim_open_curve   = v,
        Msg::AnimOpenMsChanged(v)        => s.anim_open_ms      = v,
        Msg::AnimCloseCurveChanged(v)    => s.anim_close_curve  = v,
        Msg::AnimCloseMsChanged(v)       => s.anim_close_ms     = v,
        Msg::AnimScrollCurveChanged(v)   => s.anim_scroll_curve = v,
        Msg::AnimScrollMsChanged(v)      => s.anim_scroll_ms    = v,
        Msg::OverviewApply => {
            let ws_block   = build_workspaces_block(s.show_empty);
            let set_block  = build_settings_block(&s.scroll_dir);
            let anim_block = build_animations_block(
                &s.anim_open_curve,   &s.anim_open_ms,
                &s.anim_close_curve,  &s.anim_close_ms,
                &s.anim_scroll_curve, &s.anim_scroll_ms,
            );
            let mut config = read_config();
            config = splice_workspaces_into_config(&config, &ws_block);
            config = splice_settings_into_config(&config, &set_block);
            config = splice_animations_into_config(&config, &anim_block);
            if let Err(e) = validate_lua_syntax(&config) {
                s.status = format!("Lua error — not saved: {e}");
            } else {
                match write_config(&config) {
                    Ok(()) => {
                        s.config_content = text_editor::Content::with_text(&config);
                        s.config_dirty   = false;
                        send_ipc(IpcCommand::ReloadConfig);
                        s.status = "Overview settings saved and applied.".into();
                    }
                    Err(e) => s.status = format!("Write failed: {e}"),
                }
            }
        }

        // ── Plugins ───────────────────────────────────────────────────────────
        Msg::PluginsFetch => {
            s.plugins_loading = true;
            s.plugins_status = "Fetching plugin list...".into();
            return Task::perform(
                async {
                    tokio::task::spawn_blocking(fetch_plugin_list)
                        .await
                        .unwrap_or_default()
                },
                Msg::PluginsFetched,
            );
        }
        Msg::PluginsFetched(list) => {
            s.plugins_loading = false;
            if list.is_empty() {
                s.plugins_status = "Failed to fetch plugin list from GitHub.".into();
            } else {
                s.plugins_status = format!("Found {} plugins.", list.len());
            }
            s.plugins_remote = list;
        }
        Msg::PluginInstall(name) => {
            if let Some(p) = s.plugins_remote.iter().find(|p| p.name == name) {
                let url = p.download_url.clone();
                let filename = p.filename.clone();
                let name_c = name.clone();
                s.plugins_status = format!("Installing {}...", name);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || install_plugin(&name_c, &filename, &url))
                            .await
                            .unwrap_or(Err("spawn failed".into()))
                    },
                    move |r| Msg::PluginInstalled(name, r),
                );
            }
        }
        Msg::PluginInstalled(name, result) => match result {
            Ok(()) => s.plugins_status = format!("{} installed.", name),
            Err(e) => s.plugins_status = format!("Install failed: {}", e),
        },
        Msg::PluginRemove(name) => {
            let path = plugins_dir().join(format!("{}.lua", name));
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    // Also disable if enabled
                    disable_plugin_in_config(&name);
                    s.plugins_status = format!("{} removed.", name);
                }
                Err(e) => s.plugins_status = format!("Remove failed: {}", e),
            }
        }
        Msg::PluginEnable(name) => {
            enable_plugin_in_config(&name);
            send_ipc(IpcCommand::ReloadConfig);
            s.plugins_status = format!("{} enabled and config reloaded.", name);
        }
        Msg::PluginDisable(name) => {
            disable_plugin_in_config(&name);
            send_ipc(IpcCommand::ReloadConfig);
            s.plugins_status = format!("{} disabled and config reloaded.", name);
        }
        Msg::PluginSettings(name) => {
            // Load current settings for this plugin from config
            s.plugin_settings_data = load_plugin_settings(&name);
            s.plugin_settings_open = Some(name);
        }
        Msg::PluginSettingsClose => {
            s.plugin_settings_open = None;
            s.plugin_settings_data.clear();
        }
        Msg::PluginSettingUpdate { key, value, .. } => {
            s.plugin_settings_data.insert(key, value);
        }
        Msg::PluginSettingsSave => {
            if let Some(plugin) = &s.plugin_settings_open.clone() {
                let pending = {
                    let raw = read_config();
                    apply_plugin_settings_dry(plugin, &s.plugin_settings_data, &raw)
                };
                match pending {
                    Some(new_config) => {
                        if let Err(e) = validate_lua_syntax(&new_config) {
                            s.plugins_status = format!("Lua error — not saved: {e}");
                        } else if write_config(&new_config).is_ok() {
                            s.config_content = text_editor::Content::with_text(&new_config);
                            s.config_dirty = false;
                            send_ipc(IpcCommand::ReloadConfig);
                            s.plugins_status = format!("{} settings saved.", plugin);
                            s.plugin_settings_open = None;
                            s.plugin_settings_data.clear();
                        } else {
                            s.plugins_status = "Failed to write config.".into();
                        }
                    }
                    None => s.plugins_status = "Failed to build plugin settings.".into(),
                }
            }
        }
        Msg::AppRuleAdd => {
            let class = s.app_rules_new_class.trim().to_lowercase();
            let color = s.app_rules_new_color.trim().to_string();
            if !class.is_empty() && color.starts_with('#') && color.len() >= 4 {
                let count: usize = s.plugin_settings_data.get("rule_count")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
                s.plugin_settings_data.insert(format!("rule_{}_class", count), class);
                s.plugin_settings_data.insert(format!("rule_{}_color", count), color);
                s.plugin_settings_data.insert("rule_count".into(), (count + 1).to_string());
                s.app_rules_new_class.clear();
                s.app_rules_new_color.clear();
            }
        }
        Msg::AppRuleRemove(idx_str) => {
            if let Ok(idx) = idx_str.parse::<usize>() {
                let count: usize = s.plugin_settings_data.get("rule_count")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
                if idx < count {
                    for i in idx..count - 1 {
                        let nc = s.plugin_settings_data.get(&format!("rule_{}_class", i + 1)).cloned().unwrap_or_default();
                        let nv = s.plugin_settings_data.get(&format!("rule_{}_color", i + 1)).cloned().unwrap_or_default();
                        s.plugin_settings_data.insert(format!("rule_{}_class", i), nc);
                        s.plugin_settings_data.insert(format!("rule_{}_color", i), nv);
                    }
                    s.plugin_settings_data.remove(&format!("rule_{}_class", count - 1));
                    s.plugin_settings_data.remove(&format!("rule_{}_color", count - 1));
                    s.plugin_settings_data.insert("rule_count".into(), (count - 1).to_string());
                }
            }
        }
        Msg::AppRuleNewClassChanged(v) => s.app_rules_new_class = v,
        Msg::AppRuleNewColorChanged(v) => s.app_rules_new_color = v,

        // ── Config ────────────────────────────────────────────────────────────
        Msg::ConfigAction(a) => { s.config_content.perform(a); s.config_dirty = true; }
        Msg::ConfigReload => {
            s.config_content = text_editor::Content::with_text(&read_config());
            s.config_dirty   = false;
            s.status         = "Reloaded from disk.".into();
        }
        Msg::ConfigSave => {
            let src = s.config_content.text();
            if let Err(e) = validate_lua_syntax(&src) {
                s.status = format!("Lua error — not saved: {e}");
            } else {
                match write_config(&src) {
                    Ok(()) => {
                        send_ipc(IpcCommand::ReloadConfig);
                        s.config_dirty = false;
                        s.status = format!("Saved to {}", config_path());
                    }
                    Err(e) => s.status = format!("Write failed: {e}"),
                }
            }
        }
        Msg::ConfigReset => {
            s.config_content = text_editor::Content::with_text(&default_config());
            s.config_dirty   = true;
            s.status         = "Reset to default (press Save to write).".into();
        }
    }
    Task::none()
}

// ── View ──────────────────────────────────────────────────────────────────────

fn view(s: &App) -> Element<'_, Msg> {
    let tab_bar = row![
        tab_btn("Status",   Tab::Status,   &s.tab),
        tab_btn("Bar",      Tab::Bar,      &s.tab),
        tab_btn("Theme",    Tab::Theme,    &s.tab),
        tab_btn("Overview", Tab::Overview, &s.tab),
        tab_btn("Plugins",  Tab::Plugins,  &s.tab),
        tab_btn("Config",   Tab::Config,   &s.tab),
    ].spacing(4).padding([8u16, 12u16]);

    let body: Element<Msg> = match s.tab {
        Tab::Status   => view_status(s),
        Tab::Bar      => view_bar(s),
        Tab::Theme    => view_theme(s),
        Tab::Overview => view_overview(s),
        Tab::Plugins  => view_plugins(s),
        Tab::Config   => view_config(s),
    };

    let dot = if s.daemon_on { "● " } else { "○ " };
    let status_bar = container(
        row![
            text(format!("{}daemon {}  |  {}", dot, s.daemon_ver, s.compositor)).size(11),
            Space::new().width(Length::Fill),
            text(&s.status).size(11),
        ].align_y(Alignment::Center).spacing(8),
    ).padding([5u16, 14u16]).width(Length::Fill);

    let main_ui: Element<Msg> = column![tab_bar, rule::horizontal(1), body, rule::horizontal(1), status_bar].into();

    // Overlay plugin settings modal if open
    if let Some(plugin_name) = &s.plugin_settings_open {
        use iced::widget::stack;
        stack![
            main_ui,
            view_plugin_settings_modal(s, plugin_name),
        ].into()
    } else {
        main_ui
    }
}

// ── Status tab ────────────────────────────────────────────────────────────────

fn view_status(s: &App) -> Element<'_, Msg> {
    let online_color: Color = if s.daemon_on {
        Color::from_rgb(0.63, 0.85, 0.63)
    } else {
        Color::from_rgb(0.85, 0.47, 0.47)
    };

    let daemon_info: Element<Msg> = if s.daemon_on {
        column![
            text(format!("version:    {}", s.daemon_ver)).size(13),
            text(format!("compositor: {}", s.compositor)).size(13),
            text(format!("overlay:    {}", if s.daemon_vis { "visible" } else { "hidden" })).size(13),
        ].spacing(6).into()
    } else {
        text("Run `woven` in a terminal or add it to your compositor autostart.").size(12).into()
    };

    let mut col = column![
        text("woven daemon").size(22),
        row![
            text(if s.daemon_on { "●" } else { "○" }).size(18).color(online_color),
            text(if s.daemon_on { "Running" } else { "Offline" }).size(14),
        ].spacing(8).align_y(Alignment::Center),
        daemon_info,

        rule::horizontal(1),
        text("Overlay").size(16),
        row![
            ctrl_btn("Show",          Msg::DaemonShow,   s.daemon_on),
            ctrl_btn("Hide",          Msg::DaemonHide,   s.daemon_on),
            ctrl_btn("Toggle",        Msg::DaemonToggle, s.daemon_on),
            ctrl_btn("Reload config", Msg::DaemonReload, s.daemon_on),
        ].spacing(8),

        rule::horizontal(1),
        text("CLI reference").size(16),
        cli_ref("woven-ctrl --show",   "Show the overlay"),
        cli_ref("woven-ctrl --hide",   "Hide the overlay"),
        cli_ref("woven-ctrl --toggle", "Toggle (use for keybinds)"),
        cli_ref("woven-ctrl --reload", "Reload woven.lua without restart"),

        rule::horizontal(1),
        text("Compositor setup").size(16),
        text("Detected compositors on this system:").size(12),
    ].spacing(14).padding([32u16, 32u16]);

    // Compositor rows — one per installed compositor
    let green  = Color::from_rgb(0.63, 0.85, 0.63);
    let yellow = Color::from_rgb(0.95, 0.85, 0.45);
    let dim    = Color::from_rgb(0.55, 0.55, 0.55);

    for cs in &s.comp_statuses {
        if !cs.installed { continue; }

        let running_tag = if cs.is_running { " (running now)" } else { "" };
        col = col.push(
            column![
                text(format!("{}{}", cs.name, running_tag)).size(14),
                text(format!("config: {}", cs.config_path)).size(10).color(dim),
            ].spacing(2)
        );

        if !cs.config_exists {
            col = col.push(
                text("  config file not found — create it first").size(11).color(yellow)
            );
            continue;
        }

        // Keybind row
        let kb_label = if cs.keybind_present {
            text("  ✓ keybind configured").size(11).color(green)
        } else {
            text("  ✗ keybind not found").size(11).color(yellow)
        };
        let kb_row = if cs.keybind_present {
            row![kb_label].spacing(8)
        } else {
            let name = cs.name.clone();
            row![
                kb_label,
                button("Add keybind").on_press(Msg::InjectKeybind(name)).padding([3u16, 10u16]),
            ].spacing(8).align_y(Alignment::Center)
        };
        col = col.push(kb_row);

        // Autostart row
        let as_label = if cs.autostart_present {
            text("  ✓ autostart configured").size(11).color(green)
        } else {
            text("  ✗ autostart not found").size(11).color(yellow)
        };
        let as_row = if cs.autostart_present {
            row![as_label].spacing(8)
        } else {
            let name = cs.name.clone();
            row![
                as_label,
                button("Add autostart").on_press(Msg::InjectAutostart(name)).padding([3u16, 10u16]),
            ].spacing(8).align_y(Alignment::Center)
        };
        col = col.push(as_row);
    }

    // If nothing installed at all, fall back to static reference
    let any_installed = s.comp_statuses.iter().any(|c| c.installed);
    if !any_installed {
        col = col.push(text("No supported compositors detected.").size(12).color(dim));
    }

    scrollable(col.push(Space::new())).into()
}

// ── Bar tab ───────────────────────────────────────────────────────────────────

fn view_bar(s: &App) -> Element<'_, Msg> {
    let pos_diagram = bar_position_diagram(&s.bar_position);

    scrollable(column![
        text("Bar settings").size(22),
        text("The persistent docked bar. Expand it to open the full control center.").size(11),

        rule::horizontal(1),
        checkbox(s.bar_enabled).label("Enabled").on_toggle(Msg::BarEnabledToggle),

        rule::horizontal(1),
        text("Position").size(15),
        row![
            pick_list(BAR_POSITIONS, Some(s.bar_position.as_str()),
                |p: &str| Msg::BarPositionPicked(p.to_string())).width(120),
            pos_diagram,
        ].spacing(24).align_y(Alignment::Center),

        rule::horizontal(1),
        text("What you get").size(15),
        text("Collapsed (52px):  clock, active workspace dots, CPU%, expand button").size(12),
        text("Expanded (300px):  clock + weather, media controls, WiFi/BT tiles,").size(12),
        text("                   CPU/GPU temps, RAM, volume, power menu").size(12),
        text("Requires:  playerctl  nmcli  bluetoothctl  curl").size(11),

        rule::horizontal(1),
        button("Apply & Save").on_press(Msg::BarApply).padding([6u16, 18u16]),
    ].spacing(14).padding([32u16, 32u16])).into()
}

fn bar_position_diagram(pos: &str) -> Element<'static, Msg> {
    let (top, right, bottom, left) = match pos {
        "top"    => ("[ BAR ]", "     ", "       ", "     "),
        "bottom" => ("       ", "     ", "[ BAR ]", "     "),
        "left"   => ("       ", "     ", "       ", "[ B ]"),
        _        => ("       ", "[ B ]", "       ", "     "),  // right (default)
    };
    container(column![
        text(top).size(11).font(Font::MONOSPACE),
        row![
            text(left).size(11).font(Font::MONOSPACE),
            container(text("screen").size(10)).width(52).padding([10u16, 4u16]),
            text(right).size(11).font(Font::MONOSPACE),
        ].align_y(Alignment::Center),
        text(bottom).size(11).font(Font::MONOSPACE),
    ].align_x(Alignment::Center).spacing(2))
    .padding([10u16, 14u16])
    .style(|_: &Theme| container::Style {
        border: iced::Border { radius: 6.0.into(), width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.15) },
        ..Default::default()
    }).into()
}

// ── Theme tab ─────────────────────────────────────────────────────────────────

fn view_theme(s: &App) -> Element<'_, Msg> {
    let col_row = |label: &'static str, val: String, field: ColorField| {
        row![
            text(label).width(120).size(13),
            text_input("#rrggbb", &val)
                .on_input(move |v| Msg::ColorChanged { field: field.clone(), value: v })
                .width(130).padding(6u16),
            swatch(&val),
        ].spacing(10).align_y(Alignment::Center)
    };

    let editor = scrollable(column![
        text("Theme editor").size(20),
        text("Saves to woven.lua and reloads the daemon live.").size(11),
        rule::horizontal(1),
        pick_list(PRESETS, Some(s.preset.as_str()),
            |p: &str| Msg::PresetPicked(p.to_string())).width(200),
        rule::horizontal(1),
        col_row("Background", s.col_bg.clone(),    ColorField::Background),
        col_row("Accent",     s.col_accent.clone(), ColorField::Accent),
        col_row("Text",       s.col_text.clone(),   ColorField::Text),
        col_row("Border",     s.col_border.clone(), ColorField::Border),
        rule::horizontal(1),
        row![
            text("Opacity (0–1)").width(120).size(13),
            text_input("0.92", &s.opacity).on_input(Msg::OpacityChanged).width(70).padding(6u16),
            text("Border radius").width(120).size(13),
            text_input("12", &s.radius).on_input(Msg::RadiusChanged).width(60).padding(6u16),
        ].spacing(10).align_y(Alignment::Center),
        rule::horizontal(1),
        row![
            button("Apply & Save").on_press(Msg::ThemeApply).padding([6u16, 18u16]),
            button("Reset defaults").on_press(Msg::ThemeReset).padding([6u16, 18u16]),
        ].spacing(8),
    ].spacing(12).padding([28u16, 28u16]))
    .width(Length::FillPortion(1));

    let preview = column![
        text("Preview").size(14),
        preview_card(&s.col_bg, &s.col_accent, &s.col_border, &s.col_text),
    ].spacing(10).padding([28u16, 28u16]).width(Length::FillPortion(1));

    row![editor, rule::vertical(1), preview].into()
}

// ── Overview tab ──────────────────────────────────────────────────────────────

fn view_overview(s: &App) -> Element<'_, Msg> {
    scrollable(column![
        text("Overview settings").size(22),

        rule::horizontal(1),
        text("Workspaces").size(16),
        checkbox(s.show_empty).label("Show empty workspaces").on_toggle(Msg::ShowEmptyToggle),

        rule::horizontal(1),
        text("Scroll direction").size(16),
        pick_list(
            ["horizontal", "vertical"].as_slice(),
            Some(s.scroll_dir.as_str()),
            |v: &str| Msg::ScrollDirPicked(v.to_string()),
        ).width(150),

        rule::horizontal(1),
        text("Animations").size(16),
        text("Curves: ease_out_cubic  ease_in_cubic  ease_in_out_cubic  linear  spring").size(11),

        row![
            text("Overlay open").width(130).size(13),
            pick_list(ANIM_CURVES, Some(s.anim_open_curve.as_str()),
                |c: &str| Msg::AnimOpenCurveChanged(c.to_string())).width(180),
            text("duration").size(12),
            text_input("180", &s.anim_open_ms).on_input(Msg::AnimOpenMsChanged).width(60).padding(6u16),
            text("ms").size(12),
        ].spacing(10).align_y(Alignment::Center),

        row![
            text("Overlay close").width(130).size(13),
            pick_list(ANIM_CURVES, Some(s.anim_close_curve.as_str()),
                |c: &str| Msg::AnimCloseCurveChanged(c.to_string())).width(180),
            text("duration").size(12),
            text_input("120", &s.anim_close_ms).on_input(Msg::AnimCloseMsChanged).width(60).padding(6u16),
            text("ms").size(12),
        ].spacing(10).align_y(Alignment::Center),

        row![
            text("Scroll").width(130).size(13),
            pick_list(ANIM_CURVES, Some(s.anim_scroll_curve.as_str()),
                |c: &str| Msg::AnimScrollCurveChanged(c.to_string())).width(180),
            text("duration").size(12),
            text_input("200", &s.anim_scroll_ms).on_input(Msg::AnimScrollMsChanged).width(60).padding(6u16),
            text("ms").size(12),
        ].spacing(10).align_y(Alignment::Center),

        rule::horizontal(1),
        button("Apply & Save").on_press(Msg::OverviewApply).padding([6u16, 18u16]),
    ].spacing(14).padding([32u16, 32u16])).into()
}

// ── Plugins tab ──────────────────────────────────────────────────────────────

fn view_plugins(s: &App) -> Element<'_, Msg> {
    let green  = Color::from_rgb(0.63, 0.85, 0.63);
    let dim    = Color::from_rgb(0.55, 0.55, 0.55);
    let yellow = Color::from_rgb(0.95, 0.85, 0.45);

    let plugins = build_plugin_views(s);

    let mut col = column![
        text("Plugins").size(22),
        text("Install and manage woven plugins from the official repository.").size(11),
        rule::horizontal(1),
        row![
            button(if s.plugins_loading { "Fetching..." } else { "Refresh from GitHub" })
                .on_press_maybe(if s.plugins_loading { None } else { Some(Msg::PluginsFetch) })
                .padding([6u16, 16u16]),
            Space::new().width(Length::Fill),
            text(&s.plugins_status).size(11).color(dim),
        ].spacing(12).align_y(Alignment::Center),
        rule::horizontal(1),
    ].spacing(14).padding([32u16, 32u16]);

    if plugins.is_empty() {
        col = col.push(
            text("Press \"Refresh from GitHub\" to load available plugins.").size(12).color(dim)
        );
    }

    for p in plugins {
        let status_color = if p.enabled { green } else if p.installed { yellow } else { dim };
        let status_text = if p.enabled { "enabled" } else if p.installed { "installed" } else { "available" };

        let mut action_row = row![].spacing(6).align_y(Alignment::Center);

        if !p.installed && p.download_url.is_some() {
            action_row = action_row.push(
                button("Install").on_press(Msg::PluginInstall(p.name.clone()))
                    .padding([3u16, 10u16])
            );
        }
        if p.installed && !p.enabled {
            action_row = action_row.push(
                button("Enable").on_press(Msg::PluginEnable(p.name.clone()))
                    .padding([3u16, 10u16])
            );
            action_row = action_row.push(
                button("Remove").on_press(Msg::PluginRemove(p.name.clone()))
                    .padding([3u16, 10u16])
            );
        }
        if p.enabled {
            action_row = action_row.push(
                button("Disable").on_press(Msg::PluginDisable(p.name.clone()))
                    .padding([3u16, 10u16])
            );
            // Add Settings button for plugins that need configuration
            if plugin_needs_settings(&p.name) {
                action_row = action_row.push(
                    button("Settings").on_press(Msg::PluginSettings(p.name.clone()))
                        .padding([3u16, 10u16])
                );
            }
        }

        col = col.push(
            container(
                row![
                    column![
                        text(p.name).size(14),
                        text(status_text).size(11).color(status_color),
                    ].spacing(2).width(Length::Fill),
                    action_row,
                ].align_y(Alignment::Center).spacing(12)
            ).padding([8u16, 12u16])
            .style(|_: &Theme| container::Style {
                border: iced::Border {
                    radius: 6.0.into(), width: 1.0,
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                },
                ..Default::default()
            })
        );
    }

    scrollable(col.push(Space::new())).into()
}

/// Render the plugin settings modal overlay.
fn view_plugin_settings_modal<'a>(s: &'a App, plugin_name: &'a str) -> Element<'a, Msg> {
    use iced::widget::mouse_area;
    
    // Modal content
    let modal_content = match plugin_name {
        "date"      => view_date_settings(s),
        "cava"      => view_cava_settings(s),
        "app_rules" => view_app_rules_settings(s),
        "launcher"  => view_launcher_settings(s),
        _ => column![text("No settings available")].into(),
    };

    let modal = container(
        column![
            row![
                text(format!("{} Settings", plugin_name)).size(18),
                Space::new().width(Length::Fill),
                button("✕").on_press(Msg::PluginSettingsClose).padding([4u16, 10u16]),
            ].align_y(Alignment::Center),
            rule::horizontal(1),
            modal_content,
            rule::horizontal(1),
            row![
                button("Save").on_press(Msg::PluginSettingsSave).padding([6u16, 16u16]),
                button("Cancel").on_press(Msg::PluginSettingsClose).padding([6u16, 16u16]),
            ].spacing(8),
        ].spacing(14).padding(20)
    )
    .width(Length::Fixed(500.0))
    .style(|_: &Theme| container::Style {
        background: Some(Color::from_rgb(0.12, 0.12, 0.18).into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        },
        ..Default::default()
    });

    // Backdrop that closes modal when clicked
    let backdrop = mouse_area(
        container(
            container(modal)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.5).into()),
            ..Default::default()
        })
    ).on_press(Msg::PluginSettingsClose);

    backdrop.into()
}

/// Date plugin settings UI.
fn view_date_settings(s: &App) -> Element<'_, Msg> {
    let accent = s.plugin_settings_data.get("accent_color").map(|s| s.as_str()).unwrap_or("#cba6f7");
    let text_col = s.plugin_settings_data.get("text_color").map(|s| s.as_str()).unwrap_or("#cdd6f4");

    column![
        text("Date Badge Colors").size(14),
        row![
            text("Accent").size(12).width(100),
            text_input("#rrggbb", accent)
                .on_input(|v| Msg::PluginSettingUpdate {
                    plugin: "date".into(), key: "accent_color".into(), value: v,
                })
                .width(130).padding(6u16),
            swatch(accent),
        ].spacing(8).align_y(Alignment::Center),
        row![
            text("Text").size(12).width(100),
            text_input("#rrggbb", text_col)
                .on_input(|v| Msg::PluginSettingUpdate {
                    plugin: "date".into(), key: "text_color".into(), value: v,
                })
                .width(130).padding(6u16),
            swatch(text_col),
        ].spacing(8).align_y(Alignment::Center),
    ].spacing(10).into()
}

/// Cava plugin settings UI.
fn view_cava_settings(s: &App) -> Element<'_, Msg> {
    let theme = s.plugin_settings_data.get("theme").map(|s| s.as_str()).unwrap_or("catppuccin");
    let themes: Vec<&str> = vec!["catppuccin", "gruvbox", "nord", "tokyo_night", "dracula"];

    column![
        text("Audio Visualizer").size(14),
        row![
            text("Color theme").size(12).width(100),
            pick_list(themes, Some(theme), |selected: &str| {
                Msg::PluginSettingUpdate {
                    plugin: "cava".into(), key: "theme".into(), value: selected.into(),
                }
            }).width(180),
        ].spacing(8).align_y(Alignment::Center),
        text("Themes match your favorite color palettes.").size(10).color(Color::from_rgb(0.5, 0.5, 0.5)),
    ].spacing(10).into()
}

/// App rules settings UI — per-app accent color overrides.
fn view_app_rules_settings(s: &App) -> Element<'_, Msg> {
    let dim = Color::from_rgb(0.5, 0.5, 0.5);
    let count: usize = s.plugin_settings_data.get("rule_count")
        .and_then(|v| v.parse().ok()).unwrap_or(0);

    let mut col = column![
        text("Per-app accent colors").size(14),
        text("Override auto-generated hash colors for specific window classes.").size(10).color(dim),
    ].spacing(8);

    for i in 0..count {
        let class = s.plugin_settings_data.get(&format!("rule_{}_class", i))
            .map(|s| s.as_str()).unwrap_or("");
        let color = s.plugin_settings_data.get(&format!("rule_{}_color", i))
            .map(|s| s.as_str()).unwrap_or("#89b4fa");
        let idx = i;
        col = col.push(
            row![
                text_input("class name", class)
                    .on_input(move |v| Msg::PluginSettingUpdate {
                        plugin: "app_rules".into(),
                        key: format!("rule_{}_class", idx),
                        value: v,
                    })
                    .width(150).padding(5u16),
                text_input("#rrggbb", color)
                    .on_input(move |v| Msg::PluginSettingUpdate {
                        plugin: "app_rules".into(),
                        key: format!("rule_{}_color", idx),
                        value: v,
                    })
                    .width(120).padding(5u16),
                swatch(color),
                button("x").on_press(Msg::AppRuleRemove(i.to_string())).padding([3u16, 8u16]),
            ].spacing(6).align_y(Alignment::Center)
        );
    }

    // Add new rule row
    col = col.push(
        row![
            text_input("class name", &s.app_rules_new_class)
                .on_input(Msg::AppRuleNewClassChanged)
                .width(150).padding(5u16),
            text_input("#rrggbb", &s.app_rules_new_color)
                .on_input(Msg::AppRuleNewColorChanged)
                .width(120).padding(5u16),
            swatch(&s.app_rules_new_color),
            button("+").on_press(Msg::AppRuleAdd).padding([3u16, 8u16]),
        ].spacing(6).align_y(Alignment::Center)
    );

    scrollable(col).height(Length::Fixed(300.0)).into()
}

/// Launcher plugin settings UI with installed app detection.
fn view_launcher_settings(s: &App) -> Element<'_, Msg> {
    let current_cmd = s.plugin_settings_data.get("cmd").map(|s| s.as_str()).unwrap_or("kitty");

    let apps_to_check = vec![
        "kitty", "alacritty", "foot", "wezterm", "konsole",
        "gnome-terminal", "xfce4-terminal", "firefox", "chromium",
        "brave", "nautilus", "thunar", "dolphin",
    ];
    let installed_apps: Vec<&str> = apps_to_check.iter()
        .filter(|&&app| is_app_installed(app))
        .copied()
        .collect();

    if installed_apps.is_empty() {
        return column![
            text("No supported apps detected").size(14),
            text("Install one of: kitty, alacritty, foot, firefox, etc.").size(11),
        ].spacing(10).into();
    }

    column![
        text("Application Launcher").size(14),
        row![
            text("App").size(12).width(100),
            pick_list(installed_apps, Some(current_cmd), |selected: &str| {
                Msg::PluginSettingUpdate {
                    plugin: "launcher".into(), key: "cmd".into(), value: selected.into(),
                }
            }).width(180),
        ].spacing(8).align_y(Alignment::Center),
        text(format!("Will launch: {}", current_cmd)).size(10).color(Color::from_rgb(0.5, 0.5, 0.5)),
    ].spacing(10).into()
}

fn plugins_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config",
            std::env::var("HOME").unwrap_or_else(|_| ".".into())));
    std::path::PathBuf::from(format!("{}/woven/plugins", base))
}

/// Fetch the list of .lua files from the GitHub repo plugins/ directory.
fn fetch_plugin_list() -> Vec<PluginRemote> {
    let url = "https://api.github.com/repos/viewerofall/woven/contents/plugins";
    let resp = ureq::get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "woven-ctrl")
        .call();
    let body = match resp {
        Ok(r) => r.into_body().read_to_string().unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    let items: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap_or_default();
    items.iter().filter_map(|item| {
        let name = item["name"].as_str()?;
        if !name.ends_with(".lua") { return None; }
        let stem = name.strip_suffix(".lua")?;
        let download_url = item["download_url"].as_str()?.to_string();
        Some(PluginRemote {
            name: stem.to_string(),
            filename: name.to_string(),
            download_url,
        })
    }).collect()
}

/// Download a plugin .lua file to the plugins directory.
fn install_plugin(_name: &str, filename: &str, url: &str) -> Result<(), String> {
    let dir = plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let resp = ureq::get(url)
        .header("User-Agent", "woven-ctrl")
        .call()
        .map_err(|e| e.to_string())?;
    let body = resp.into_body().read_to_string().map_err(|e| e.to_string())?;
    let path = dir.join(filename);
    std::fs::write(&path, &body).map_err(|e| e.to_string())
}

/// Build a merged list of all known plugins (remote + local).
fn build_plugin_views(s: &App) -> Vec<PluginView> {
    let config = read_config();
    let dir = plugins_dir();

    let mut map: std::collections::BTreeMap<String, PluginView> = std::collections::BTreeMap::new();

    // Remote plugins
    for p in &s.plugins_remote {
        map.insert(p.name.clone(), PluginView {
            name: p.name.clone(),
            download_url: Some(p.download_url.clone()),
            installed: false,
            enabled: false,
        });
    }

    // Local plugins
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".lua") { continue; }
            let stem = fname.strip_suffix(".lua").unwrap_or(&fname).to_string();
            map.entry(stem.clone()).or_insert_with(|| PluginView {
                name: stem.clone(),
                download_url: None,
                installed: false,
                enabled: false,
            }).installed = true;
        }
    }

    // Check which are enabled in woven.lua
    for pv in map.values_mut() {
        let pattern = format!("require(\"plugins.{}\")", pv.name);
        let pattern2 = format!("require('plugins.{}')", pv.name);
        pv.enabled = config.contains(&pattern) || config.contains(&pattern2);
    }

    map.into_values().collect()
}

/// Add a require line for a plugin to woven.lua.
fn enable_plugin_in_config(name: &str) {
    let config = read_config();
    // Don't add if already present
    if config.contains(&format!("require(\"plugins.{}\")", name))
        || config.contains(&format!("require('plugins.{}')", name))
    {
        return;
    }
    let require_line = format!("require(\"plugins.{}\").setup()", name);
    let new_config = format!("{}\n{}\n", config.trim_end(), require_line);
    let _ = write_config(&new_config);
}

/// Remove the require line for a plugin from woven.lua.
fn disable_plugin_in_config(name: &str) {
    let config = read_config();
    // Match exact require patterns to avoid clobbering similar names
    // (e.g. "date" must not match "date_extended") or removing comments.
    let pat_dq = format!("require(\"plugins.{}\")", name);
    let pat_sq = format!("require('plugins.{}')", name);
    let new_config: String = config.lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Skip commented-out lines
            if trimmed.starts_with("--") { return true; }
            // Only remove lines that contain the exact require pattern
            !trimmed.contains(&pat_dq) && !trimmed.contains(&pat_sq)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let new_config = format!("{}\n", new_config.trim_end());
    let _ = write_config(&new_config);
}

/// Determine if a plugin needs a settings button.
fn plugin_needs_settings(name: &str) -> bool {
    matches!(name, "date" | "cava" | "app_rules" | "launcher")
}

/// Check if an app is installed on the system.
fn is_app_installed(app: &str) -> bool {
    std::process::Command::new("which")
        .arg(app)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Load current settings for a plugin from the config file.
fn load_plugin_settings(name: &str) -> std::collections::HashMap<String, String> {
    let mut settings = std::collections::HashMap::new();
    let config = read_config();
    let opts = extract_plugin_opts(&config, name);

    match name {
        "date" => {
            settings.insert("accent_color".into(),
                opt_str(&opts, "accent_color").unwrap_or_else(|| "#cba6f7".into()));
            settings.insert("text_color".into(),
                opt_str(&opts, "text_color").unwrap_or_else(|| "#cdd6f4".into()));
        }
        "cava" => {
            settings.insert("theme".into(),
                opt_str(&opts, "theme").unwrap_or_else(|| "catppuccin".into()));
        }
        "app_rules" => {
            let rules = parse_lua_bracket_table(&opts);
            settings.insert("rule_count".into(), rules.len().to_string());
            for (i, (class, color)) in rules.iter().enumerate() {
                settings.insert(format!("rule_{}_class", i), class.clone());
                settings.insert(format!("rule_{}_color", i), color.clone());
            }
        }
        "launcher" => {
            let cmd = opt_str(&opts, "cmd").unwrap_or_else(|| "kitty".into());
            settings.insert("cmd".into(), cmd);
        }
        _ => {}
    }

    settings
}

/// Apply plugin settings by updating the config file.
/// Build the new config string for plugin settings without writing it.
/// Returns `None` if the plugin name is unknown.
fn apply_plugin_settings_dry(
    name: &str,
    settings: &std::collections::HashMap<String, String>,
    config: &str,
) -> Option<String> {
    let opts     = extract_plugin_opts(config, name);
    // Use opt_str/opt_num — they handle both inline (single-line) and multi-line opts.
    let slot     = opt_str(&opts, "slot").unwrap_or_default();
    let height   = opt_num(&opts, "height").unwrap_or_default();
    let interval = opt_num(&opts, "interval").unwrap_or_default();

    // Build positional opts only when we actually have them, to avoid a
    // leading comma like `{ , key = val }` (invalid Lua).
    let widget_opts = if !slot.is_empty() {
        let h = if height.is_empty() { "0".to_string() } else { height };
        let i = if interval.is_empty() { "0".to_string() } else { interval };
        format!("slot = \"{}\", height = {}, interval = {}", slot, h, i)
    } else {
        String::new()
    };

    // Prefix separator: only add ", " between widget_opts and plugin-specific opts
    // when widget_opts is non-empty.
    let sep = if widget_opts.is_empty() { "" } else { ", " };

    let block = match name {
        "date" => {
            let accent = settings.get("accent_color").cloned().unwrap_or_else(|| "#cba6f7".into());
            let txt    = settings.get("text_color").cloned().unwrap_or_else(|| "#cdd6f4".into());
            format!(
                "require(\"plugins.date\").setup({{ {}{}accent_color = \"{}\", text_color = \"{}\" }})",
                widget_opts, sep, accent, txt
            )
        }
        "cava" => {
            let theme = settings.get("theme").cloned().unwrap_or_else(|| "catppuccin".into());
            format!(
                "require(\"plugins.cava\").setup({{ {}{}theme = \"{}\" }})",
                widget_opts, sep, theme
            )
        }
        "app_rules" => {
            let count: usize = settings.get("rule_count")
                .and_then(|s| s.parse().ok()).unwrap_or(0);
            let entries: String = (0..count)
                .filter_map(|i| {
                    let class = settings.get(&format!("rule_{}_class", i))?;
                    let color = settings.get(&format!("rule_{}_color", i))?;
                    if class.is_empty() { return None; }
                    Some(format!("    [\"{}\"] = \"{}\",\n", class, color))
                })
                .collect();
            format!("require(\"plugins.app_rules\").setup({{\n{}}})", entries)
        }
        "launcher" => {
            // Always sync label = cmd so the icon updates when the app changes.
            let cmd = settings.get("cmd").cloned().unwrap_or_else(|| "kitty".into());
            format!(
                "require(\"plugins.launcher\").setup({{ {}{}label = \"{}\", cmd = \"{}\" }})",
                widget_opts, sep, cmd, cmd
            )
        }
        _ => return None,
    };

    Some(splice_plugin_setup(config, name, &block))
}

#[allow(dead_code)]
fn apply_plugin_settings(name: &str, settings: &std::collections::HashMap<String, String>) -> bool {
    let config = read_config();
    match apply_plugin_settings_dry(name, settings, &config) {
        Some(new_config) => write_config(&new_config).is_ok(),
        None => false,
    }
}

// ── Config tab ────────────────────────────────────────────────────────────────

fn view_config(s: &App) -> Element<'_, Msg> {
    let save_label = if s.config_dirty { "Save *" } else { "Save" };
    column![
        row![
            text("Config editor").size(20),
            Space::new().width(Length::Fill),
            text(config_path()).size(10),
        ].align_y(Alignment::Center).padding([12u16, 28u16]),

        text_editor(&s.config_content)
            .on_action(Msg::ConfigAction)
            .font(Font::MONOSPACE)
            .height(Length::Fill)
            .padding(12u16),

        row![
            button(save_label).on_press(Msg::ConfigSave).padding([6u16, 16u16]),
            button("Reload from disk").on_press(Msg::ConfigReload).padding([6u16, 16u16]),
            button("Reset to default").on_press(Msg::ConfigReset).padding([6u16, 16u16]),
        ].spacing(8).padding([8u16, 28u16]),
    ]
    .height(Length::Fill)
    .into()
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn tab_btn<'a>(label: &'a str, tab: Tab, current: &Tab) -> Element<'a, Msg> {
    let active = &tab == current;
    button(text(label).size(13))
        .on_press(Msg::TabSelect(tab))
        .padding([5u16, 16u16])
        .style(if active { button::primary } else { button::secondary })
        .into()
}

fn ctrl_btn(label: &str, msg: Msg, enabled: bool) -> Element<'_, Msg> {
    let b = button(text(label).size(12)).padding([6u16, 14u16]);
    if enabled { b.on_press(msg) } else { b }.into()
}

fn cli_ref<'a>(cmd: &'a str, desc: &'a str) -> Element<'a, Msg> {
    row![
        text(cmd).size(12).font(Font::MONOSPACE).width(300),
        text(desc).size(12),
    ].spacing(8).align_y(Alignment::Center).into()
}

fn hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 { return Color::from_rgb(0.2, 0.2, 0.2); }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32 / 255.0;
    Color::from_rgb(r, g, b)
}

fn swatch(hex: &str) -> Element<'static, Msg> {
    let c = hex_color(hex);
    container(text("")).width(22).height(22)
        .style(move |_: &Theme| container::Style {
            background: Some(iced::Background::Color(c)),
            border: iced::Border { radius: 4.0.into(), width: 1.0,
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.15) },
            ..Default::default()
        }).into()
}

fn preview_card<'a>(bg: &str, accent: &str, border: &str, txt: &str) -> Element<'a, Msg> {
    let bg_c = hex_color(bg);
    let ac_c = hex_color(accent);
    let bd_c = hex_color(border);
    let tx_c = hex_color(txt);

    let win = |name: &'static str| -> Element<'static, Msg> {
        container(text(name).size(11).color(tx_c))
            .padding([4u16, 8u16]).width(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.07))),
                border: iced::Border { radius: 4.0.into(), width: 0.5,
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.12) },
                ..Default::default()
            }).into()
    };

    container(column![
        container(text("workspace 1").size(11).color(ac_c))
            .padding([5u16, 10u16]).width(Length::Fill),
        column![win("Firefox"), win("Alacritty"), win("Terminal")]
            .spacing(4).padding([0u16, 10u16]),
    ])
    .width(240)
    .style(move |_: &Theme| container::Style {
        background: Some(iced::Background::Color(bg_c)),
        border: iced::Border { radius: 10.0.into(), width: 1.5, color: bd_c },
        ..Default::default()
    }).into()
}

// ── self-update ──────────────────────────────────────────────────────────────

fn self_update() {
    use std::process::Command;

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let bin_dir = format!("{}/.local/bin", home);
    let config_dir = format!("{}/.config/woven", home);
    let service_dir = format!("{}/.config/systemd/user", home);
    let tmp = format!("{}/woven-update-{}", std::env::temp_dir().display(), std::process::id());

    let run = |name: &str, cmd: &mut Command| -> bool {
        match cmd.status() {
            Ok(s) if s.success() => true,
            Ok(s) => { eprintln!("  {} exited with {}", name, s); false }
            Err(e) => { eprintln!("  {} failed: {}", name, e); false }
        }
    };

    println!("==> Stopping woven daemon...");
    let _ = Command::new("systemctl").args(["--user", "stop", "woven"]).status();
    // Wait for the process to fully exit so binary files are no longer busy
    std::thread::sleep(std::time::Duration::from_millis(500));

    println!("==> Downloading latest release...");
    let _ = std::fs::create_dir_all(&tmp);
    let tarball = format!("{}/woven.tar.gz", tmp);
    let url = "https://github.com/viewerofall/woven/releases/latest/download/v2.2.2.tar.gz";
    let fallback = "https://raw.githubusercontent.com/viewerofall/woven/main/v2.2.2.tar.gz";

    let downloaded = run("curl", Command::new("curl").args(["-fsSL", url, "-o", &tarball]))
        || run("curl (fallback)", Command::new("curl").args(["-fsSL", fallback, "-o", &tarball]));

    if !downloaded {
        eprintln!("==> Download failed. Restarting old version...");
        let _ = Command::new("systemctl").args(["--user", "start", "woven"]).status();
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }

    println!("==> Extracting...");
    if !run("tar", Command::new("tar").args(["-xzf", &tarball, "-C", &tmp])) {
        eprintln!("==> Extract failed. Restarting old version...");
        let _ = Command::new("systemctl").args(["--user", "start", "woven"]).status();
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }

    // Find extracted directory (first subdir in tmp)
    let src = std::fs::read_dir(&tmp).ok()
        .and_then(|mut d| d.find_map(|e| {
            let e = e.ok()?;
            if e.file_type().ok()?.is_dir() { Some(e.path()) } else { None }
        }))
        .unwrap_or(std::path::PathBuf::from(&tmp));

    println!("==> Installing binaries...");
    let _ = std::fs::create_dir_all(&bin_dir);
    let exec_dir = src.join("exec");
    if exec_dir.exists() {
        for bin in ["woven", "woven-ctrl"] {
            let from = exec_dir.join(bin);
            if from.exists() {
                let to = format!("{}/{}", bin_dir, bin);
                let tmp_to = format!("{}.new", to);
                // Copy to a temp name, then rename over the old binary.
                // rename() is atomic and avoids ETXTBSY — the kernel keeps
                // the old inode alive for the running process but the
                // directory entry now points to the new file.
                if let Err(e) = std::fs::copy(&from, &tmp_to) {
                    eprintln!("  failed to copy {}: {}", bin, e);
                } else if let Err(e) = std::fs::rename(&tmp_to, &to) {
                    eprintln!("  failed to rename {}: {}", bin, e);
                    let _ = std::fs::remove_file(&tmp_to);
                }
            }
        }
    } else {
        eprintln!("  no exec/ directory in tarball — binaries not updated");
    }

    println!("==> Updating runtime...");
    let runtime_src = src.join("runtime");
    if runtime_src.exists() {
        run("cp runtime", Command::new("cp").args(["-r",
            &runtime_src.to_string_lossy(),
            &format!("{}/", config_dir)]));
    }

    // Update service file if present
    let service_src = src.join("woven.service");
    if service_src.exists() {
        let _ = std::fs::create_dir_all(&service_dir);
        let _ = std::fs::copy(&service_src, format!("{}/woven.service", service_dir));
    }

    println!("==> Reloading and starting...");
    let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
    run("systemctl start", Command::new("systemctl").args(["--user", "start", "woven"]));

    let _ = std::fs::remove_dir_all(&tmp);
    println!("==> woven updated successfully.");
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> iced::Result {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--show"   => { send_ipc(IpcCommand::Show);         return Ok(()); }
            "--hide"   => { send_ipc(IpcCommand::Hide);         return Ok(()); }
            "--toggle" => { send_ipc(IpcCommand::Toggle);       return Ok(()); }
            "--reload" => {
                // Full reload: restart the daemon so all config (theme, plugins,
                // bar, namer, widgets, animations) is re-evaluated from scratch.
                // A partial in-process hot-reload only covered theme — this is
                // the only way to pick up plugin/widget/namer changes.
                let status = std::process::Command::new("systemctl")
                    .args(["--user", "restart", "woven"])
                    .status();
                match status {
                    Ok(s) if s.success() => eprintln!("woven reloaded."),
                    Ok(s)  => eprintln!("reload failed (exit {})", s),
                    Err(e) => eprintln!("reload failed: {}", e),
                }
                return Ok(());
            }
            "--update" => { self_update(); return Ok(()); }
            "--setup"  => {
                return iced::application(setup::Setup::init, setup::update, setup::view)
                    .title(|_: &setup::Setup| "woven — first time setup".to_string())
                    .window(iced::window::Settings {
                        size: iced::Size::new(620.0, 480.0),
                        resizable: false,
                        ..Default::default()
                    })
                    .theme(|_: &setup::Setup| iced::Theme::CatppuccinMocha)
                    .run();
            }
            _ => {}
        }
    }

    iced::application(App::init, update, view)
        .title(|_: &App| "woven-ctrl".to_string())
        .window(iced::window::Settings {
            size: iced::Size::new(900.0, 600.0),
            resizable: true,
            ..Default::default()
        })
        .theme(|_: &App| Theme::CatppuccinMocha)
        .run()
}
