-- compositor/sway.lua
-- Sway-specific API surface.
-- Auto-loaded by boot.lua when SWAYSOCK is set.
-- Exposed as woven.sway.*
--
-- All shell calls go through woven.process.exec — io/os are stripped by the sandbox.

local M = {}

-- Send a raw swaymsg command and return stdout.
-- e.g. woven.sway.msg("workspace 3")
-- e.g. woven.sway.msg("-t", "get_workspaces")
function M.msg(...)
    local args = { ... }
    local out = woven.process.exec("swaymsg", args)
    return out ~= "" and out or nil
end

-- Focus a workspace by name or number.
function M.focus_workspace(target)
    M.msg("workspace", tostring(target))
end

-- Move the focused container to a workspace by name or number.
function M.move_to_workspace(target)
    M.msg("move", "container", "to", "workspace", tostring(target))
end

-- Toggle float on the focused container.
function M.toggle_float()
    M.msg("floating", "toggle")
end

-- Toggle fullscreen on the focused container.
function M.toggle_fullscreen()
    M.msg("fullscreen", "toggle")
end

-- Close the focused window.
function M.close_window()
    M.msg("kill")
end

-- Focus the next output/monitor.
function M.focus_next_output()
    M.msg("focus", "output", "next")
end

-- Move the focused container to the next output.
function M.move_to_next_output()
    M.msg("move", "container", "to", "output", "next")
end

-- Returns workspaces via woven compositor IPC (same source as overlay).
function M.workspaces()
    return woven.compositor.workspaces()
end

return M
