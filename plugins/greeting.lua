-- woven plugin api: 1.0
-- plugins/greeting.lua
-- Time-of-day greeting, horizontally and vertically centered in the overlay strip.

local M = {}

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "greeting",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "overlay",
        height   = opts.height   or 56,
        interval = opts.interval or 60,
        render   = function(ctx)
            local t = woven.now()
            local greet

            if     t.hour < 5  then greet = "up late"
            elseif t.hour < 12 then greet = "good morning"
            elseif t.hour < 17 then greet = "good afternoon"
            elseif t.hour < 21 then greet = "good evening"
            else                     greet = "good night"
            end

            -- Vertically centered baseline in a 50px canvas (WIDGET_H-6).
            ctx.text_centered(greet, {
                y = 30,
                size = 15, color = "#cba6f7", alpha = 0.88,
            })
        end,
    })
end

return M
