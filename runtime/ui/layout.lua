-- ui/layout.lua
-- Declares how the overlay is laid out. Rust reads these values
-- and executes the actual positioning — Lua only declares intent.

local M = {
    _settings = {
        -- overall overlay
        direction       = "horizontal",  -- "horizontal" | "vertical"
        padding         = 24,            -- px from screen edge
        gap             = 16,            -- px between workspace columns
        overlay_opacity = 0.92,

        -- workspace columns
        workspace = {
            min_width     = 200,
            max_width     = 400,
            header_height = 32,          -- workspace name bar
            show_empty    = false,
        },

        -- window thumbnails inside each column
        thumbnail = {
            height        = 120,
            gap           = 8,
            show_title    = true,
            show_class    = true,
            fallback_icon = true,        -- show app icon when no thumbnail available
        },

        -- popout panel (resource usage per workspace)
        popout = {
            enabled   = true,
            side      = "right",         -- "right" | "left" | "bottom"
            width     = 220,
            show_cpu  = true,
            show_mem  = true,
            show_gpu  = false,           -- gpu metrics need extra work, off by default
            bar_height = 8,
        },
    }
}

-- Apply user layout overrides from woven.workspaces({}) in config
function M.apply_workspace(values)
    if type(values) ~= "table" then return end
    local ws = M._settings.workspace
    if values.show_empty  ~= nil then ws.show_empty  = values.show_empty  end
    if values.min_width               then ws.min_width   = values.min_width   end
    if values.max_width               then ws.max_width   = values.max_width   end
    woven.log.info("layout: workspace settings applied")
end

function M.apply_popout(values)
    if type(values) ~= "table" then return end
    local p = M._settings.popout
    if values.popout      ~= nil  then p.enabled    = values.popout      end
    if values.popout_side         then p.side        = values.popout_side end
    if values.popout_width        then p.width       = values.popout_width end

    -- popout_metrics is a list like { "cpu", "memory", "gpu" }
    if type(values.popout_metrics) == "table" then
        p.show_cpu = false
        p.show_mem = false
        p.show_gpu = false
        for _, metric in ipairs(values.popout_metrics) do
            if metric == "cpu"    then p.show_cpu = true end
            if metric == "memory" then p.show_mem = true end
            if metric == "gpu"    then p.show_gpu = true end
        end
    end
    woven.log.info("layout: popout settings applied")
end

function M.apply_settings(values)
    if type(values) ~= "table" then return end
    if values.scroll_dir then
        M._settings.direction = values.scroll_dir == "horizontal"
            and "horizontal" or "vertical"
    end
    if values.overlay_opacity then
        M._settings.overlay_opacity = values.overlay_opacity
    end
end

function M.get()
    return M._settings
end

return M
