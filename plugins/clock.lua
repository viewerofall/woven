-- plugins/clock.lua
-- A bar widget that shows the current time and date.
--
-- Usage (in woven.lua):
--   local clock = require("plugins.clock")
--   clock.setup()
--
-- Renders a two-line display:
--   14:32
--   Sun 29 Mar

local M = {}

function M.setup(opts)
    opts = opts or {}
    local slot     = opts.slot     or "top"
    local height   = opts.height   or 56
    local interval = opts.interval or 1   -- refresh every second

    local handle = woven.plugin.register({
        name = "clock",
        type = "bar_widget",
    })

    handle.widget({
        slot     = slot,
        height   = height,
        interval = interval,
        render   = function(ctx)
            -- clear background
            ctx:clear({ color = "#1e1e2e", alpha = 0.0 })

            local now    = os.date("*t")
            local time   = string.format("%02d:%02d", now.hour, now.min)
            local days   = { "Sun","Mon","Tue","Wed","Thu","Fri","Sat" }
            local months = { "Jan","Feb","Mar","Apr","May","Jun",
                             "Jul","Aug","Sep","Oct","Nov","Dec" }
            local date   = string.format("%s %d %s", days[now.wday],
                                         now.mday, months[now.month])

            local cx = 38.0
            local cy = ctx.height / 2

            -- clock icon circle (accent dot)
            ctx:circle(cx - 20, cy - 4, 3.5, { color = "#cba6f7", alpha = 0.8 })

            -- time (large)
            ctx:text(time, { x = cx, y = cy + 2, size = 15, color = "#cdd6f4", alpha = 1.0 })

            -- date (small, dimmer)
            ctx:text(date, { x = cx, y = cy + 16, size = 10, color = "#cdd6f4", alpha = 0.55 })
        end,
    })
end

return M
