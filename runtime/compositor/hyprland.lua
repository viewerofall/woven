-- compositor/hyprland.lua
-- Hyprland-specific API surface.
-- Auto-loaded by boot.lua when HYPRLAND_INSTANCE_SIGNATURE is set.
-- Exposed as woven.hypr.*
--
-- All shell calls go through woven.process.exec — io/os are stripped by the sandbox.

local M = {}

local function hyprctl(...)
    local out = woven.process.exec("hyprctl", { ... })
    return out ~= "" and out or nil
end

-- Send a hyprctl dispatch command.
-- e.g. woven.hypr.dispatch("togglefloating")
-- e.g. woven.hypr.dispatch("movetoworkspace", "3")
function M.dispatch(cmd, ...)
    local args = { "dispatch", cmd }
    for _, v in ipairs({ ... }) do args[#args + 1] = v end
    woven.process.exec("hyprctl", args)
end

-- Live-tweak a config value without reloading.
-- e.g. woven.hypr.keyword("general:gaps_in", 5)
function M.keyword(key, value)
    hyprctl("keyword", key, tostring(value))
end

-- Send a Hyprland desktop notification.
-- icon: 0=no icon, 1=warning, 2=info, 3=hint, 4=error
-- color: hex string like "0xc792ea"
function M.notify(msg, opts)
    opts = opts or {}
    local icon  = tostring(opts.icon  or 0)
    local ms    = tostring(opts.ms    or 3000)
    local color = opts.color or "0xc792ea"
    hyprctl("notify", icon, ms, color, msg)
end

-- Returns basic info about the currently focused window, or nil.
-- Parses enough of the JSON to be useful without a full JSON library.
function M.active_window()
    local out = hyprctl("-j", "activewindow")
    if not out then return nil end
    local function str_field(key)
        return out:match('"' .. key .. '":%s*"([^"]*)"')
    end
    local function bool_field(key)
        return out:match('"' .. key .. '":%s*(true)') ~= nil
    end
    local function num_field(key)
        local v = out:match('"' .. key .. '":%s*(%d+)')
        return v and tonumber(v)
    end
    return {
        class     = str_field("class"),
        title     = str_field("title"),
        workspace = num_field("workspace"),
        floating  = bool_field("floating"),
        pid       = num_field("pid"),
    }
end

-- Move the focused window to a workspace (number or name).
function M.move_to_workspace(target)
    M.dispatch("movetoworkspace", tostring(target))
end

-- Focus a workspace by number or name.
function M.focus_workspace(target)
    M.dispatch("workspace", tostring(target))
end

-- Toggle float on the focused window.
function M.toggle_float()
    M.dispatch("togglefloating")
end

-- Toggle fullscreen on the focused window.
function M.toggle_fullscreen()
    M.dispatch("fullscreen")
end

-- Pin the focused window so it shows on all workspaces.
function M.toggle_pin()
    M.dispatch("pin")
end

-- Focus the next or previous window in the current workspace.
-- dir: "next" (default) | "prev"
function M.cycle_window(dir)
    if dir == "prev" then
        M.dispatch("cyclenext", "prev")
    else
        M.dispatch("cyclenext")
    end
end

return M
