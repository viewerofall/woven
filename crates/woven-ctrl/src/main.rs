//! woven-ctrl — iced 0.14 GUI control panel for the woven daemon.

use iced::{
    widget::{
        button, column, container, pick_list,
        row, rule, scrollable, text, text_input, Space,
    },
    Alignment, Color, Element, Font, Length, Task, Theme,
};

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use woven_common::ipc::{IpcCommand, IpcResponse};
use woven_common::types::Theme as WovenTheme;

fn send_ipc(cmd: IpcCommand) -> Option<IpcResponse> {
    let mut stream = UnixStream::connect(woven_common::ipc::socket_path()).ok()?;
    let mut line   = serde_json::to_string(&cmd).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    let mut buf = String::new();
    BufReader::new(stream).read_line(&mut buf).ok()?;
    serde_json::from_str(buf.trim()).ok()
}

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Default)]
enum Tab { #[default] Setup, Theme, Config, Backends }

#[derive(Debug, Clone, PartialEq, Eq)]
enum ColorField { Background, Accent, Text, Border }

#[derive(Debug, Clone)]
enum Msg {
    TabSelect(Tab),
    SetupNext, SetupBack,
    KeybindChanged(String),
    PresetPicked(String),
    ColorChanged { field: ColorField, value: String },
    ThemeApply, ThemeReset,
    ConfigChanged(String), ConfigSave, ConfigReset,
    DaemonPoll(String, String),
    DaemonToggle,
    Noop,
}

const PRESETS: &[&str] = &[
    "Catppuccin Mocha", "Dracula", "Nord", "Tokyo Night", "Gruvbox",
];

#[derive(Default)]
struct App {
    tab:          Tab,
    setup_step:   usize,
    keybind:      String,
    compositor:   String,
    daemon_ver:   String,
    daemon_on:    bool,
    preset:       String,
    col_bg:       String,
    col_accent:   String,
    col_text:     String,
    col_border:   String,
    config_text:  String,
    config_dirty: bool,
    status:       String,
}

impl App {
    fn init() -> (Self, Task<Msg>) {
        let t = WovenTheme::default();
        let app = Self {
            keybind:     "SUPER, grave".into(),
            compositor:  "detecting…".into(),
            preset:      "Catppuccin Mocha".into(),
            col_bg:      t.background,
            col_accent:  t.accent,
            col_text:    t.text,
            col_border:  t.border,
            config_text: default_config(),
            ..Default::default()
        };
        let task = Task::perform(
            async {
                tokio::task::spawn_blocking(|| {
                    match send_ipc(IpcCommand::GetStatus) {
                        Some(IpcResponse::Status(s)) => (s.compositor, s.version),
                                            _ => ("offline".into(), "–".into()),
                    }
                }).await.unwrap_or(("offline".into(), "–".into()))
            },
            |(c, v)| Msg::DaemonPoll(c, v),
        );
        (app, task)
    }
}

// ── Update ────────────────────────────────────────────────────────────────────

fn update(s: &mut App, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::TabSelect(t)  => s.tab = t,
        Msg::DaemonToggle  => { send_ipc(IpcCommand::Toggle); s.status = "Toggled.".into(); }
        Msg::Noop          => {}

        Msg::DaemonPoll(comp, ver) => {
            s.daemon_on  = comp != "offline";
            s.compositor = comp;
            s.daemon_ver = ver;
        }

        Msg::SetupNext => {
            if s.setup_step < 2 { s.setup_step += 1; }
            else { s.tab = Tab::Theme; s.setup_step = 0; }
        }
        Msg::SetupBack => { if s.setup_step > 0 { s.setup_step -= 1; } }
        Msg::KeybindChanged(k) => s.keybind = k,
        Msg::PresetPicked(p)   => { apply_preset(s, &p); s.preset = p; }

        Msg::ColorChanged { field, value } => match field {
            ColorField::Background => s.col_bg     = value,
            ColorField::Accent     => s.col_accent = value,
            ColorField::Text       => s.col_text   = value,
            ColorField::Border     => s.col_border = value,
        },
        Msg::ThemeApply => {
            send_ipc(IpcCommand::ReloadConfig);
            s.status = "Theme applied.".into();
        }
        Msg::ThemeReset => {
            let d = WovenTheme::default();
            s.col_bg = d.background; s.col_accent = d.accent;
            s.col_text = d.text;     s.col_border = d.border;
            s.status = "Theme reset.".into();
        }

        Msg::ConfigChanged(t) => { s.config_text = t; s.config_dirty = true; }
        Msg::ConfigSave => {
            let path = config_path();
            if std::fs::write(&path, &s.config_text).is_ok() {
                send_ipc(IpcCommand::ReloadConfig);
                s.config_dirty = false;
                s.status = format!("Saved {}", path);
            } else {
                s.status = format!("Could not write {}", path);
            }
        }
        Msg::ConfigReset => { s.config_text = default_config(); s.config_dirty = true; }
    }
    Task::none()
}

