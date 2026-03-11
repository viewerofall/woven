//! woven-ctrl first-time setup wizard.
//! Launched automatically by the woven daemon when no config exists.
//! Run manually with:  woven-ctrl --setup

use iced::{
    widget::{button, column, container, pick_list, row, rule, text, text_input, Space},
    Alignment, Color, Element, Length, Task, Theme,
};
use crate::helpers::*;
use woven_common::ipc::IpcCommand;

type El<'a> = Element<'a, Msg, iced::Theme, iced::Renderer>;

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Msg {
    Next,
    Back,
    PresetPicked(String),
    KeybindChanged(String),
    Done,
}

#[derive(Debug, Clone, PartialEq)]
enum Step { Welcome, Compositor, Theme, Keybind, Done }

pub struct Setup {
    step:        Step,
    compositor:  String,
    preset:      String,
    col_bg:      String,
    col_accent:  String,
    col_text:    String,
    col_border:  String,
    keybind:     String,
    status:      String,
}

impl Default for Setup {
    fn default() -> Self {
        // detect compositor from env
        let compositor = if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            "Hyprland"
        } else if std::env::var("NIRI_SOCKET").is_ok() {
            "Niri"
        } else if std::env::var("SWAYSOCK").is_ok() {
            "Sway"
        } else {
            "Unknown"
        }.to_string();

        let (bg, ac, txt, bd) = preset_colors("Catppuccin Mocha");
        Self {
            step:       Step::Welcome,
            compositor,
            preset:     "Catppuccin Mocha".to_string(),
            col_bg:     bg.to_string(),
            col_accent: ac.to_string(),
            col_text:   txt.to_string(),
            col_border: bd.to_string(),
            keybind:    "Super+grave".into(),
            status:     String::new(),
        }
    }
}

impl Setup {
    pub fn init() -> (Self, Task<Msg>) {
        (Self::default(), Task::none())
    }
}

// ── Update ────────────────────────────────────────────────────────────────────

pub fn update(s: &mut Setup, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::Next => {
            s.step = match s.step {
                Step::Welcome    => Step::Compositor,
                Step::Compositor => Step::Theme,
                Step::Theme      => Step::Keybind,
                Step::Keybind    => Step::Done,
                Step::Done       => Step::Done,
            };
            if s.step == Step::Done {
                write_first_config(s);
            }
        }
        Msg::Back => {
            s.step = match s.step {
                Step::Compositor => Step::Welcome,
                Step::Theme      => Step::Compositor,
                Step::Keybind    => Step::Theme,
                _                => s.step.clone(),
            };
        }
        Msg::PresetPicked(p) => {
            let (bg, ac, txt, bd) = preset_colors(&p);
            s.col_bg = bg.to_string(); s.col_accent = ac.to_string();
            s.col_text = txt.to_string(); s.col_border = bd.to_string();
            s.preset = p;
        }
        Msg::KeybindChanged(k) => s.keybind = k,
        Msg::Done => {
            // open main woven-ctrl window
            let _ = std::process::Command::new("woven-ctrl").spawn();
            std::process::exit(0);
        }
    }
    Task::none()
}

