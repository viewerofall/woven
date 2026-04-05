-- plugins/nowplaying.lua
-- Current media track in the sidebar bar via playerctl.

local M = {}

local MAX = 5

local function trunc(s, n)
    if not s or s == "" then return "" end
    if #s <= n then return s end
    return s:sub(1, n - 1) .. "."
end

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "nowplaying",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "bottom",
        height   = opts.height   or 62,
        interval = opts.interval or 5,
        render   = function(ctx)
            -- Single exec call: status|artist|title
            local raw = woven.process.exec("playerctl", {
                "metadata", "--format",
                "{{status}}|{{artist}}|{{title}}"
            })

            local status, artist, title = "", "", ""
            if raw and raw ~= "" then
                local parts = {}
                for p in (raw .. "|"):gmatch("([^|]*)|") do
                    parts[#parts + 1] = p
                end
                status = parts[1] or ""
                artist = trunc(parts[2] or "", MAX)
                title  = trunc(parts[3] or "", MAX)
            end

            local playing  = (status == "Playing")
            local paused   = (status == "Paused")
            local note_col = playing and "#a6e3a1" or (paused and "#f9e2af" or "#585b70")
            local text_col = playing and "#cdd6f4" or "#585b70"
            local alpha    = playing and 0.9 or 0.4

            ctx.text("~", {
                x = 11, y = 14,
                size = 14, color = note_col, alpha = 0.95,
            })

            if status == "" then
                ctx.text("-", { x = 12, y = 34, size = 11, color = "#585b70", alpha = 0.35 })
                return
            end

            local line1 = artist ~= "" and artist or title
            local line2 = artist ~= "" and title  or ""

            ctx.text(line1, { x = 3, y = 32, size = 9, color = text_col, alpha = alpha })
            if line2 ~= "" then
                ctx.text(line2, { x = 3, y = 46, size = 9, color = text_col, alpha = alpha * 0.65 })
            end
        end,
    })
end

return M
