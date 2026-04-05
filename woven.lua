-- ~/.config/woven/woven.lua
-- Woven overlay daemon configuration.
-- Reload live with:  woven-ctrl --reload
-- Full restart with: woven-ctrl --hide && woven &

-- ── Theme ─────────────────────────────────────────────────────────────────────
-- All colors are hex strings (#rrggbb).
-- opacity: 0.0 = fully transparent, 1.0 = fully opaque.
-- font:    any name fontconfig can resolve (fc-list to see installed fonts).
--
-- Built-in presets (copy the block you want):
--
--   Catppuccin Mocha  background=#1e1e2e  accent=#cba6f7  text=#cdd6f4  border=#6c7086
--   Gruvbox           background=#282828  accent=#d79921  text=#ebdbb2  border=#504945
--   Nord              background=#2e3440  accent=#88c0d0  text=#eceff4  border=#4c566a
--   Tokyo Night       background=#1a1b26  accent=#7aa2f7  text=#c0caf5  border=#414868
--   Dracula           background=#282a36  accent=#bd93f9  text=#f8f8f2  border=#6272a4

woven.theme({
    background    = "#2e3440",
    border        = "#4c566a",
    text          = "#eceff4",
    accent        = "#88c0d0",
    border_radius = 10,
    font          = "JetBrainsMono Nerd Font",
    font_size     = 13,
    opacity       = 0.80,
})

-- ── Workspaces ────────────────────────────────────────────────────────────────
-- show_empty:  show workspaces that have no windows open
-- min_width:   minimum width of a workspace column in pixels
-- max_width:   maximum width of a workspace column in pixels

woven.workspaces({
    show_empty = false,
    min_width  = 200,
    max_width  = 400,
})

-- ── Settings ──────────────────────────────────────────────────────────────────
-- scroll_dir:      "horizontal" (left/right) or "vertical" (up/down)
-- overlay_opacity: separate from theme.opacity — controls the grid area only

woven.settings({
    scroll_dir      = "vertical",
    overlay_opacity = 0.92,
})

-- ── Animations ────────────────────────────────────────────────────────────────
-- Each animation takes:
--   curve:       "linear" | "ease_out_cubic" | "ease_in_cubic" |
--                "ease_in_out_cubic" | "spring"
--   duration_ms: milliseconds (0 = instant)
--
-- Available animations:
--   overlay_open      — overlay sliding/fading in
--   overlay_close     — overlay sliding/fading out
--   scroll            — scrolling between workspace pages
--   workspace_switch  — active workspace indicator moving

woven.animations({
    overlay_open  = { curve = "spring", duration_ms = 180 },
    overlay_close = { curve = "linear", duration_ms = 180 },
    scroll        = { curve = "ease_in_out_cubic", duration_ms = 180 },
})

-- ── Bar ───────────────────────────────────────────────────────────────────────
-- enabled:  show the persistent workspace bar (true / false)
-- position: "left" | "right" | "top" | "bottom"
--
-- The bar shows workspace screenshots, a toggle button for the overlay,
-- and a hide button.  Clicking a workspace thumbnail focuses it.

woven.bar({
    enabled  = false,
    position = "left",
})

-- ── AI workspace namer ───────────────────────────────────────────────────
woven.namer({ enabled = true })

-- ── Per-app accent colors ─────────────────────────────────────────────────────
-- Maps window class names (lowercase) to hex accent colors.
-- These override the auto-generated hash colors on workspace cards.
require("plugins.app_rules").setup({
    -- your custom overrides go here, e.g.:
    -- ["myapp"] = "#ff6c6b",
["kitty"] = "#ff6c6b", 
})

-- ── Bar widgets (narrow sidebar bar) ─────────────────────────────────────────
-- Date badge — above workspace cards
require("plugins.date").setup({ slot = "top", height = 58, interval = 60 })

-- Battery level — uncomment on laptops only
-- require("plugins.battery").setup({ slot = "top", height = 58, interval = 30 })

-- Now playing — below workspace cards, above sys info
require("plugins.nowplaying").setup({ slot = "bottom", height = 62, interval = 5 })

-- ── Panel widgets (expanded control center) ───────────────────────────────────
-- Mini cava spectrum visualizer — appears at bottom of expanded bar
require("plugins.cava").setup({ slot = "panel", height = 72, interval = 0 })

-- ── Overlay widgets (bottom strip of the overlay) ─────────────────────────────
-- Greeting message — center zone of overlay bottom strip
require("plugins.greeting").setup({ slot = "overlay", height = 56, interval = 60 })

-- Live network usage — download + upload rates
require("plugins.network").setup({ slot = "overlay", height = 56, interval = 2 })

-- Kitty launcher tile
require("plugins.launcher").setup({ slot = "overlay", height = 56, label = "kitty", cmd = "kitty" })


-- ── Event hooks & error handler ───────────────────────────────────────────────
-- Logs workspace/window events to journalctl and installs a custom error handler.
require("plugins.ws_logger").setup()

-- ── Raw hooks (optional) ─────────────────────────────────────────────────────
-- You can also register hooks directly here without a plugin module:
--
-- woven.on("workspace_focus", function(data)
--     woven.log.info("focused workspace: " .. tostring(data.id))
-- end)
--
-- woven.on("window_open", function(data)
--     woven.log.info("opened: " .. data.class .. " — " .. data.title)
-- end)
