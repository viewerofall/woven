local M = { _workspaces = {}, _active_id = nil }
function M.refresh()
    local ws = woven.compositor.workspaces()
    M._workspaces = ws
    woven.log.info("workspace: loaded " .. #ws .. " workspaces")
end
function M.all()   return M._workspaces end
function M.get(id)
    for _, ws in ipairs(M._workspaces) do
        if ws.id == id then return ws end
    end
end
function M.active()    return M.get(M._active_id) end
function M.set_active(id) M._active_id = id end
return M