// ── View ──────────────────────────────────────────────────────────────────────

fn view(s: &App) -> Element<'_, Msg> {
    let tab_bar = row![
        tab_btn("Setup",    Tab::Setup,    &s.tab),
        tab_btn("Theme",    Tab::Theme,    &s.tab),
        tab_btn("Config",   Tab::Config,   &s.tab),
        tab_btn("Backends", Tab::Backends, &s.tab),
    ].spacing(4).padding([8u16, 12]);

    let body: Element<Msg> = match s.tab {
        Tab::Setup    => view_setup(s),
        Tab::Theme    => view_theme(s),
        Tab::Config   => view_config(s),
        Tab::Backends => view_backends(s),
    };

    let dot = if s.daemon_on { "● " } else { "○ " };
    let status_label = format!("{}daemon {}  ·  {}", dot, s.daemon_ver, s.compositor);
    let status_bar = container(
        row![
            text(status_label).size(11),
                               Space::new().width(Length::Fill),
                               text(&s.status).size(11),
                               Space::new().width(Length::Fill),
                               button(text("Toggle").size(11))
                               .on_press(Msg::DaemonToggle)
                               .padding([3u16, 10]),
        ]
        .align_y(Alignment::Center)
        .spacing(8),
    )
    .padding([5u16, 14])
    .width(Length::Fill);

    column![
        tab_bar,
        rule::horizontal(1),
        body,
        rule::horizontal(1),
        status_bar,
    ]
    .into()
}

// ── Setup tab ─────────────────────────────────────────────────────────────────

fn view_setup(s: &App) -> Element<'_, Msg> {
    let steps = ["1. Detect", "2. Keybind", "3. Theme"];
    let crumbs = row(
        steps.iter().enumerate().map(|(i, &label)| {
            text(label).size(12)
            .color(if i == s.setup_step {
                Color::from_rgb(0.78, 0.65, 0.98)
            } else {
                Color::from_rgb(0.45, 0.45, 0.45)
            })
            .into()
        }).collect::<Vec<_>>()
    ).spacing(20);

    let body: Element<Msg> = match s.setup_step {
        0 => column![
            text("Compositor detection").size(20),
            text(format!("Detected: {}", s.compositor)).size(14),
            text(if s.daemon_on {
                "✓ woven daemon is running"
            } else {
                "✗ Daemon not found — make sure woven is running"
            }).size(13),
            text("Supported: Hyprland  ·  Niri and Sway coming soon").size(12),
        ].spacing(12).into(),

        1 => column![
            text("Toggle keybind").size(20),
            text("Keybind to add to your compositor config:").size(13),
            text_input("e.g. SUPER, grave", &s.keybind)
            .on_input(Msg::KeybindChanged).padding(8u16),
            text("Hyprland — paste into hyprland.conf:").size(12),
            text(format!("  bind = {}, exec, woven-ctrl --toggle", s.keybind))
            .size(12).font(Font::MONOSPACE),
        ].spacing(12).into(),

        _ => column![
            text("Pick a theme").size(20),
            pick_list(PRESETS, Some(s.preset.as_str()),
                      |p| Msg::PresetPicked(p.to_string())).width(220),
                      row![swatch(&s.col_bg), swatch(&s.col_accent),
                      swatch(&s.col_text), swatch(&s.col_border)].spacing(6),
                      text("Fine-tune colors in the Theme tab.").size(12),
        ].spacing(12).into(),
    };

    let nav = row![
        button("← Back").on_press(Msg::SetupBack).padding([6u16, 16]),
        Space::new().width(Length::Fill),
        button(if s.setup_step < 2 { "Next →" } else { "Finish →" })
        .on_press(Msg::SetupNext).padding([6u16, 16]),
    ];

    scrollable(
        column![crumbs, rule::horizontal(1), body, nav]
        .spacing(20).padding(32u16)
    ).into()
}

