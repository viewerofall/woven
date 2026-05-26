-- compositor/niri.lua
-- Niri-specific API surface.
-- Auto-loaded by boot.lua when NIRI_SOCKET is set.
-- Exposed as woven.niri.*
--
-- All shell calls go through woven.process.exec — io/os are stripped by the sandbox.

local M = {}

-- Send a raw niri msg action.
-- e.g. woven.niri.action("focus-workspace-up")
-- e.g. woven.niri.action("move-column-to-workspace", "3")
function M.action(name, ...)
    local args = { "msg", "action", name }
    for _, v in ipairs({ ... }) do args[#args + 1] = tostring(v) end
    woven.process.exec("niri", args)
end

-- Focus a workspace by 1-based index.
function M.focus_workspace(idx)
    M.action("focus-workspace", tostring(idx))
end

-- Move the focused column to a workspace by index.
function M.move_to_workspace(idx)
    M.action("move-column-to-workspace", tostring(idx))
end

-- Focus the next workspace.
function M.focus_next()
    M.action("focus-workspace-down")
end

-- Focus the previous workspace.
function M.focus_prev()
    M.action("focus-workspace-up")
end

-- Toggle fullscreen on the focused window.
function M.toggle_fullscreen()
    M.action("fullscreen-window")
end

-- Close the focused window.
function M.close_window()
    M.action("close-window")
end

-- Returns workspaces via woven compositor IPC (same source as overlay).
function M.workspaces()
    return woven.compositor.workspaces()
end

return M
