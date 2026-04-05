# woven

A Wayland workspace overview and control center for tiling compositors. Press a key, see everything, click to focus. Ships with a persistent bar, a full plugin system, and a graphical control panel.

```
Super+` → overlay appears → click a window → overlay closes, window focused
```

---

## Features

- **Workspace overview** — live screencopy thumbnails of every window, animated grid layout
- **Workspace strip** — horizontal strip at the top; click to preview, click again to switch
- **Persistent bar** — docked sidebar showing workspaces, clock, date, now playing, and system stats
- **Control center** — expand the bar for media controls, CPU/GPU temps, volume, power menu, and a cava audio visualizer
- **Plugin system** — Lua-based plugins for bar widgets, panel widgets, and overlay widgets
- **Plugin settings** — configure date colors, cava color themes, per-app accent colors, and launcher apps from the GUI
- **Theme engine** — 5 built-in presets (Catppuccin Mocha, Dracula, Nord, Tokyo Night, Gruvbox) or fully custom
- **AI workspace namer** — automatic workspace names based on open windows
- **Multi-monitor** — one bar per connected output, automatic hotplug handling
- **woven-ctrl** — graphical control panel with tabs for status, bar, theme, overview, plugins, and raw config editing
- **First-time setup wizard** — compositor detection, theme selection, keybind injection
- **Self-update** — `woven-ctrl --update` pulls the latest release and restarts

---

## Supported compositors

| Compositor | Status |
|------------|--------|
| Hyprland   | Full support |
| Niri       | Full support |
| Sway       | Full support |
| River      | Basic support |

GNOME is not supported (no `wlr-layer-shell`). KDE support is planned.

---

## Install

### One-liner

```bash
curl -fsSL https://raw.githubusercontent.com/viewerofall/woven/main/get.sh | sh
```

### From source

```bash
git clone https://github.com/viewerofall/woven.git
cd woven
cargo build --release
```

Then copy manually:

```bash
cp target/release/woven target/release/woven-ctrl ~/.local/bin/
mkdir -p ~/.config/woven
cp config/woven.lua ~/.config/woven/
cp -r runtime ~/.config/woven/
cp -r plugins ~/.config/woven/
cp woven.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable woven.service
```

---

## First-time setup

On first launch with no config, `woven` opens `woven-ctrl --setup` — a graphical wizard that handles compositor detection, color theme, and keybind injection. No terminal required.

---

## Compositor setup

### Hyprland

```ini
exec-once = woven
bind = SUPER, grave, exec, woven-ctrl --toggle
```

### Niri

```kdl
spawn-at-startup "woven"

binds {
    Super+Grave { spawn "woven-ctrl" "--toggle"; }
}
```

### Sway

```
exec woven
bindsym Super+grave exec woven-ctrl --toggle
```

### River

```sh
riverctl spawn woven
riverctl map normal Super grave spawn 'woven-ctrl --toggle'
```

---

## Usage

### Overlay

| Action | Result |
|--------|--------|
| Click a window card | Focus that window, close overlay |
| Hover a window card | Show action buttons (focus, float, pin, fullscreen, close) |
| Click a workspace card | Preview that workspace |
| Right-click / any key | Close overlay |
| Scroll | Page through workspaces |

### Bar

Collapsed (52px): workspace dots, clock, date, expand button. Click to expand into the full control center (300px) with media controls, system stats, cava visualizer, WiFi/BT toggles, and power menu.

---

## Configuration

Config lives at `~/.config/woven/woven.lua`. Edit through `woven-ctrl` or directly.

```bash
woven-ctrl --reload   # reload config live
```

### Theme

```lua
woven.theme({
    background    = "#1e1e2e",
    border        = "#6c7086",
    text          = "#cdd6f4",
    accent        = "#cba6f7",
    border_radius = 12,
    font          = "JetBrainsMono Nerd Font",
    font_size     = 13,
    opacity       = 0.92,
})
```

### Bar

```lua
woven.bar({
    enabled  = true,
    position = "right",   -- "left" | "right" | "top" | "bottom"
})
```

### Animations

```lua
woven.animations({
    overlay_open  = { curve = "ease_out_cubic",    duration_ms = 180 },
    overlay_close = { curve = "ease_in_cubic",     duration_ms = 120 },
    scroll        = { curve = "ease_in_out_cubic", duration_ms = 200 },
})
```

Curves: `linear` `ease_out_cubic` `ease_in_cubic` `ease_in_out_cubic` `spring`

---

## Plugins

Plugins are Lua scripts in `~/.config/woven/plugins/`. Enable them in `woven.lua`:

```lua
require("plugins.date").setup({ slot = "top", height = 58 })
require("plugins.cava").setup({ slot = "panel", theme = "catppuccin" })
require("plugins.app_rules").setup({ ["kitty"] = "#89b4fa" })
require("plugins.nowplaying").setup({ slot = "bottom", height = 62 })
require("plugins.greeting").setup({ slot = "overlay" })
require("plugins.network").setup({ slot = "overlay", interval = 2 })
require("plugins.launcher").setup({ slot = "overlay", label = "kitty", cmd = "kitty" })
```

### Included plugins

| Plugin | Slot | Description |
|--------|------|-------------|
| `date` | bar | Compact date badge with configurable accent/text colors |
| `clock` | bar | Simple clock widget |
| `battery` | bar | Battery level indicator |
| `nowplaying` | bar | Media now playing (requires playerctl) |
| `cava` | panel | Audio spectrum visualizer with 5 color themes |
| `app_rules` | - | Per-app accent color overrides |
| `greeting` | overlay | Greeting message |
| `network` | overlay | Live network usage (download/upload rates) |
| `launcher` | overlay | App launcher tile |
| `sysinfo` | overlay | System info widget |
| `uptime` | overlay | System uptime |
| `ws_logger` | - | Workspace/window event logger |

### Plugin settings

The Plugins tab in `woven-ctrl` lets you install, enable, disable, and configure plugins. Configurable plugins get a Settings button that opens a modal:

- **date** — accent and text colors
- **cava** — color theme (catppuccin, gruvbox, nord, tokyo_night, dracula)
- **app_rules** — add/remove/edit class-to-color mappings
- **launcher** — pick from installed apps

---

## woven-ctrl

```
woven-ctrl              open the GUI control panel
woven-ctrl --toggle     toggle the overlay
woven-ctrl --show       show the overlay
woven-ctrl --hide       hide the overlay
woven-ctrl --reload     reload config (full daemon restart)
woven-ctrl --setup      run the first-time setup wizard
woven-ctrl --update     self-update to the latest release
```

---

## Dependencies

Optional runtime dependencies — the bar and overlay degrade gracefully without them:

| Package | Used for |
|---------|----------|
| `playerctl` | Media controls / now playing |
| `nmcli` | WiFi toggle |
| `bluetoothctl` | Bluetooth toggle |
| `cava` | Audio spectrum visualizer plugin |

---

## Architecture

```
woven (workspace)
├── woven-sys        main daemon — Lua VM, IPC server, compositor backends, audio
├── woven-render     render thread — Wayland surfaces, tiny-skia/wgpu painter
├── woven-protocols  Wayland protocol extensions — screencopy, layer-shell
├── woven-common     shared types and IPC protocol
├── woven-plugin     plugin system crate
└── woven-ctrl       iced GUI control panel + CLI

Runtime:  ~/.config/woven/runtime/
Plugins:  ~/.config/woven/plugins/
Config:   ~/.config/woven/woven.lua
IPC:      /run/user/$UID/woven.sock
Service:  ~/.config/systemd/user/woven.service
```

---

## License

MIT
