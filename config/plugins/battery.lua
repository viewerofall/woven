-- plugins/battery.lua
-- Battery level + charging status in the sidebar bar (Top slot).
--
-- Usage (add to woven.lua):
--   require("plugins.battery").setup({ slot = "top", height = 58 })
--
-- Reads /sys/class/power_supply/BAT0 by default.
-- Change opts.bat to "BAT1" etc. if needed.

local M = {}

local function read(path)
    local s = woven.io.read(path)
    if not s or s == "" then return nil end
    return s:match("^%s*(.-)%s*$")  -- trim
end

function M.setup(opts)
    opts = opts or {}
    local bat  = opts.bat or "BAT0"
    local base = "/sys/class/power_supply/" .. bat

    local handle = woven.plugin.register({
        name = "battery",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "top",
        height   = opts.height   or 58,
        interval = opts.interval or 30,
        render   = function(ctx)
            local pct_s  = read(base .. "/capacity")
            local status = read(base .. "/status")

            if not pct_s then
                -- No battery found
                ctx.text_centered("no bat", { y = ctx.h / 2 + 4, size = 9,
                    color = "#585b70", alpha = 0.5 })
                return
            end

            local pct = tonumber(pct_s) or 0
            local charging = (status == "Charging")
            local full     = (status == "Full")

            -- Color: blue when charging/full, green >50%, yellow 20-50%, red <20%
            local col
            if charging or full then
                col = "#89b4fa"
            elseif pct > 50 then
                col = "#a6e3a1"
            elseif pct > 20 then
                col = "#f9e2af"
            else
                col = "#f38ba8"
            end

            local w = ctx.w   -- 40px

            -- Battery body outline (rounded rect)
            local bx = 6; local by = 8
            local bw = w - 12; local bh = 20
            ctx.rect(bx, by, bw, bh, { color = col, alpha = 0.18, radius = 3 })
            -- Nub on right
            ctx.rect(bx + bw, by + 6, 3, 8, { color = col, alpha = 0.30, radius = 1 })
            -- Fill bar (clamp to bw - 2 inner)
            local fill_w = math.max(2, math.floor((pct / 100) * (bw - 4)))
            ctx.rect(bx + 2, by + 2, fill_w, bh - 4, { color = col, alpha = 0.75, radius = 2 })

            -- Percentage label
            local lbl = (charging and "+" or "") .. pct .. "%"
            ctx.text_centered(lbl, { y = by + bh + 12, size = 10, color = col, alpha = 0.9 })

            -- "FULL" or status hint at bottom
            if full then
                ctx.text_centered("full", { y = by + bh + 24, size = 8,
                    color = "#89b4fa", alpha = 0.5 })
            end
        end,
    })
end

return M
