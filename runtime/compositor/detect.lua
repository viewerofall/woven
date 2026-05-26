-- compositor/detect.lua
-- Detects the running compositor from environment variables.
-- Loaded early by boot.lua; result exposed as woven.compositor_name / woven.is_compositor().

local M = {}

local function env_set(k)
    local v = os.getenv(k)
    return v ~= nil and v ~= ""
end

if     env_set("HYPRLAND_INSTANCE_SIGNATURE") then M.name = "hyprland"
elseif env_set("NIRI_SOCKET")                 then M.name = "niri"
elseif env_set("SWAYSOCK")                    then M.name = "sway"
elseif env_set("RIVER_SESSION")               then M.name = "river"
else                                               M.name = "unknown"
end

function M.is(name) return M.name == name end

return M