fn write_first_config(s: &Setup) {
    let opacity: f32 = 0.92;
    let radius:  u32 = 12;
    let theme_block = build_theme_block(
        &s.col_bg, &s.col_accent, &s.col_text, &s.col_border, radius, opacity,
    );
    let config = splice_theme_into_config(&default_config(), &theme_block);
    match write_config(&config) {
        Ok(()) => {
            // tell daemon to reload if it's running
            send_ipc(IpcCommand::ReloadConfig);
        }
        Err(e) => {
            eprintln!("setup: failed to write config: {}", e);
        }
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

pub fn view(s: &Setup) -> El<'_> {
    let steps = ["Welcome", "Compositor", "Theme", "Keybind", "Done"];
    let current = match s.step {
        Step::Welcome    => 0,
        Step::Compositor => 1,
        Step::Theme      => 2,
        Step::Keybind    => 3,
        Step::Done       => 4,
    };

    let breadcrumb = row(
        steps.iter().enumerate().map(|(i, &label)| {
            text(format!("{}{}",
                         if i < current { "✓ " } else if i == current { "→ " } else { "  " },
                             label
            ))
            .size(12)
            .color(if i == current {
                Color::from_rgb(0.78, 0.65, 0.98)
            } else if i < current {
                Color::from_rgb(0.5, 0.85, 0.5)
            } else {
                Color::from_rgb(0.4, 0.4, 0.4)
            })
            .into()
        }).collect::<Vec<El>>()
    ).spacing(20);

    let body: El = match s.step {
        Step::Welcome    => view_welcome(),
        Step::Compositor => view_compositor(s),
        Step::Theme      => view_theme(s),
        Step::Keybind    => view_keybind(s),
        Step::Done       => view_done(s),
    };

    // Every .into() must be a typed let — never inline inside row!/column!
    let back_btn: El = if s.step != Step::Welcome {
        button("← Back").on_press(Msg::Back).padding([6u16, 16u16]).into()
    } else {
        Space::new().into()
    };
    let next_label = if s.step == Step::Keybind { "Finish →" } else { "Next →" };
    let next_btn: El = button(next_label)
    .on_press(Msg::Next)
    .padding([6u16, 18u16])
    .style(button::primary)
    .into();
    let open_btn: El = button("Open woven-ctrl →")
    .on_press(Msg::Done)
    .padding([8u16, 20u16])
    .style(button::primary)
    .into();
    let fill: El = Space::new().width(Length::Fill).into();
    let fill2: El = Space::new().width(Length::Fill).into();
    let nav: El = if s.step != Step::Done {
        row![back_btn, fill, next_btn]
        .spacing(8).align_y(Alignment::Center).into()
    } else {
        row![fill2, open_btn].into()
    };

    container(column![
        breadcrumb,
        rule::horizontal(1),
              body,
              rule::horizontal(1),
              nav,
    ].spacing(20).padding([28u16, 36u16]))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_welcome<'a>() -> El<'a> {
    column![
        text("Welcome to woven").size(28),
        text("A Wayland workspace overview for Hyprland, Niri, and Sway.").size(14),
        rule::horizontal(1),
        text("This wizard will:").size(13),
        text("  1. Confirm your compositor is supported").size(12),
        text("  2. Let you pick a color theme").size(12),
        text("  3. Show you how to set your toggle keybind").size(12),
        text("  4. Write your config to ~/.config/woven/woven.lua").size(12),
        rule::horizontal(1),
        text("You can change everything later in woven-ctrl or by editing the config directly.").size(11),
    ].spacing(10).into()
}

fn view_compositor<'a>(s: &'a Setup) -> El<'a> {
    let (icon, status, note) = match s.compositor.as_str() {
        "Hyprland" => ("✓", "Hyprland detected", "Full support — workspaces, focus, close, float, pin, fullscreen."),
        "Niri"     => ("✓", "Niri detected",     "Full support — workspaces, focus, close. Pin is a no-op on Niri."),
        "Sway"     => ("✓", "Sway detected",     "Full support — workspaces, focus, close, float."),
        _          => ("✗", "Compositor not detected",
                       "woven supports Hyprland, Niri, and Sway. Make sure woven is launched from your compositor session."),
    };
    let col = if s.compositor == "Unknown" {
        Color::from_rgb(0.9, 0.4, 0.4)
    } else {
        Color::from_rgb(0.4, 0.85, 0.5)
    };

    column![
        text("Compositor").size(22),
        row![
            text(icon).size(20).color(col),
            text(status).size(16).color(col),
        ].spacing(8).align_y(Alignment::Center),
        text(note).size(12),
        rule::horizontal(1),
        text("Detected from environment variables:").size(11),
        text("  HYPRLAND_INSTANCE_SIGNATURE / NIRI_SOCKET / SWAYSOCK").size(11),
    ].spacing(12).into()
}

