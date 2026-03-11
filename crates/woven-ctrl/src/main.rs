//! woven-ctrl — control panel + CLI for the woven daemon.
//!
//! CLI (no GUI, exits immediately):
//!   woven-ctrl --show | --hide | --toggle | --reload | --setup

mod setup;
mod helpers;
use helpers::*;
use woven_common::ipc::{IpcCommand, IpcResponse};

use iced::{
    widget::{
        button, column, container, pick_list, row, rule,
        scrollable, text, text_editor, text_input, Space,
    },
    Alignment, Color, Element, Font, Length, Task, Theme,
};

// ── App ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
enum Tab { #[default] Daemon, Theme, Config }

#[derive(Debug, Clone, PartialEq, Eq)]
enum ColorField { Background, Accent, Text, Border }

#[derive(Debug, Clone)]
enum Msg {
    TabSelect(Tab),
    DaemonPoll(String, String, bool),
    DaemonShow, DaemonHide, DaemonToggle, DaemonReload,
    PresetPicked(String),
    ColorChanged { field: ColorField, value: String },
    OpacityChanged(String),
    RadiusChanged(String),
    ThemeApply,
    ThemeReset,
    ConfigAction(text_editor::Action),
    ConfigSave,
    ConfigReset,
    ConfigReload,
    Noop,
}

struct App {
    tab:            Tab,
    compositor:     String,
    daemon_ver:     String,
    daemon_on:      bool,
    daemon_vis:     bool,
    preset:         String,
    col_bg:         String,
    col_accent:     String,
    col_text:       String,
    col_border:     String,
    opacity:        String,
    radius:         String,
    config_content: text_editor::Content,
    config_dirty:   bool,
    status:         String,
}

impl Default for App {
    fn default() -> Self {
        let parsed = parse_theme_from_config();
        Self {
            tab:            Tab::Daemon,
            compositor:     "detecting\u{2026}".into(),
            daemon_ver:     "\u{2013}".into(),
            daemon_on:      false,
            daemon_vis:     false,
            preset:         parsed.preset,
            col_bg:         parsed.background,
            col_accent:     parsed.accent,
            col_text:       parsed.text,
            col_border:     parsed.border,
            opacity:        parsed.opacity,
            radius:         parsed.border_radius,
            config_content: text_editor::Content::with_text(&read_config()),
            config_dirty:   false,
            status:         String::new(),
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
                                            _                            => ("offline".into(), "-".into(), false),
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
            s.tab = t;
        }
        Msg::Noop => {}

        Msg::DaemonPoll(comp, ver, vis) => {
            s.daemon_on  = comp != "offline";
            s.compositor = comp;
            s.daemon_ver = ver;
            s.daemon_vis = vis;
        }
        Msg::DaemonShow   => { send_ipc(IpcCommand::Show);   s.daemon_vis = true;  s.status = "Overlay shown.".into(); }
        Msg::DaemonHide   => { send_ipc(IpcCommand::Hide);   s.daemon_vis = false; s.status = "Overlay hidden.".into(); }
        Msg::DaemonToggle => { send_ipc(IpcCommand::Toggle); s.status = "Toggled.".into(); }
        Msg::DaemonReload => { send_ipc(IpcCommand::ReloadConfig); s.status = "Reloaded.".into(); }

        Msg::PresetPicked(p) => {
            if p != "Custom" {
                let (bg, ac, txt, bd) = preset_colors(&p);
                s.col_bg = bg.to_string(); s.col_accent = ac.to_string();
                s.col_text = txt.to_string(); s.col_border = bd.to_string();
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
            let block = build_theme_block(
                &s.col_bg, &s.col_accent, &s.col_text, &s.col_border, radius, opacity,
            );
            let new_config = splice_theme_into_config(&read_config(), &block);
            match write_config(&new_config) {
                Ok(()) => {
                    s.config_content = text_editor::Content::with_text(&new_config);
                    s.config_dirty = false;
                    send_ipc(IpcCommand::ReloadConfig);
                    s.status = "Theme saved and applied.".into();
                }
                Err(e) => s.status = format!("Write failed: {}", e),
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

        Msg::ConfigAction(a) => { s.config_content.perform(a); s.config_dirty = true; }
        Msg::ConfigReload => {
            s.config_content = text_editor::Content::with_text(&read_config());
            s.config_dirty = false;
            s.status = "Reloaded from disk.".into();
        }
        Msg::ConfigSave => {
            match write_config(&s.config_content.text()) {
                Ok(()) => {
                    send_ipc(IpcCommand::ReloadConfig);
                    s.config_dirty = false;
                    s.status = format!("Saved to {}", config_path());
                }
                Err(e) => s.status = format!("Write failed: {}", e),
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
        tab_btn("Daemon", Tab::Daemon, &s.tab),
        tab_btn("Theme",  Tab::Theme,  &s.tab),
        tab_btn("Config", Tab::Config, &s.tab),
    ].spacing(4).padding([8u16, 12u16]);

    let body: Element<Msg> = match s.tab {
        Tab::Daemon => view_daemon(s),
        Tab::Theme  => view_theme(s),
        Tab::Config => view_config(s),
    };

    let dot = if s.daemon_on { "● " } else { "○ " };
    let status_bar = container(
        row![
            text(format!("{}daemon {}  |  {}", dot, s.daemon_ver, s.compositor)).size(11),
                               Space::new().width(Length::Fill),
                               text(&s.status).size(11),
        ].align_y(Alignment::Center).spacing(8),
    ).padding([5u16, 14u16]).width(Length::Fill);

    column![tab_bar, rule::horizontal(1), body, rule::horizontal(1), status_bar].into()
}

// ── Daemon tab ────────────────────────────────────────────────────────────────

fn view_daemon(s: &App) -> Element<'_, Msg> {
    let status_col: Color = if s.daemon_on {
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

    scrollable(column![
        text("woven daemon").size(22),
               row![
                   text(if s.daemon_on { "●" } else { "○" }).size(18).color(status_col),
               text(if s.daemon_on { "Running" } else { "Offline" }).size(14),
               ].spacing(8).align_y(Alignment::Center),
               daemon_info,

               rule::horizontal(1),
               text("Overlay controls").size(16),
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
               text("Keybind examples").size(16),
               text("Hyprland:").size(12),
               text("  bind = SUPER, grave, exec, woven-ctrl --toggle").size(12).font(Font::MONOSPACE),
               text("Niri:").size(12),
               text("  key \"Super+grave\" { spawn \"woven-ctrl\" \"--toggle\"; }").size(12).font(Font::MONOSPACE),
               text("Sway:").size(12),
               text("  bindsym Super+grave exec woven-ctrl --toggle").size(12).font(Font::MONOSPACE),
    ].spacing(14).padding([32u16, 32u16])).into()
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
                                text("Opacity (0-1)").width(120).size(13),
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tab_btn<'a>(label: &'a str, tab: Tab, current: &Tab) -> Element<'a, Msg> {
    let active = &tab == current;
    button(text(label).size(13))
    .on_press(Msg::TabSelect(tab))
    .padding([5u16, 16u16])
    .style(if active { button::primary } else { button::secondary })
    .into()
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

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> iced::Result {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--show"   => { send_ipc(IpcCommand::Show);         return Ok(()); }
            "--hide"   => { send_ipc(IpcCommand::Hide);         return Ok(()); }
            "--toggle" => { send_ipc(IpcCommand::Toggle);       return Ok(()); }
            "--reload" => { send_ipc(IpcCommand::ReloadConfig); return Ok(()); }
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
            _          => {}
        }
    }

    iced::application(App::init, update, view)
    .title(|_: &App| "woven-ctrl".to_string())
    .window(iced::window::Settings {
        size: iced::Size::new(860.0, 580.0),
            resizable: true,
            ..Default::default()
    })
    .theme(|_: &App| Theme::CatppuccinMocha)
    .run()
}
