-- ui/animation.lua
-- Declares animation descriptors that Rust's animation engine executes.
-- Lua never interpolates values itself — it only declares what the
-- animation should be and Rust handles every frame.

local M = {
    _config = {
        overlay_open  = { curve = "ease_out_cubic", duration_ms = 180 },
        overlay_close = { curve = "ease_in_cubic",  duration_ms = 120 },
        scroll        = { curve = "ease_in_out_cubic", duration_ms = 200 },
        popout_open   = { curve = "ease_out_cubic", duration_ms = 140 },
        popout_close  = { curve = "ease_in_cubic",  duration_ms = 100 },
        workspace_switch = { curve = "spring", duration_ms = 250, tension = 0.3 },
    }
}

-- Valid curve names Rust understands
local VALID_CURVES = {
    linear            = true,
    ease_out_cubic    = true,
    ease_in_cubic     = true,
    ease_in_out_cubic = true,
    spring            = true,
}

-- Apply user animation overrides from woven.animations({}) in config
function M.apply(values)
    if type(values) ~= "table" then
        woven.log.error("animation.apply: expected table")
        return
    end
    for name, def in pairs(values) do
        if M._config[name] then
            -- validate curve name
            if def.curve and not VALID_CURVES[def.curve] then
                woven.log.warn("animation: unknown curve '" .. def.curve ..
                    "' for '" .. name .. "', keeping default")
                def.curve = M._config[name].curve
            end
            -- merge over existing def
            for k, v in pairs(def) do
                M._config[name][k] = v
            end
        else
            woven.log.warn("animation: unknown animation '" .. name .. "', skipping")
        end
    end
    woven.log.info("animation: config applied")
end

-- Returns a single animation def by name
function M.get(name)
    return M._config[name]
end

-- Returns full config table — Rust reads this once at startup
function M.all()
    local copy = {}
    for k, v in pairs(M._config) do
        local def = {}
        for dk, dv in pairs(v) do def[dk] = dv end
        copy[k] = def
    end
    return copy
end

return M
