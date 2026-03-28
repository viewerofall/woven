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
    background    = "#1e1e2e",   -- overlay background
    border        = "#6c7086",   -- window card borders
    text          = "#cdd6f4",   -- primary text
    accent        = "#cba6f7",   -- focused / hover highlight
    border_radius = 12,          -- card corner radius in pixels
    font          = "JetBrainsMono Nerd Font",
    font_size     = 13,
    opacity       = 0.92,        -- overall overlay alpha
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
    scroll_dir      = "horizontal",
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
    overlay_open  = { curve = "ease_out_cubic",    duration_ms = 180 },
    overlay_close = { curve = "ease_in_cubic",     duration_ms = 120 },
    scroll        = { curve = "ease_in_out_cubic", duration_ms = 200 },
})

-- ── Bar ───────────────────────────────────────────────────────────────────────
-- enabled:  show the persistent workspace bar (true / false)
-- position: "left" | "right" | "top" | "bottom"
--
-- The bar shows workspace screenshots, a toggle button for the overlay,
-- and a hide button.  Clicking a workspace thumbnail focuses it.

woven.bar({
    enabled  = true,
    position = "right",
})

-- ── Hooks (optional) ─────────────────────────────────────────────────────────
-- React to compositor events. Currently informational — use woven.log to debug.
--
-- Available events:
--   "workspace_focus"  — user switched to a different workspace
--   "window_open"      — a new window appeared
--   "window_close"     — a window was closed
--
-- Example:
-- woven.on("workspace_focus", function(ws)
--     woven.log.info("focused workspace: " .. tostring(ws.id))
-- end)
--
-- woven.on("window_open", function(win)
--     woven.log.info("opened: " .. win.class .. " — " .. win.title)
-- end)