// ── Theme tab ─────────────────────────────────────────────────────────────────

fn view_theme(s: &App) -> Element<'_, Msg> {
    let col_row = |label: &'static str, val: String, field: ColorField| {
        row![
            text(label).width(110).size(13),
            text_input("#rrggbb", &val)
            .on_input(move |v| Msg::ColorChanged { field: field.clone(), value: v })
            .width(130).padding(6u16),
            swatch(&val),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };

    let editor = scrollable(column![
        text("Theme editor").size(20),
                            pick_list(PRESETS, Some(s.preset.as_str()),
                                      |p| Msg::PresetPicked(p.to_string())).width(220),
                            rule::horizontal(1),
                            col_row("Background", s.col_bg.clone(),     ColorField::Background),
                            col_row("Accent",     s.col_accent.clone(),  ColorField::Accent),
                            col_row("Text",       s.col_text.clone(),    ColorField::Text),
                            col_row("Border",     s.col_border.clone(),  ColorField::Border),
                            rule::horizontal(1),
                            row![
                                button("Apply").on_press(Msg::ThemeApply).padding([6u16, 16]),
                            button("Reset").on_press(Msg::ThemeReset).padding([6u16, 16]),
                            ].spacing(8),
    ].spacing(12).padding(28u16))
    .width(Length::FillPortion(1));

    let preview = column![
        text("Preview").size(14),
        preview_card(&s.col_bg, &s.col_accent, &s.col_border),
    ]
    .spacing(10).padding(28u16)
    .width(Length::FillPortion(1));

    row![editor, preview].into()
}

// ── Config tab ────────────────────────────────────────────────────────────────

fn view_config(s: &App) -> Element<'_, Msg> {
    scrollable(column![
        text("Config editor").size(20),
               text("Edit woven.lua — saved to ~/.config/woven/woven.lua").size(12),
               text_input("", &s.config_text)
               .on_input(Msg::ConfigChanged)
               .font(Font::MONOSPACE).padding(10u16),
               row![
                   button(if s.config_dirty { "Save *" } else { "Save" })
                   .on_press(Msg::ConfigSave).padding([6u16, 16]),
               button("Reset").on_press(Msg::ConfigReset).padding([6u16, 16]),
               ].spacing(8),
    ].spacing(14).padding(32u16)).into()
}

// ── Backends tab ──────────────────────────────────────────────────────────────

fn view_backends(s: &App) -> Element<'_, Msg> {
    let card = |name: &'static str, desc: &'static str| -> Element<'static, Msg> {
        container(row![
            column![
                text(name).size(14),
                  text(desc).size(11),
            ].spacing(3).width(Length::Fill),
                  button(text("Coming soon").size(12))
                  .on_press(Msg::Noop).padding([5u16, 12]),
        ]
        .align_y(Alignment::Center).spacing(12))
        .padding(14u16).width(Length::Fill)
        .into()
    };

    let _ = s;
    scrollable(column![
        text("Compositor backends").size(20),
               text("Drop a backend binary into ~/.config/woven/backends/ to enable it.").size(12),
               rule::horizontal(1),
               card("Hyprland",  "Dynamic tiling Wayland compositor"),
               card("Niri",      "Scrollable tiling compositor"),
               card("Sway",      "i3-compatible Wayland compositor"),
               card("KWin",      "KDE Plasma compositor"),
               rule::horizontal(1),
               text("Backend downloads not yet available — placeholder for future releases.").size(11),
    ].spacing(8).padding(32u16)).into()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tab_btn<'a>(label: &'a str, tab: Tab, current: &Tab) -> Element<'a, Msg> {
    let active = &tab == current;
    button(text(label).size(13))
    .on_press(Msg::TabSelect(tab))
    .padding([5u16, 16])
    .style(if active { button::primary } else { button::secondary })
    .into()
}

