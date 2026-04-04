-- woven plugin api: 1.0
-- plugins/launcher.lua
-- Icon button in the overlay strip.  Clicking it spawns the configured program.
--
-- Usage:
--   require("plugins.launcher").setup({
--       slot = "overlay", height = 56,
--       label = "kitty", cmd = "kitty",
--   })

local M = {}

function M.setup(opts)
    opts = opts or {}
    local cmd   = opts.cmd   or "kitty"
    local label = opts.label or cmd

    local handle = woven.plugin.register({
        name = "launcher-" .. label,
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot   or "overlay",
        height   = opts.height or 56,
        interval = 0,
        onclick  = cmd,
        render   = function(ctx)
            local h  = ctx.height   -- canvas height (50px in overlay strip)
            local sz = h - 10       -- icon size: 40px for h=50

            -- App icon auto-centered horizontally (x=-1), 5px top padding.
            ctx.app_icon(label, { x = -1, y = 5, size = sz })
        end,
    })
end

return M
