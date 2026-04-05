-- woven plugin api: 1.0
-- plugins/cava.lua
-- Mini audio spectrum visualizer for the expanded control-center panel.
-- Reads live bar data from cava via woven.audio (Rust-managed background thread).
-- Requires: cava installed (pacman -S cava)
--
-- Usage (in woven.lua):
--   require("plugins.cava").setup()

local M = {}

-- Theme color palettes
local THEMES = {
    catppuccin = {
        "#89b4fa", "#89b4fa",   -- blue
        "#89dceb", "#89dceb",   -- sky
        "#94e2d5", "#94e2d5",   -- teal
        "#a6e3a1", "#a6e3a1",   -- green
        "#f9e2af", "#f9e2af",   -- yellow
        "#fab387", "#fab387",   -- peach
        "#f38ba8", "#f38ba8",   -- red
        "#cba6f7", "#cba6f7",   -- mauve
    },
    gruvbox = {
        "#458588", "#458588",   -- blue
        "#83a598", "#83a598",   -- aqua
        "#8ec07c", "#8ec07c",   -- green
        "#b8bb26", "#b8bb26",   -- green bright
        "#fabd2f", "#fabd2f",   -- yellow
        "#fe8019", "#fe8019",   -- orange
        "#fb4934", "#fb4934",   -- red
        "#d3869b", "#d3869b",   -- purple
    },
    nord = {
        "#5e81ac", "#5e81ac",   -- frost blue
        "#81a1c1", "#81a1c1",   -- frost lighter
        "#88c0d0", "#88c0d0",   -- frost cyan
        "#8fbcbb", "#8fbcbb",   -- frost teal
        "#a3be8c", "#a3be8c",   -- aurora green
        "#ebcb8b", "#ebcb8b",   -- aurora yellow
        "#d08770", "#d08770",   -- aurora orange
        "#bf616a", "#bf616a",   -- aurora red
    },
    tokyo_night = {
        "#7aa2f7", "#7aa2f7",   -- blue
        "#7dcfff", "#7dcfff",   -- cyan
        "#73daca", "#73daca",   -- teal
        "#9ece6a", "#9ece6a",   -- green
        "#e0af68", "#e0af68",   -- yellow
        "#ff9e64", "#ff9e64",   -- orange
        "#f7768e", "#f7768e",   -- red
        "#bb9af7", "#bb9af7",   -- purple
    },
    dracula = {
        "#8be9fd", "#8be9fd",   -- cyan
        "#50fa7b", "#50fa7b",   -- green
        "#f1fa8c", "#f1fa8c",   -- yellow
        "#ffb86c", "#ffb86c",   -- orange
        "#ff79c6", "#ff79c6",   -- pink
        "#bd93f9", "#bd93f9",   -- purple
        "#ff5555", "#ff5555",   -- red
        "#6272a4", "#6272a4",   -- comment
    },
}

function M.setup(opts)
    opts = opts or {}
    local n_bars = opts.bars or 16
    local theme = opts.theme or "catppuccin"
    local colors = THEMES[theme] or THEMES.catppuccin

    -- Start the Rust cava reader (no-op if already running or cava missing).
    woven.audio.start(n_bars)

    local handle = woven.plugin.register({
        name = "cava",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "panel",
        height   = opts.height   or 72,
        interval = opts.interval or 0,   -- render every woven.sleep() tick
        render   = function(ctx)
            local bars  = woven.audio.bars()
            local h     = ctx.height
            local total = #bars
            if total == 0 then return end

            local canvas_w = 272   -- inner_w of the expanded bar (300 - 14*2)
            local bar_w    = math.floor((canvas_w - (total - 1) * 3) / total)
            local gap      = 3
            local max_h    = h - 6
            local base_y   = h - 2

            for i = 1, total do
                local v   = bars[i] or 0
                local bh  = math.max(2, v * max_h)
                local x   = (i - 1) * (bar_w + gap)
                local col = colors[i] or colors[1]
                ctx.rect(x, base_y - bh, bar_w, bh, {
                    color  = col,
                    alpha  = 0.55 + v * 0.45,   -- dim when quiet, bright when loud
                    radius = 2.0,
                })
            end
        end,
    })
end

return M