fn hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 { return Color::BLACK; }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32 / 255.0;
    Color::from_rgb(r, g, b)
}

fn swatch(hex: &str) -> Element<'static, Msg> {
    let c = hex_color(hex);
    container(text(""))
    .width(20).height(20)
    .style(move |_t: &Theme| container::Style {
        background: Some(iced::Background::Color(c)),
           border: iced::Border {
               radius: 4.0.into(),
           width: 1.0,
           color: Color::from_rgba(1.0, 1.0, 1.0, 0.15),
           },
           ..Default::default()
    })
    .into()
}

fn preview_card<'a>(bg: &str, accent: &str, border: &str) -> Element<'a, Msg> {
    let bg_c = hex_color(bg);
    let ac_c = hex_color(accent);
    let bd_c = hex_color(border);

    let win = |name: &'static str| -> Element<'static, Msg> {
        container(text(name).size(11).color(ac_c))
        .padding([4u16, 8]).width(Length::Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(iced::Background::Color(
                Color::from_rgba(1.0, 1.0, 1.0, 0.07)
            )),
            border: iced::Border {
                radius: 4.0.into(), width: 0.5,
               color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            },
            ..Default::default()
        })
        .into()
    };

    container(column![
        container(text("workspace 1").size(11).color(ac_c))
        .padding([5u16, 10]).width(Length::Fill),
              column![win("Firefox"), win("Alacritty")]
              .spacing(4).padding([0u16, 10]),
    ])
    .width(240)
    .style(move |_t: &Theme| container::Style {
        background: Some(iced::Background::Color(bg_c)),
           border: iced::Border { radius: 10.0.into(), width: 1.5, color: bd_c },
           ..Default::default()
    })
    .into()
}

fn apply_preset(s: &mut App, preset: &str) {
    let (bg, accent, txt, border) = match preset {
        "Catppuccin Mocha" => ("#1e1e2e", "#cba6f7", "#cdd6f4", "#6c7086"),
        "Dracula"          => ("#282a36", "#bd93f9", "#f8f8f2", "#6272a4"),
        "Nord"             => ("#2e3440", "#88c0d0", "#eceff4", "#4c566a"),
        "Tokyo Night"      => ("#1a1b26", "#7aa2f7", "#c0caf5", "#414868"),
        "Gruvbox"          => ("#282828", "#d79921", "#ebdbb2", "#504945"),
        _                  => return,
    };
    s.col_bg = bg.into(); s.col_accent = accent.into();
    s.col_text = txt.into(); s.col_border = border.into();
}

fn default_config() -> String {
    concat!(
        "-- woven config\n",
        "woven.theme.set({\n",
            "    background    = \"#1e1e2e\",\n",
            "    accent        = \"#cba6f7\",\n",
            "    text          = \"#cdd6f4\",\n",
            "    border        = \"#6c7086\",\n",
            "    border_radius = 12,\n",
            "    opacity       = 0.92,\n",
            "})\n",
    ).into()
}

fn config_path() -> String {
    format!("{}/.config/woven/woven.lua",
        std::env::var("HOME").unwrap_or(".".into()))
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--toggle") {
        send_ipc(IpcCommand::Toggle);
        return Ok(());
    }

    iced::application(App::init, update, view)
    .title(|_s: &App| String::from("woven-ctrl"))
    .window(iced::window::Settings {
        size:      iced::Size::new(800.0, 560.0),
            resizable: true,
            ..Default::default()
    })
    .theme(|_s: &App| Theme::CatppuccinMocha)
    .run()
}