fn view_theme<'a>(s: &'a Setup) -> El<'a> {
    let swatch = |hex: &str| -> El<'static> {
        let h = hex.trim_start_matches('#');
        if h.len() < 6 { return Space::new().into(); }
        let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32 / 255.0;
        let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32 / 255.0;
        let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32 / 255.0;
        let c = iced::Color::from_rgb(r, g, b);
        container(text("")).width(22).height(22)
        .style(move |_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(c)),
               border: iced::Border { radius: 4.0.into(), width: 1.0,
                   color: iced::Color::from_rgba(1.0,1.0,1.0,0.2) },
               ..Default::default()
        }).into()
    };

    // only show non-Custom presets in setup
    let setup_presets: &[&str] = &["Catppuccin Mocha","Dracula","Nord","Tokyo Night","Gruvbox"];

    column![
        text("Color theme").size(22),
        text("Pick a preset — you can fine-tune colors in woven-ctrl later.").size(12),
        rule::horizontal(1),
        pick_list(setup_presets, Some(s.preset.as_str()),
                  |p| Msg::PresetPicked(p.to_string())).width(220),
                  row![
                      swatch(&s.col_bg),
                      swatch(&s.col_accent),
                      swatch(&s.col_text),
                      swatch(&s.col_border),
                      text(format!("bg  {}  accent  {}  text  {}  border  {}",
                                   s.col_bg, s.col_accent, s.col_text, s.col_border)).size(11),
                  ].spacing(8).align_y(Alignment::Center),
    ].spacing(12).into()
}

fn view_keybind<'a>(s: &'a Setup) -> El<'a> {
    let snippet = match s.compositor.as_str() {
        "Hyprland" => format!("bind = SUPER, grave, exec, woven-ctrl --toggle\n\n# Or with your custom key:\nbind = {}, exec, woven-ctrl --toggle", s.keybind),
        "Niri"     => format!("key \"Super+grave\" {{ spawn \"woven-ctrl\" \"--toggle\"; }}\n\n# Or with your custom key:\nkey \"{}\" {{ spawn \"woven-ctrl\" \"--toggle\"; }}", s.keybind),
        "Sway"     => format!("bindsym Super+grave exec woven-ctrl --toggle\n\n# Or with your custom key:\nbindsym {} exec woven-ctrl --toggle", s.keybind),
        _          => "Add a keybind in your compositor config to run:\n  woven-ctrl --toggle".into(),
    };

    let snippet_owned = snippet.clone();
    column![
        text("Toggle keybind").size(22),
        text("woven is toggled by running woven-ctrl --toggle from a keybind.").size(12),
        text("The default is Super+grave (the backtick key).").size(12),
        rule::horizontal(1),
        text("Custom keybind (optional):").size(13),
        text_input("e.g. Super+grave", &s.keybind)
        .on_input(Msg::KeybindChanged)
        .width(240).padding(6u16),
        rule::horizontal(1),
        text("Add this to your compositor config:").size(13),
        container(
            text(snippet_owned).size(11).font(iced::Font::MONOSPACE)
        ).padding([10u16, 14u16]).width(Length::Fill),
        text("woven must be running as a daemon before the keybind works.").size(11),
        text("Add `woven &` to your compositor autostart.").size(11),
    ].spacing(12).into()
}

fn view_done<'a>(_s: &'a Setup) -> El<'a> {
    let cfg   = config_path();
    let wrote = config_exists();
    let body: El = if wrote {
        column![
            text(format!("Config written to:  {}", cfg)).size(12),
            text("").size(8),
            text("Next steps:").size(14),
            text("  1. Make sure woven is in your compositor autostart").size(12),
            text("  2. Add the toggle keybind shown in the previous step").size(12),
            text("  3. Run woven-ctrl to adjust theme and settings anytime").size(12),
        ].spacing(6).into()
    } else {
        column![
            text(format!("Tried to write: {}", cfg)).size(12),
            text("Check that the directory is writable and try again.").size(12),
        ].spacing(6).into()
    };
    column![
        text(if wrote { "Setup complete!" } else { "Setup failed — could not write config." }).size(24),
        rule::horizontal(1),
        body,
    ].spacing(14).into()
}
