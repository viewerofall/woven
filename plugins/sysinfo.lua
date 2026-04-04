-- plugins/sysinfo.lua
-- A bar widget showing live CPU %, RAM usage, and uptime.
--
-- Usage (in woven.lua):
--   local sysinfo = require("plugins.sysinfo")
--   sysinfo.setup()
--
-- Renders a compact vertical stack:
--   CPU  ████░░  34%
--   RAM  ████████ 6.1G

local M = {}

-- Simple bar-graph using block characters.
local function bar_chars(pct, width)
    width = width or 6
    local filled = math.floor(pct / 100 * width + 0.5)
    filled = math.min(filled, width)
    local empty  = width - filled
    return string.rep("█", filled) .. string.rep("░", empty)
end

function M.setup(opts)
    opts = opts or {}
    local slot     = opts.slot     or "bottom"
    local height   = opts.height   or 64
    local interval = opts.interval or 3

    local handle = woven.plugin.register({
        name = "sysinfo",
        type = "bar_widget",
    })

    handle.widget({
        slot     = slot,
        height   = height,
        interval = interval,
        render   = function(ctx)
            ctx:clear({ color = "#1e1e2e", alpha = 0.0 })

            -- Pull metrics from the compositor API
            local metrics = woven.metrics.all() or {}
            local cpu_pct = metrics.cpu_pct or 0.0
            local mem_kb  = metrics.mem_used_kb or 0
            local mem_gb  = mem_kb / (1024 * 1024)

            local cy    = ctx.height / 2
            local lx    = 12.0   -- label x
            local bx    = 44.0   -- bar x
            local vx    = 90.0   -- value x
            local dim   = "#cdd6f4"
            local acc   = "#89b4fa"
            local warn  = "#f38ba8"

            -- CPU row
            local cpu_color = (cpu_pct > 80) and warn or acc
            ctx:text("CPU", { x = lx, y = cy - 6, size = 10, color = dim, alpha = 0.6 })
            ctx:text(bar_chars(cpu_pct),
                     { x = bx, y = cy - 6, size = 10, color = cpu_color, alpha = 0.85 })
            ctx:text(string.format("%3.0f%%", cpu_pct),
                     { x = vx, y = cy - 6, size = 10, color = cpu_color, alpha = 0.9 })

            -- RAM row
            local mem_pct   = metrics.mem_pct or 0.0
            local mem_color = (mem_pct > 80) and warn or "#a6e3a1"
            ctx:text("RAM", { x = lx, y = cy + 8, size = 10, color = dim, alpha = 0.6 })
            ctx:text(bar_chars(mem_pct),
                     { x = bx, y = cy + 8, size = 10, color = mem_color, alpha = 0.85 })
            ctx:text(string.format("%.1fG", mem_gb),
                     { x = vx, y = cy + 8, size = 10, color = mem_color, alpha = 0.9 })
        end,
    })
end

return M
