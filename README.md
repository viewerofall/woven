# woven

A Wayland workspace overview daemon. Press a key, see all your workspaces and windows at once, click to focus.

```
Super+` → overlay appears → click a window → overlay closes, window focused
```
## Install

### From source

```bash
curl -fsSL https://raw.githubusercontent.com/viewerofall/woven/main/get.sh | bash
```

`get.sh` downloads a compressed version of all the files neccessary for a prebuilt startup from the repo, uncompresses it and copies everything neccesary over to your system

---

## Supported compositors

| Compositor | Status |
|------------|--------|
| Hyprland   | ✅ Full support |
| Niri       | ⚠️  Performance issues |
| Sway       | ⚠️ Implemented, untested |

GNOME is not supported — it does not implement `wlr-layer-shell`.
KDE is coming in v2.5/3 and will not be meant for daily use due to the fact it does not use the same system as window managers

---



### Manual
## You can either download comp.tar.gz which is everything without the setup or manually do it
Download comp.tar.gz
- Extract it
- Copy woven.service to ~/.config/systemd/user
- Copy runtime and woven.lua to ~/.config/woven
- Run ```cargo build --release```
- Copy executables ```cp target/release/woven ~/.local/bin && cp target/release/woven-ctrl ~/.local/bin```
- 
```bash
git clone https://github.com/viewerofall/woven.git && cd woven
mkdir ~/.config/woven && cp -r runtime ~/.config/woven && cp woven.lua ~/.config/woven
cp woven.service ~/.config/systemd/user && systemctl daemon-reload && sudo systemctl --user enable woven.service
cargo build --release
cp target/release/woven ~/.local/bin && cp target/release/woven-ctrl ~/.local/bin
```

---

## First-time setup

On first launch, if no config exists, `woven` opens `woven-ctrl --setup` — a graphical wizard that handles compositor detection, color theme selection, and keybind instructions. No terminal interaction required.

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

### Sway ⚠️

```
exec woven
bindsym Super+grave exec woven-ctrl --toggle
```

---

## Usage

| Action | Result |
|--------|--------|
| Click a window card | Focus that window, close overlay |
| Hover a window card | Show action buttons |
| Right-click / any key | Close overlay |
| Scroll | Scroll through workspaces |

### Hover buttons

| Button | Action |
|--------|--------|
| ✕ | Close window |
| ⧉ | Toggle float |
| ⊞ | Toggle fullscreen |
| ⬡ | Toggle pin |

---

## Configuration

Config lives at `~/.config/woven/woven.lua`. Open `woven-ctrl` to edit theme and settings with a GUI, or use the built in editor included in woven-ctrl directly. Changes apply with the click of a button:

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

Built-in presets selectable in `woven-ctrl`: Catppuccin Mocha, Dracula, Nord, Tokyo Night, Gruvbox.

### Workspaces

```lua
woven.workspaces({
    show_empty = false,
    min_width  = 200,
    max_width  = 400,
})
```

### Settings

```lua
woven.settings({
    scroll_dir      = "horizontal",  -- or "vertical"
    overlay_opacity = 0.92,
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

## Architecture

```
woven (daemon)
├── woven-sys       main process — Lua VM, IPC server, compositor backend
├── woven-render    render thread — Wayland surface, tiny-skia painter
├── woven-common    shared types and IPC protocol
└── woven-ctrl      iced GUI + CLI control panel

Runtime:  /usr/local/share/woven/runtime/
Config:   ~/.config/woven/woven.lua
IPC:      /run/user/$UID/woven.sock
```

The Lua runtime handles config, theming, workspace layout, and animation declarations. Rust handles all rendering, input, and compositor communication.

---

## v2 plans

- Window thumbnails
- Lua plugin API
- River backend
- Better x11 to wayland support
- Look like niri's overlay with the pop out feature and add the bar ontop with everything.
- True popout features
