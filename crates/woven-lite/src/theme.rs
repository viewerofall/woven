//! Built-in theme palette for woven-lite.
//! No config files — themes are compiled in.

use woven_common::types::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTheme {
    Catppuccin,
    Gruvbox,
    Nord,
    TokyoNight,
    Dracula,
    OneDark,
}

impl BuiltinTheme {
    pub const ALL: &'static [BuiltinTheme] = &[
        BuiltinTheme::Catppuccin,
        BuiltinTheme::Gruvbox,
        BuiltinTheme::Nord,
        BuiltinTheme::TokyoNight,
        BuiltinTheme::Dracula,
        BuiltinTheme::OneDark,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            BuiltinTheme::Catppuccin => "Catppuccin",
            BuiltinTheme::Gruvbox   => "Gruvbox",
            BuiltinTheme::Nord      => "Nord",
            BuiltinTheme::TokyoNight => "Tokyo Night",
            BuiltinTheme::Dracula   => "Dracula",
            BuiltinTheme::OneDark   => "One Dark",
        }
    }

    pub fn to_theme(&self) -> Theme {
        match self {
            BuiltinTheme::Catppuccin => Theme {
                background:    "#1e1e2e".into(),
                border:        "#cba6f7".into(),
                text:          "#cdd6f4".into(),
                accent:        "#89b4fa".into(),
                border_radius: 12,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.92,
                blur:          true,
            },
            BuiltinTheme::Gruvbox => Theme {
                background:    "#282828".into(),
                border:        "#d79921".into(),
                text:          "#ebdbb2".into(),
                accent:        "#458588".into(),
                border_radius: 8,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.95,
                blur:          false,
            },
            BuiltinTheme::Nord => Theme {
                background:    "#2e3440".into(),
                border:        "#88c0d0".into(),
                text:          "#eceff4".into(),
                accent:        "#5e81ac".into(),
                border_radius: 10,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.93,
                blur:          true,
            },
            BuiltinTheme::TokyoNight => Theme {
                background:    "#1a1b26".into(),
                border:        "#7aa2f7".into(),
                text:          "#c0caf5".into(),
                accent:        "#bb9af7".into(),
                border_radius: 12,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.92,
                blur:          true,
            },
            BuiltinTheme::Dracula => Theme {
                background:    "#282a36".into(),
                border:        "#bd93f9".into(),
                text:          "#f8f8f2".into(),
                accent:        "#50fa7b".into(),
                border_radius: 10,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.94,
                blur:          true,
            },
            BuiltinTheme::OneDark => Theme {
                background:    "#282c34".into(),
                border:        "#61afef".into(),
                text:          "#abb2bf".into(),
                accent:        "#e06c75".into(),
                border_radius: 8,
                font:          "JetBrainsMono Nerd Font".into(),
                font_size:     13,
                opacity:       0.93,
                blur:          false,
            },
        }
    }
}

/// Runtime theme picker state — which theme is selected, whether panel is open.
pub struct ThemePicker {
    pub current: BuiltinTheme,
    pub open:    bool,
}

impl Default for ThemePicker {
    fn default() -> Self {
        Self { current: BuiltinTheme::Catppuccin, open: false }
    }
}

impl ThemePicker {
    pub fn theme(&self) -> Theme {
        self.current.to_theme()
    }

    pub fn cycle_next(&mut self) {
        let idx = BuiltinTheme::ALL
            .iter()
            .position(|t| *t == self.current)
            .unwrap_or(0);
        self.current = BuiltinTheme::ALL[(idx + 1) % BuiltinTheme::ALL.len()];
    }
}
