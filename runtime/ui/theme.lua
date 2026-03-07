-- ui/theme.lua
-- Holds the active theme table after woven.theme() is called from user config.
-- Rust reads these values once at startup to build its render state.
-- Lua uses them for any logic that needs to know visual properties
-- (e.g. deciding text color based on background luminance).

local M = {
    _active = {
        background    = "#1e1e2e",
        border        = "#cba6f7",
        text          = "#cdd6f4",
        accent        = "#89b4fa",
        border_radius = 12,
        font          = "JetBrainsMono Nerd Font",
        font_size     = 13,
        opacity       = 0.92,
        blur          = true,
    }
}

-- Called by woven.theme() shim in user config.
-- Merges user values over defaults so partial configs are safe.
-- rust_push is the original Rust-backed woven.theme() saved before the override.
function M.apply(values, rust_push)
if type(values) ~= "table" then
    woven.log.error("theme.apply: expected table, got " .. type(values))
    return
    end
    for k, v in pairs(values) do
        M._active[k] = v
        end
        -- push merged result directly to Rust — never through woven.theme
        -- to avoid infinite recursion since woven.theme now points to this function
        if rust_push then
            rust_push(M._active)
            end
            woven.log.info("theme: applied")
            end

            -- Returns a copy of the active theme so callers can't mutate it
            function M.get()
            local copy = {}
            for k, v in pairs(M._active) do
                copy[k] = v
                end
                return copy
                end

                -- Utility: given a hex color string like "#1e1e2e"
                -- returns perceived luminance 0.0 (dark) to 1.0 (light)
                -- useful for deciding whether to use light or dark text on a surface
                function M.luminance(hex)
                hex = hex:gsub("#", "")
                local r = tonumber(hex:sub(1,2), 16) / 255
                local g = tonumber(hex:sub(3,4), 16) / 255
                local b = tonumber(hex:sub(5,6), 16) / 255
                return 0.2126 * r + 0.7152 * g + 0.0722 * b
                end

                -- Utility: returns "dark" or "light" for a given hex background
                function M.contrast_mode(hex)
                return M.luminance(hex) > 0.5 and "dark" or "light"
                end

                return M
