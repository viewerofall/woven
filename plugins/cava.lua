-- woven plugin api: 1.0
-- plugins/cava.lua
-- Mini audio spectrum visualizer for the expanded control-center panel.
-- Reads live bar data from cava via woven.audio (Rust-managed background thread).
-- Requires: cava installed (pacman -S cava)
--
-- Usage (in woven.lua):
--   require("plugins.cava").setup()

local M = {}

-- Catppuccin Mocha gradient low→high
local COLORS = {
    "#89b4fa", "#89b4fa",   -- blue
    "#89dceb", "#89dceb",   -- sky
    "#94e2d5", "#94e2d5",   -- teal
    "#a6e3a1", "#a6e3a1",   -- green
    "#f9e2af", "#f9e2af",   -- yellow
    "#fab387", "#fab387",   -- peach
    "#f38ba8", "#f38ba8",   -- red
    "#cba6f7", "#cba6f7",   -- mauve
}

function M.setup(opts)
    opts = opts or {}
    local n_bars = opts.bars or 16

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
                local col = COLORS[i] or "#89b4fa"
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
