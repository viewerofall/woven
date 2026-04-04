-- woven plugin api: 1.0
-- plugins/date.lua
-- Compact date badge for the sidebar bar.
-- Shows day-of-week, day number, and month — complementing the bar's existing clock.
--
-- Usage (in woven.lua):
--   require("plugins.date").setup()

local M = {}

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "date",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "top",
        height   = opts.height   or 58,
        interval = opts.interval or 60,
        render   = function(ctx)
            local t = woven.now()

            -- accent dot
            ctx.circle(7, 10, 3, { color = "#cba6f7", alpha = 0.7 })

            -- day abbreviation  e.g. "Mon"
            ctx.text(t.day_abbr, {
                x = 3, y = 12,
                size = 9, color = "#cdd6f4", alpha = 0.55,
            })

            -- date number large  e.g. "29"
            local d = tostring(t.day)
            ctx.text(d, {
                x = (#d == 1) and 13 or 7,
                y = 28,
                size = 18, color = "#cba6f7", alpha = 0.95,
            })

            -- month abbreviation  e.g. "Mar"
            ctx.text(t.month_abbr, {
                x = 3, y = 44,
                size = 9, color = "#cdd6f4", alpha = 0.55,
            })
        end,
    })
end

return M
