-- woven plugin api: 1.0
-- plugins/date.lua
-- Compact date badge for the bar.
-- Adapts layout based on canvas dimensions (vertical vs horizontal bar).
--
-- Usage (in woven.lua):
--   require("plugins.date").setup()

local M = {}

function M.setup(opts)
    opts = opts or {}
    
    local accent_color = opts.accent_color or "#cba6f7"
    local text_color   = opts.text_color   or "#cdd6f4"

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
            local w = ctx.w or 40
            local h = ctx.h or 58

            if h >= 50 then
                -- Vertical bar layout (tall canvas)
                -- Dynamically center the content block within the canvas.
                local dot_r   = 3
                local abbr_sz = 9
                local num_sz  = 18
                local gap     = 3
                local block_h = dot_r*2 + gap + abbr_sz + gap + num_sz + gap + abbr_sz
                local top_y   = math.floor((h - block_h) / 2)
                local mid     = w / 2

                local cy = top_y + dot_r
                ctx.circle(mid, cy, dot_r, { color = accent_color, alpha = 0.7 })

                local abbr_y = cy + dot_r + gap
                ctx.text_centered(t.day_abbr, {
                    y = abbr_y, size = abbr_sz, color = text_color, alpha = 0.55,
                })

                local num_y = abbr_y + abbr_sz + gap
                ctx.text_centered(tostring(t.day), {
                    y = num_y, size = num_sz, color = accent_color, alpha = 0.95,
                })

                local mon_y = num_y + num_sz + gap
                ctx.text_centered(t.month_abbr, {
                    y = mon_y, size = abbr_sz, color = text_color, alpha = 0.55,
                })
            else
                -- Horizontal bar layout (wide canvas, short height)
                local label = t.day_abbr .. " " .. tostring(t.day) .. " " .. t.month_abbr

                ctx.circle(5, h/2, 3, { color = accent_color, alpha = 0.7 })

                ctx.text(label, {
                    x = 12, y = h/2 - 5,
                    size = 10, color = text_color, alpha = 0.75,
                })
            end
        end,
    })
end

return M
