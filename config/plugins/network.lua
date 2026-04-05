-- plugins/network.lua
-- Live network usage (download + upload) in the overlay strip.
-- Reads /proc/net/dev and diffs on each render tick.
--
-- Usage:
--   require("plugins.network").setup({ slot = "overlay", height = 56, interval = 2 })

local M = {}

-- Module-level state persists between render ticks.
local prev_rx   = nil
local prev_tx   = nil
local prev_ts   = nil
local disp_rx   = "..."
local disp_tx   = "..."

local function fmt_bytes(bps)
    if bps < 0     then return "0 B/s" end
    if bps < 1024  then return string.format("%d B/s",   math.floor(bps)) end
    if bps < 1e6   then return string.format("%.1f KB/s", bps / 1024) end
    if bps < 1e9   then return string.format("%.1f MB/s", bps / 1048576) end
    return string.format("%.2f GB/s", bps / 1073741824)
end

local function read_net()
    local raw = woven.io.read("/proc/net/dev")
    if not raw then return nil, nil end
    local rx, tx = 0, 0
    for line in raw:gmatch("[^\n]+") do
        local iface = line:match("^%s*(%S+):")
        if iface and iface ~= "lo" then
            -- Fields after colon: rx_bytes rx_packets ... (9th = tx_bytes)
            local fields = {}
            for n in line:gsub("^[^:]+:", ""):gmatch("%d+") do
                fields[#fields + 1] = tonumber(n)
            end
            if fields[1] then rx = rx + fields[1] end
            if fields[9] then tx = tx + fields[9] end
        end
    end
    return rx, tx
end

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "network",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "overlay",
        height   = opts.height   or 56,
        interval = opts.interval or 2,
        render   = function(ctx)
            local now = woven.now()
            local ts  = now.unix_ts
            local rx, tx = read_net()

            if rx and tx and prev_rx and prev_ts then
                local dt = ts - prev_ts
                if dt > 0 then
                    local rx_rate = (rx - prev_rx) / dt
                    local tx_rate = (tx - prev_tx) / dt
                    disp_rx = fmt_bytes(rx_rate)
                    disp_tx = fmt_bytes(tx_rate)
                end
            end

            prev_rx = rx
            prev_tx = tx
            prev_ts = ts

            local h = ctx.h

            -- Down arrow + rate (green)
            ctx.text("↓", {
                x = 6, y = h / 2 - 2,
                size = 11, color = "#a6e3a1", alpha = 0.90,
            })
            ctx.text(disp_rx, {
                x = 20, y = h / 2 - 2,
                size = 10, color = "#cdd6f4", alpha = 0.80,
            })

            -- Up arrow + rate (blue-purple)
            ctx.text("↑", {
                x = 6, y = h / 2 + 13,
                size = 11, color = "#89b4fa", alpha = 0.90,
            })
            ctx.text(disp_tx, {
                x = 20, y = h / 2 + 13,
                size = 10, color = "#cdd6f4", alpha = 0.80,
            })
        end,
    })
end

return M
