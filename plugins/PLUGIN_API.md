# Woven Plugin API

Plugins are single Lua files that live in `~/.config/woven/plugins/`.
Load them in `~/.config/woven/woven.lua` with `require("plugins.name").setup({...})`.

Every plugin file should start with a version comment so woven can surface compatibility warnings:

```lua
-- woven plugin api: 1.0
```

---

## Minimal plugin skeleton

```lua
local M = {}

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "my-plugin",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "bottom",  -- "top"|"bottom"|"panel"|"overlay"
        height   = opts.height   or 40,        -- logical pixels
        interval = opts.interval or 5,         -- re-render every N seconds (0 = once)
        render   = function(ctx)
            ctx.text("hello", { x = 4, y = ctx.h / 2 + 4, size = 11,
                                 color = "#cdd6f4", alpha = 0.8 })
        end,
    })
end

return M
```

---

## Slots

| Slot | Location | Canvas size (approx) |
|---|---|---|
| `"top"` | Above workspace cards in the sidebar bar | 40 × widget height |
| `"bottom"` | Below workspace cards in the sidebar bar | 40 × widget height |
| `"panel"` | Expanded control-center panel | 272 × widget height |
| `"overlay"` | Bottom strip of the full-screen overlay | 252 × 50 |

Overlay slot widgets are laid out side-by-side horizontally; each gets an equal share (max 260px) centered in the strip.

---

## `handle.widget(opts)` — widget registration

| Field | Type | Default | Description |
|---|---|---|---|
| `slot` | string | `"bottom"` | Where the widget appears |
| `height` | integer | 40 | Canvas height in logical pixels |
| `interval` | integer | 5 | Re-render interval in seconds; `0` = re-render every daemon tick (~16 ms) |
| `onclick` | string | nil | Shell command to spawn when widget is clicked (overlay slot only) |
| `render` | function | **required** | Called every interval; receives `ctx` |

---

## `ctx` — draw context

All coordinates are canvas-local (0, 0 = top-left of the widget area).

### Properties

| Property | Type | Description |
|---|---|---|
| `ctx.w` | number | Canvas width in logical pixels |
| `ctx.h` | number | Canvas height in logical pixels (same as `ctx.height`) |
| `ctx.height` | number | Alias for `ctx.h` |

### Methods

#### `ctx.text(content, opts)`
Draw a line of text anchored at its baseline.

```lua
ctx.text("hello", {
    x     = 8,        -- horizontal position
    y     = 24,       -- baseline y
    size  = 13,       -- font size in px
    color = "#cdd6f4", -- hex color
    alpha = 0.85,     -- 0.0–1.0 opacity
})
```

#### `ctx.text_centered(content, opts)`
Same as `ctx.text` but horizontally centered in the canvas. `x` is ignored.

```lua
ctx.text_centered("good morning", { y = 28, size = 15, color = "#cba6f7", alpha = 0.88 })
```

#### `ctx.rect(x, y, w, h, opts)`
Draw a filled rounded rectangle.

```lua
ctx.rect(4, 8, 32, 4, {
    color  = "#cba6f7",
    alpha  = 0.70,
    radius = 2,        -- corner radius (default 4)
})
```

#### `ctx.circle(cx, cy, r, opts)`
Draw a filled circle.

```lua
ctx.circle(20, 20, 8, { color = "#a6e3a1", alpha = 0.6 })
```

#### `ctx.clear(opts)`
Fill the entire canvas with a solid color (useful as a background).

```lua
ctx.clear({ color = "#1e1e2e", alpha = 0.9 })
```

#### `ctx.app_icon(class, opts)`
Draw an application icon looked up by WM class name. Falls back to a colored circle + initial letter if no icon is found.

```lua
ctx.app_icon("firefox", {
    x    = -1,    -- -1 = auto-center horizontally
    y    = 5,     -- top of icon box
    size = 40,    -- icon box width/height
})
```

---

## `woven.*` — global APIs

### Time

```lua
local t = woven.now()
-- t.hour, t.min, t.sec    (integers)
-- t.day, t.month, t.year
-- t.day_abbr              ("Mon")
-- t.month_abbr            ("Jan")
-- t.unix_ts               (seconds since epoch — use for rate/delta calculations)
```

### System info

```lua
local s = woven.sys_info()
-- s.cpu_pct       (0–100)
-- s.mem_pct       (0–100)
-- s.mem_used_gb
-- s.mem_total_gb
```

### Process execution

```lua
-- Run a command synchronously; returns stdout as a string (or "" on error).
local output = woven.process.exec("playerctl", { "metadata", "--format", "{{title}}" })

-- Spawn a process detached (fire-and-forget).
woven.process.spawn("kitty", {})
```

### File I/O

```lua
-- Read a text file; returns string or nil on failure.
local content = woven.io.read("/sys/class/power_supply/BAT0/capacity")

-- Read up to N bytes (useful for binary/large files).
local bytes = woven.io.read_bytes("/path/to/file", 64)
```

### Logging

```lua
woven.log.info("message")
woven.log.warn("something odd")
woven.log.error("this is bad")
-- Output visible in: journalctl --user -u woven -f
```

### Event hooks

```lua
woven.on("workspace_focus", function(data)
    -- data.id  (workspace id)
end)

woven.on("window_open", function(data)
    -- data.class, data.title, data.id
end)

woven.on("window_close",   function(data) end)
woven.on("window_focus",   function(data) end)
```

### Error handler

```lua
woven.on_error(function(msg)
    woven.log.error("plugin error: " .. msg)
end)
```

### Audio (cava integration)

```lua
-- Start a cava subprocess with N bars; call once in setup().
woven.audio.start(16)

-- Get current bar levels (Vec<f32> in 0.0–1.0); call in render.
local bars = woven.audio.bars()  -- returns a Lua table of N floats
```

### Per-app accent colors

```lua
woven.rules({
    ["firefox"]  = "#f97316",
    ["kitty"]    = "#ff6c6b",
    ["obsidian"] = "#7c3aed",
})
```

---

## Tips

### Caching slow data between renders
Lua module-level locals persist between render ticks (the module is loaded once at startup).
Use them to cache previous values for rate calculations or to avoid re-fetching on every tick:

```lua
local cached_value = nil
local last_fetch   = 0

handle.widget({ interval = 2, render = function(ctx)
    local now = woven.now()
    if now.unix_ts - last_fetch >= 10 then
        cached_value = woven.process.exec("some-slow-command", {})
        last_fetch = now.unix_ts
    end
    -- use cached_value
end })
```

### Hot-reload
Save `~/.config/woven/woven.lua` and run:
```
woven-ctrl --reload
```
Plugin state resets on reload. Module-level locals are re-initialized.

### Debugging
Watch logs live while developing:
```
journalctl --user -u woven -f
```
Render errors print the plugin name and Lua error; they don't crash the daemon.

---

## Bundled plugins

| Plugin | Slot | Description |
|---|---|---|
| `date` | `top` | Day + date badge |
| `nowplaying` | `bottom` | Current media track via playerctl |
| `battery` | `top` | Battery level + charging status (**laptops only**) |
| `network` | `overlay` | Live download + upload rates |
| `cava` | `panel` | Mini spectrum visualizer |
| `greeting` | `overlay` | Time-of-day greeting |
| `launcher` | `overlay` | App icon button |
| `app_rules` | — | Per-app accent color overrides |
| `ws_logger` | — | Workspace/window event logger (example hooks) |
