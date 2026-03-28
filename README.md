# woven

A Wayland workspace overview daemon for tiling compositors. Press a key, see all your workspaces and windows at once, click to focus. Ships with a persistent sidebar bar that doubles as a control center.

```
Super+` → overlay appears → click a window → overlay closes, window focused
```

---

## What's in v2

- **Workspace overview** — live screencopy thumbnails of every window, zoom animation, responsive grid layout
- **Workspace strip** — horizontal strip of workspace cards at the top; click to preview, click again to switch
- **Persistent bar** — docked side/top/bottom bar showing active workspaces, clock, and system stats
- **Control center** — expand the bar to access media controls, CPU/GPU temps, and power menu
- **Multi-monitor** — one bar per connected output, automatic hotplug handling
- **River backend** — basic support alongside Hyprland, Niri, and Sway

---

## Supported compositors

| Compositor | Status |
|------------|--------|
| Hyprland   | ✅ Full support |
| Niri       | ✅ Full support |
| Sway       | ✅ Full support |
| River      | ⚠️ Basic support, untested |

GNOME is not supported — it does not implement `wlr-layer-shell`.
KDE support is planned for v2.5/v3.

---

## Install

### One-liner

```bash
curl -fsSL https://raw.githubusercontent.com/viewerofall/woven/main/get.sh | sh
```

`get.sh` downloads the latest release, extracts it, and copies everything to the right places automatically.

### From source

```bash
git clone https://github.com/viewerofall/woven.git
cd woven
cargo build --release
```

Then copy manually:

```bash
# Binaries
cp target/release/woven target/release/woven-ctrl ~/.local/bin/

# Config and runtime
mkdir -p ~/.config/woven
cp config/woven.lua ~/.config/woven/
cp -r runtime ~/.config/woven/

# Systemd user service
cp woven.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable woven.service
```

---

## First-time setup

On first launch, if no config exists, `woven` opens `woven-ctrl --setup` — a graphical wizard that handles compositor detection, color theme selection, and keybind instructions. No terminal required.

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

> River: woven maps River tags 1–9 to workspaces 1–9. Window titles are not available due to River CLI limitations — `wlr-foreign-toplevel` support is planned.

---

## Usage

### Overlay

| Action | Result |
|--------|--------|
| Click a window card | Focus that window, close overlay |
| Hover a window card | Show action buttons |
| Click a workspace card | Preview that workspace |
| Right-click / any key | Close overlay |
| Scroll | Page through workspaces |

### Hover buttons

| Button | Action |
|--------|--------|
| focus | Focus window |
| float | Toggle float |
| pin | Toggle pin |
| fs | Toggle fullscreen |
| ✕ | Close window |

### Bar

The bar shows active workspaces, clock, and quick stats collapsed (52px). Click `>` to expand into the control center (300px).

**Control center includes:**
- Clock and system stats (CPU, GPU, RAM, volume)
- Media player controls (requires `playerctl`)
- WiFi and Bluetooth toggles (requires `nmcli` / `bluetoothctl`)
- Power menu (suspend, reboot, shutdown, lock, logout)

---

## Configuration

Config lives at `~/.config/woven/woven.lua`. Edit through `woven-ctrl` or directly — reload with:

```bash
woven-ctrl --reload
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

Built-in presets: Catppuccin Mocha, Dracula, Nord, Tokyo Night, Gruvbox.

### Bar

```lua
woven.bar({
    enabled  = true,
    position = "right",   -- "left" | "right" | "top" | "bottom"
})
```

### Workspaces

```lua
woven.workspaces({
    show_empty = false,
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

## woven-ctrl

```
woven-ctrl              open the GUI control panel
woven-ctrl --toggle     toggle the overlay
woven-ctrl --show       show the overlay
woven-ctrl --hide       hide the overlay
woven-ctrl --reload     reload config from disk
woven-ctrl --setup      run the first-time setup wizard
```

---

## Dependencies

Optional runtime dependencies — bar degrades gracefully without them:

| Package | Used for |
|---------|----------|
| `playerctl` | Media controls |
| `nmcli` | WiFi toggle |
| `bluetoothctl` | Bluetooth toggle |

---

## Architecture

```
woven (daemon)
├── woven-sys        main process — Lua VM, IPC server, compositor backends
├── woven-render     render thread — Wayland surfaces, tiny-skia painter
├── woven-protocols  Wayland protocol extensions — screencopy
├── woven-common     shared types and IPC protocol
└── woven-ctrl       iced GUI + CLI control panel

Runtime:  ~/.config/woven/runtime/
Config:   ~/.config/woven/woven.lua
IPC:      /run/user/$UID/woven.sock
```

The Lua runtime handles config, theming, and animation declarations. Rust handles all rendering, input, and compositor communication.
