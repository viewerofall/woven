-- plugins/uptime.lua
-- Shows system hostname and uptime in the overlay widget strip.
--
-- Usage (in woven.lua):
--   require("plugins.uptime").setup()

local M = {}

local function fmt_uptime(secs)
    secs = math.floor(tonumber(secs) or 0)
    local h = math.floor(secs / 3600)
    local m = math.floor((secs % 3600) / 60)
    if h > 0 then
        return string.format("%dh %dm", h, m)
    else
        return string.format("%dm", m)
    end
end

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "uptime",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "overlay",
        height   = opts.height   or 40,
        interval = opts.interval or 60,
        render   = function(ctx)
            local host = woven.process.exec("hostname", {})
            local up_raw = woven.io.read("/proc/uptime")
            local secs = up_raw and up_raw:match("^(%S+)") or "0"
            local up = fmt_uptime(secs)

            ctx.circle(8, ctx.height / 2, 3, { color = "#a6e3a1", alpha = 0.7 })
            ctx.text(host, {
                x = 16, y = ctx.height / 2 + 4,
                size = 11, color = "#cdd6f4", alpha = 0.8,
            })
            ctx.text("up " .. up, {
                x = 16, y = ctx.height / 2 + 16,
                size = 9, color = "#cdd6f4", alpha = 0.45,
            })
        end,
    })
end

return M
